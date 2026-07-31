//! Parquet query engine — cold-storage read path.
//!
//! Reads Parquet files from S3/local storage and extracts specific body fields
//! for a given call_id. Uses column projection to minimize I/O.
//!
//! Stage 83 added `query_parquet_with_cache`: a footer-cached, row-group-aware
//! range read that avoids re-fetching the parquet footer (and full file) on
//! repeated queries to the same object.

use std::sync::Arc;

use arrow::array::{Array, StringArray};
use parquet::arrow::arrow_reader::{
    ArrowReaderMetadata, ParquetRecordBatchReaderBuilder as SyncReaderBuilder,
};
use parquet::arrow::async_reader::ParquetObjectReader;
use parquet::arrow::{ParquetRecordBatchStreamBuilder, ProjectionMask};
use parquet::file::metadata::ParquetMetaData;

use crate::body_archive::FooterCache;

/// Body payload returned from Parquet cold storage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BodyPayload {
    pub messages: Option<serde_json::Value>,
    pub response: Option<serde_json::Value>,
    pub proxy_server_request: Option<serde_json::Value>,
}

/// Decode a Parquet file (in-memory bytes) and extract a specific call_id's body.
///
/// Column-name compatibility (Stage 85): parquet files written before the
/// request_id→call_id rename have a `request_id` column; new files have
/// `call_id`.  Resolve the row-key column name by trying `call_id` first,
/// then falling back to `request_id`.
pub fn decode_body_from_parquet(
    parquet_data: &[u8],
    target_call_id: &str,
) -> Result<Option<BodyPayload>, String> {
    if parquet_data.is_empty() {
        return Err("empty parquet data".into());
    }

    // Use Bytes for ChunkReader compatibility
    let bytes = bytes::Bytes::copy_from_slice(parquet_data);
    let builder = SyncReaderBuilder::try_new(bytes)
        .map_err(|e| format!("Parquet reader: {}", e))?;

    // Resolve the row-key column name: prefer `call_id` (new schema),
    // fall back to `request_id` (pre-rename parquet files).
    let schema_desc = builder.metadata().file_metadata().schema_descr();
    let key_col = schema_desc
        .columns()
        .iter()
        .find(|c| c.name() == "call_id")
        .map(|_| "call_id")
        .or_else(|| {
            schema_desc
                .columns()
                .iter()
                .any(|c| c.name() == "request_id")
                .then_some("request_id")
        })
        .ok_or_else(|| "parquet schema has neither call_id nor request_id column".to_string())?;

    // Build projection: only read the key col + messages, response, proxy_server_request
    let mask = ProjectionMask::columns(
        &schema_desc,
        [key_col, "messages", "response", "proxy_server_request"],
    );

    let reader = builder
        .with_projection(mask)
        .build()
        .map_err(|e| format!("Build reader: {}", e))?;

    for batch_result in reader {
        let batch = batch_result.map_err(|e| format!("Read batch: {}", e))?;
        let num_rows = batch.num_rows();

        let request_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or("key column not StringArray")?;

        for row in 0..num_rows {
            if request_col.value(row) == target_call_id {
                let messages = extract_json_column(&batch, 1, row);
                let response = extract_json_column(&batch, 2, row);
                let proxy_server_request = extract_json_column(&batch, 3, row);

                return Ok(Some(BodyPayload {
                    messages,
                    response,
                    proxy_server_request,
                }));
            }
        }
    }

    Ok(None)
}

fn extract_json_column(
    batch: &arrow::record_batch::RecordBatch,
    col_idx: usize,
    row: usize,
) -> Option<serde_json::Value> {
    if col_idx >= batch.num_columns() {
        return None;
    }
    let col = batch.column(col_idx);
    let string_col = col.as_any().downcast_ref::<StringArray>()?;
    if string_col.is_null(row) {
        return None;
    }
    let text = string_col.value(row);
    serde_json::from_str(text).ok()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 83: footer-cached range read (query_parquet_with_cache)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Query a Parquet object on object storage for a specific `call_id`,
/// caching the parsed footer (`ParquetMetaData`) so repeated queries to the
/// same object skip the footer round-trip.
///
/// Pipeline:
/// 1. `footer_cache.get(path)` hit → reuse metadata; miss → `head` + range-read
///    the footer via `ParquetObjectReader`/`ArrowReaderMetadata`, then cache.
/// 2. Use the metadata + projected columns
///    (`call_id`, `messages`, `response`, `proxy_server_request`) to build
///    an async record-batch stream that only fetches the column chunks needed.
/// 3. Scan the decoded rows for `target_call_id` and return its body.
///
/// Column-name compatibility (Stage 85): resolves `call_id` first, falls back
/// to `request_id` for parquet files written before the rename.
///
/// `path_str` is the object-store location (e.g.
/// `year=2026/month=07/day=25/hour=14/data.parquet`).
pub async fn query_parquet_with_cache(
    store: &Arc<dyn object_store::ObjectStore>,
    footer_cache: &FooterCache,
    path_str: &str,
    target_call_id: &str,
) -> Result<Option<BodyPayload>, String> {
    use object_store::path::Path as ObjPath;

    let path = ObjPath::from(path_str);

    // 1. Obtain the object size via head(); needed to drive footer decoding.
    let meta = store
        .head(&path)
        .await
        .map_err(|e| format!("head {}: {}", path_str, e))?;

    // 2. Resolve footer metadata — cache hit avoids re-fetching the footer.
    let metadata = if let Some(cached) = footer_cache.get(path_str) {
        cached
    } else {
        // ParquetObjectReader drives all IO via the store (footer + col chunks).
        let mut reader = ParquetObjectReader::new(store.clone(), meta.clone());
        let arrow_meta = ArrowReaderMetadata::load_async(&mut reader, Default::default())
            .await
            .map_err(|e| format!("load footer metadata: {}", e))?;
        let md = arrow_meta.metadata().clone();
        footer_cache.put(path_str, md.clone());
        md
    };

    // 3. Build a stream that reads only the projected columns for the row
    //    groups that might contain `target_call_id`. Use bloom filters (if
    //    present) to prune row groups that definitely don't contain the target,
    //    avoiding ZSTD decompression + Arrow decoding of large JSON columns.
    let schema_desc = metadata.file_metadata().schema_descr();
    // Resolve key column name: prefer `call_id` (new), fall back to `request_id` (old).
    let key_col = schema_desc
        .columns()
        .iter()
        .find(|c| c.name() == "call_id")
        .map(|_| "call_id")
        .or_else(|| {
            schema_desc
                .columns()
                .iter()
                .any(|c| c.name() == "request_id")
                .then_some("request_id")
        })
        .ok_or_else(|| "parquet schema has neither call_id nor request_id column".to_string())?;

    // Find key column index (for bloom filter lookup).
    let key_col_idx = schema_desc
        .columns()
        .iter()
        .position(|c| c.name() == key_col)
        .unwrap_or(0);

    let mask = ProjectionMask::columns(
        &schema_desc,
        [key_col, "messages", "response", "proxy_server_request"],
    );

    let reader = ParquetObjectReader::new(store.clone(), meta);
    // Clone metadata: try_new_arrow_reader_metadata consumes it, but we still
    // need the original for row group iteration below.
    let arrow_meta = try_new_arrow_reader_metadata(Arc::clone(&metadata))?;
    let mut builder = ParquetRecordBatchStreamBuilder::new_with_metadata(reader, arrow_meta);

    let total_row_groups = metadata.row_groups().len();
    let mut pruned = 0usize;
    let mut passing_indices: Vec<usize> = Vec::with_capacity(total_row_groups);

    #[cfg(debug_assertions)]
    let bloom_start = std::time::Instant::now();
    #[cfg(debug_assertions)]
    let mut bloom_checks: usize = 0;

    for rg_idx in 0..total_row_groups {
        let rg_meta = metadata.row_group(rg_idx);
        let col_meta = rg_meta.column(key_col_idx);

        // Only check bloom filter if it was written (offset is Some).
        if col_meta.bloom_filter_offset().is_some() {
            #[cfg(debug_assertions)]
            {
                bloom_checks += 1;
            }
            // Try to read + probe the bloom filter. This does a byte-range IO.
            match builder
                .get_row_group_column_bloom_filter(rg_idx, key_col_idx)
                .await
            {
                Ok(Some(sbbf)) => {
                    let key: &str = target_call_id;
                    if sbbf.check(&key) {
                        passing_indices.push(rg_idx);
                    } else {
                        pruned += 1;
                    }
                }
                _ => {
                    // Bloom filter read failed — conservatively include.
                    passing_indices.push(rg_idx);
                }
            }
        } else {
            // No bloom filter written (too few rows) — include.
            passing_indices.push(rg_idx);
        }
    }

    #[cfg(debug_assertions)]
    {
        let bloom_elapsed = bloom_start.elapsed();
        tracing::trace!(
            target_call_id,
            %path_str,
            total_row_groups,
            scanned = passing_indices.len(),
            pruned,
            bloom_checks,
            bloom_check_ms = bloom_elapsed.as_secs_f64() * 1000.0,
            "🍉 parquet bloom filter: checked={} scanned={}/{} pruned={} bloom_ms={:.2}",
            bloom_checks, passing_indices.len(), total_row_groups, pruned,
            bloom_elapsed.as_secs_f64() * 1000.0,
        );
    }

    let stream = builder
        .with_row_groups(passing_indices)
        .with_projection(mask)
        .build()
        .map_err(|e| format!("build stream: {}", e))?;

    #[cfg(debug_assertions)]
    let scan_start = std::time::Instant::now();

    let mut found: Option<BodyPayload> = None;
    tokio::pin!(stream);
    while let Some(batch_result) = futures::StreamExt::next(&mut stream).await {
        let batch = batch_result.map_err(|e| format!("read batch: {}", e))?;
        if let Some(body) = find_body_in_batch(&batch, target_call_id) {
            found = Some(body);
            break;
        }
    }

    #[cfg(debug_assertions)]
    {
        use std::time::Instant;
        let scan_elapsed = scan_start.elapsed();
        tracing::trace!(
            target_call_id,
            %path_str,
            scan_ms = scan_elapsed.as_secs_f64() * 1000.0,
            "🍉 parquet scan: scan_ms={:.2}",
            scan_elapsed.as_secs_f64() * 1000.0,
        );
    }

    Ok(found)
}

/// Construct an `ArrowReaderMetadata` from a known `ParquetMetaData` without
/// re-decoding the footer. `ArrowReaderMetadata::try_new` is the supported
/// path (it re-derives the arrow schema from the parquet schema).
fn try_new_arrow_reader_metadata(
    metadata: Arc<ParquetMetaData>,
) -> Result<ArrowReaderMetadata, String> {
    ArrowReaderMetadata::try_new(metadata, Default::default())
        .map_err(|e| format!("ArrowReaderMetadata::try_new: {}", e))
}

/// Scan a record batch for the target call_id and return its body fields.
fn find_body_in_batch(
    batch: &arrow::record_batch::RecordBatch,
    target_call_id: &str,
) -> Option<BodyPayload> {
    let num_rows = batch.num_rows();
    let request_col = batch
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| "key column not StringArray".to_string())
        .ok()?;

    for row in 0..num_rows {
        if request_col.value(row) == target_call_id {
            let messages = extract_json_column(batch, 1, row);
            let response = extract_json_column(batch, 2, row);
            let proxy_server_request = extract_json_column(batch, 3, row);
            return Some(BodyPayload {
                messages,
                response,
                proxy_server_request,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_archive::writer::write_parquet_to_buffer;
    use crate::body_archive::BodyRow;

    #[test]
    fn test_decode_finds_target_request_id() {
        let rows = vec![
            BodyRow {
                call_id: "req-001".into(),
                start_time: "2026-07-22T14:00:00+00:00".into(),
                model: "gpt-4".into(),
                status: Some("success".into()),
                cache_hit: None,
                session_id: None,
                messages: Some(r#"{"role":"user","content":"hello"}"#.into()),
                response: Some(r#"{"choices":[{"text":"hi"}]}"#.into()),
                proxy_server_request: Some(r#"{"url":"/v1/chat"}"#.into()),
                request_id: Some("chatcmpl-001".into()),
                spend: 0.01,
                total_tokens: 100,
                prompt_tokens: 30,
                completion_tokens: 70,
                end_time: "2026-07-22T14:01:00+00:00".into(),
                model_group: None,
            },
            BodyRow {
                call_id: "req-002".into(),
                start_time: "2026-07-22T14:01:00+00:00".into(),
                model: "claude-3".into(),
                status: Some("success".into()),
                cache_hit: None,
                session_id: None,
                messages: Some(r#"{"role":"user","content":"test"}"#.into()),
                response: Some(r#"{"content":[{"text":"r"}]}"#.into()),
                proxy_server_request: None,
                request_id: None,
                spend: 0.02,
                total_tokens: 200,
                prompt_tokens: 50,
                completion_tokens: 150,
                end_time: "2026-07-22T14:02:00+00:00".into(),
                model_group: Some("claude-group".into()),
            },
        ];

        let data = write_parquet_to_buffer(&rows, 5000, 10, "zstd", 6).expect("write parquet");

        let body = decode_body_from_parquet(&data, "req-001")
            .expect("decode")
            .expect("found");
        assert!(body.messages.is_some());
        assert!(body.proxy_server_request.is_some());

        let body2 = decode_body_from_parquet(&data, "req-002")
            .expect("decode")
            .expect("found");
        assert!(body2.messages.is_some());
        assert!(body2.proxy_server_request.is_none());
    }

    #[test]
    fn test_decode_missing_request_id() {
        let rows = vec![BodyRow {
            call_id: "only-one".into(),
            start_time: "2026-07-22T14:00:00+00:00".into(),
            model: "gpt-4".into(),
            status: None,
            cache_hit: None,
            session_id: None,
            messages: Some(r#"{}"#.into()),
            response: None,
            proxy_server_request: None,
            request_id: None,
            spend: 0.0,
            total_tokens: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            end_time: "2026-07-22T14:00:00+00:00".into(),
            model_group: None,
        }];

        let data = write_parquet_to_buffer(&rows, 5000, 10, "zstd", 6).expect("write");
        let result = decode_body_from_parquet(&data, "nonexistent").expect("decode");
        assert!(result.is_none(), "should return None for missing call_id");
    }

    #[test]
    fn test_decode_empty_parquet() {
        let result = decode_body_from_parquet(&[], "req-001");
        assert!(result.is_err(), "empty data should error");
    }

    #[test]
    fn test_decode_body_with_null_json_values() {
        let rows = vec![BodyRow {
            call_id: "req-null".into(),
            start_time: "2026-07-22T14:00:00+00:00".into(),
            model: "gpt-4".into(),
            status: None,
            cache_hit: None,
            session_id: None,
            messages: None, // null messages
            response: None,  // null response
            proxy_server_request: None,
            request_id: None,
            spend: 0.0,
            total_tokens: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            end_time: "2026-07-22T14:00:00+00:00".into(),
            model_group: None,
        }];

        let data = write_parquet_to_buffer(&rows, 5000, 10, "zstd", 6).expect("write");
        let body = decode_body_from_parquet(&data, "req-null")
            .expect("decode")
            .expect("found");
        assert!(body.messages.is_none());
        assert!(body.response.is_none());
    }
}
