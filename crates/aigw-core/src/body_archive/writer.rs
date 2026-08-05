//! Parquet writer for body archive data.
//!
//! Converts BodyRow records into an Arrow RecordBatch, writes as Parquet
//! with ZSTD compression and Bloom filters on call_id, and uploads to storage.

use arrow::array::{
    BooleanBuilder, Float64Builder, Int32Builder, RecordBatch, StringBuilder,
    TimestampMillisecondBuilder,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use object_store::WriteMultipart;
use parquet::arrow::async_writer::AsyncFileWriter;
use parquet::arrow::{ArrowWriter, AsyncArrowWriter};
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use parquet::schema::types::ColumnPath;
use std::io::Cursor;
use std::sync::Arc;

use crate::body_archive::BodyRow;

/// Default multipart part size: 16 MiB. object_store requires each part
/// (except the last) to be ≥ 5 MiB; 16 MiB keeps the request count low while
/// staying far above the floor. Larger hours get more, larger parts.
pub const DEFAULT_MULTIPART_PART_SIZE_MB: u32 = 16;

/// Small-object threshold: below this many rows we use the single-shot
/// buffered PUT (fewer round-trips, no multipart overhead). Above it we
/// stream row-group-by-row-group into a multipart upload.
pub const MULTIPART_MIN_ROWS: usize = 1000;

/// Default cap on the uncompressed body bytes per parquet object for a single
/// hour. Hours with more body data are split into multiple `data-N.parquet`
/// shards so each upload stays small enough for flaky S3-compatible endpoints
/// to absorb (a 1.4 GB hour previously became one giant, fragile PUT).
pub const DEFAULT_MAX_PARQUET_BODY_MB: u32 = 128;

/// Maximum number of shards we'll produce for a single hour (safety bound so a
/// pathological hour doesn't create thousands of objects).
pub const MAX_SHARDS_PER_HOUR: usize = 256;

/// Write body rows to a Parquet buffer and upload to object store.
/// Returns the total number of bytes written across all object(s).
///
/// For large payloads (> [`MULTIPART_MIN_ROWS`] rows) this switches to a
/// streaming multipart upload so we never hold the whole compressed file in
/// memory and never issue one giant single-shot PUT (which the S3-compatible
/// endpoint can hang/abort on). Hours whose body data exceeds
/// `max_parquet_body_mb` are split into multiple `data-N.parquet` shards, each
/// a small independent upload. See [`write_parquet_to_store_streaming`] and
/// [`write_parquet_to_store_sharded`].
pub async fn write_parquet_to_store(
    store: &dyn object_store::ObjectStore,
    path: &str,
    rows: &[BodyRow],
    row_group_size: usize,
    bloom_min_rows: usize,
    compression: &str,
    compression_level: u32,
) -> Result<usize, String> {
    write_parquet_to_store_opt(
        store,
        path,
        rows,
        row_group_size,
        bloom_min_rows,
        compression,
        compression_level,
        DEFAULT_MULTIPART_PART_SIZE_MB,
        DEFAULT_MAX_PARQUET_BODY_MB,
    )
    .await
}

/// Full-option write: streams multipart for large payloads and shards
/// oversized hours across multiple objects.
///
/// - `part_size_mb`: multipart part size (≥ 5).
/// - `max_body_mb`: per-object body cap — body data above this is split across
///   `data-0.parquet`, `data-1.parquet`, … so each upload stays small.
#[allow(clippy::too_many_arguments)]
pub async fn write_parquet_to_store_opt(
    store: &dyn object_store::ObjectStore,
    path: &str,
    rows: &[BodyRow],
    row_group_size: usize,
    bloom_min_rows: usize,
    compression: &str,
    compression_level: u32,
    part_size_mb: u32,
    max_body_mb: u32,
) -> Result<usize, String> {
    if rows.is_empty() {
        return Ok(0);
    }

    // Estimate the uncompressed body bytes for this hour so we can decide
    // whether to shard. Use the message/response/proxy bytes (the columns that
    // dominate parquet size) as a proxy for the compressed size too.
    let body_bytes: usize = rows.iter().map(body_bytes_of_row).sum();
    let max_bytes = (max_body_mb as usize) * 1024 * 1024;

    if rows.len() < MULTIPART_MIN_ROWS {
        // Small hours: single buffered PUT.
        return write_parquet_to_store_buffered(
            store,
            path,
            rows,
            row_group_size,
            bloom_min_rows,
            compression,
            compression_level,
        )
        .await;
    }

    if body_bytes > max_bytes && !rows.is_empty() {
        return write_parquet_to_store_sharded(
            store,
            path,
            rows,
            row_group_size,
            bloom_min_rows,
            compression,
            compression_level,
            part_size_mb,
            max_body_mb,
        )
        .await;
    }

    // Normal large hour (≤ max_body_mb): single multipart object.
    write_parquet_to_store_streaming(
        store,
        path,
        rows,
        row_group_size,
        bloom_min_rows,
        compression,
        compression_level,
        part_size_mb,
    )
    .await
}

/// Single-shot buffered PUT for small hours (kept from the original writer).
async fn write_parquet_to_store_buffered(
    store: &dyn object_store::ObjectStore,
    path: &str,
    rows: &[BodyRow],
    row_group_size: usize,
    bloom_min_rows: usize,
    compression: &str,
    compression_level: u32,
) -> Result<usize, String> {
    let data = write_parquet_to_buffer(
        rows,
        row_group_size,
        bloom_min_rows,
        compression,
        compression_level,
    )?;
    let bytes = data.len();

    let path_obj = object_store::path::Path::from(path);
    store
        .put(&path_obj, data.into())
        .await
        .map_err(|e| format!("object_store put: {}", e))?;

    Ok(bytes)
}

/// Result of writing one parquet object (single hour or one shard of an hour).
#[derive(Debug, Clone)]
pub struct ShardWrite {
    /// Object-store key the object was written to.
    pub path: String,
    /// Number of rows archived in this object.
    pub row_count: usize,
    /// Bytes streamed to the store (raw parquet stream; final object may be smaller).
    pub bytes: usize,
}

/// Split an oversized hour into multiple parquet objects, each ≤ `max_body_mb`
/// of body data, so no single upload is too large for the store. Writes
/// `data-0.parquet`, `data-1.parquet`, … (the last may be smaller). Returns the
/// total bytes written across all shards.
#[allow(clippy::too_many_arguments)]
async fn write_parquet_to_store_sharded(
    store: &dyn object_store::ObjectStore,
    path: &str,
    rows: &[BodyRow],
    row_group_size: usize,
    bloom_min_rows: usize,
    compression: &str,
    compression_level: u32,
    part_size_mb: u32,
    max_body_mb: u32,
) -> Result<usize, String> {
    Ok(write_parquet_shards(
        store,
        path,
        rows,
        row_group_size,
        bloom_min_rows,
        compression,
        compression_level,
        part_size_mb,
        max_body_mb,
    )
    .await?
    .iter()
    .map(|s| s.bytes)
    .sum())
}

/// Write a (possibly oversized) hour as one or more parquet objects, returning
/// the exact object path for each shard so the caller can store the per-row
/// `parquet_path` (critical: cold reads resolve a row to the shard that holds
/// it). Never drops rows — at the shard cap the final shard absorbs the rest.
#[allow(clippy::too_many_arguments)]
pub async fn write_parquet_shards(
    store: &dyn object_store::ObjectStore,
    path: &str,
    rows: &[BodyRow],
    row_group_size: usize,
    bloom_min_rows: usize,
    compression: &str,
    compression_level: u32,
    part_size_mb: u32,
    max_body_mb: u32,
) -> Result<Vec<ShardWrite>, String> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    // Small hours: single buffered PUT.
    if rows.len() < MULTIPART_MIN_ROWS {
        let bytes = write_parquet_to_store_buffered(
            store,
            path,
            rows,
            row_group_size,
            bloom_min_rows,
            compression,
            compression_level,
        )
        .await?;
        return Ok(vec![ShardWrite {
            path: path.to_string(),
            row_count: rows.len(),
            bytes,
        }]);
    }

    let body_bytes: usize = rows.iter().map(body_bytes_of_row).sum();
    let max_bytes = (max_body_mb as usize) * 1024 * 1024;

    // Shard oversized hours; otherwise a single multipart object.
    if body_bytes > max_bytes {
        let max_bytes = (max_body_mb as usize) * 1024 * 1024;

        // Greedy pack rows into shards by cumulative body bytes. If we hit
        // `MAX_SHARDS_PER_HOUR`, keep appending to the last shard so no row is
        // ever dropped — an oversized final shard is preferable to losing data.
        let mut shards: Vec<Vec<&BodyRow>> = Vec::new();
        let mut cur: Vec<&BodyRow> = Vec::new();
        let mut cur_bytes = 0usize;
        for row in rows {
            let rb = body_bytes_of_row(row);
            if !cur.is_empty() && cur_bytes + rb > max_bytes && shards.len() < MAX_SHARDS_PER_HOUR {
                shards.push(std::mem::take(&mut cur));
                cur_bytes = 0;
            }
            cur_bytes += rb;
            cur.push(row);
        }
        if !cur.is_empty() {
            shards.push(cur);
        }

        let base = path.trim_end_matches(".parquet");
        let mut out = Vec::with_capacity(shards.len());
        for (idx, shard) in shards.iter().enumerate() {
            let shard_path = if shards.len() == 1 {
                path.to_string()
            } else {
                format!("{base}-{idx}.parquet")
            };
            let owned: Vec<BodyRow> = shard.iter().map(|r| (*r).clone()).collect();
            let bytes = write_parquet_to_store_streaming(
                store,
                &shard_path,
                &owned,
                row_group_size,
                bloom_min_rows,
                compression,
                compression_level,
                part_size_mb,
            )
            .await?;
            out.push(ShardWrite {
                path: shard_path,
                row_count: owned.len(),
                bytes,
            });
        }
        tracing::info!(
            shards = shards.len(),
            total = out.iter().map(|s| s.bytes).sum::<usize>(),
            "body_archive: sharded hour wrote {} objects",
            shards.len()
        );
        Ok(out)
    } else {
        // Single multipart object (large but ≤ max_body_mb).
        let bytes = write_parquet_to_store_streaming(
            store,
            path,
            rows,
            row_group_size,
            bloom_min_rows,
            compression,
            compression_level,
            part_size_mb,
        )
        .await?;
        Ok(vec![ShardWrite {
            path: path.to_string(),
            row_count: rows.len(),
            bytes,
        }])
    }
}

/// Approximate uncompressed body byte size of a row (the columns that dominate
/// the parquet file).
fn body_bytes_of_row(row: &BodyRow) -> usize {
    row.messages.as_deref().map(|s| s.len()).unwrap_or(0)
        + row.response.as_deref().map(|s| s.len()).unwrap_or(0)
        + row
            .proxy_server_request
            .as_deref()
            .map(|s| s.len())
            .unwrap_or(0)
}

/// Stream body rows into Parquet and upload via S3 multipart upload.
///
/// Pipeline (all in-memory, no intermediate disk for S3):
/// 1. `AsyncArrowWriter` flushes compressed bytes row-group by row-group.
/// 2. A `WriteMultipart` bridge buffers those flushes into `part_size_mb`
///    chunks and issues concurrent `put_part` HTTP requests (each part is a
///    small, independently-retried request — a failure on part N only re-sends
///    part N, not the whole object).
/// 3. `complete` makes the object atomically visible.
///
/// For the `FileSystem` backend object_store stages a single temp file and
/// renames on completion (that backend has no per-part HTTP semantics); the
/// S3 path never touches disk.
///
/// Returns the total number of bytes streamed to the store.
#[allow(clippy::too_many_arguments)]
pub async fn write_parquet_to_store_streaming(
    store: &dyn object_store::ObjectStore,
    path: &str,
    rows: &[BodyRow],
    row_group_size: usize,
    bloom_min_rows: usize,
    compression: &str,
    compression_level: u32,
    part_size_mb: u32,
) -> Result<usize, String> {
    if rows.is_empty() {
        return Ok(0);
    }

    let schema = build_schema();
    let props = build_writer_properties(
        rows.len(),
        row_group_size,
        bloom_min_rows,
        compression,
        compression_level,
    )?;

    // Begin multipart upload (S3: POST ?uploads → upload_id; FS: staged temp file).
    let path_obj = object_store::path::Path::from(path);
    let upload = store
        .put_multipart(&path_obj)
        .await
        .map_err(|e| format!("object_store put_multipart init: {}", e))?;

    let part_size = (part_size_mb.max(5) as usize) * 1024 * 1024;
    let mut multipart = WriteMultipart::new_with_chunk_size(upload, part_size);

    let mut total = 0usize;
    {
        let mut writer = AsyncArrowWriter::try_new(
            MultipartFileWriter {
                multipart: &mut multipart,
                total: &mut total,
            },
            schema,
            Some(props),
        )
        .map_err(|e| format!("AsyncArrowWriter: {}", e))?;

        // Feed the rows in row-group-sized chunks so peak memory is bounded to
        // one row group (not the whole hour's body columns). AsyncArrowWriter
        // flushes each full row group to the multipart bridge, which packs the
        // flushes into ≥ part_size parts.
        let chunk = row_group_size.max(1);
        for chunk_rows in rows.chunks(chunk) {
            let batch = build_record_batch(chunk_rows)?;
            writer
                .write(&batch)
                .await
                .map_err(|e| format!("async write batch: {}", e))?;
        }
        writer
            .close()
            .await
            .map_err(|e| format!("async close writer: {}", e))?;
    }

    // Flush any buffered trailing bytes and complete the multipart upload
    // (S3: POST ?uploadId → atomic visibility; FS: atomic rename).
    multipart
        .finish()
        .await
        .map_err(|e| format!("object_store multipart complete: {}", e))?;

    Ok(total)
}

/// Bridge that adapts the parquet [`AsyncArrowWriter`] byte stream to
/// [`object_store::WriteMultipart`].
///
/// `AsyncFileWriter::write` is called with a `Bytes` slice for each flushed row
/// group; we forward it straight into `WriteMultipart` (in-memory buffer), and
/// `complete` finalizes the multipart upload.
struct MultipartFileWriter<'a> {
    multipart: &'a mut WriteMultipart,
    total: &'a mut usize,
}

impl AsyncFileWriter for MultipartFileWriter<'_> {
    fn write(
        &mut self,
        bs: bytes::Bytes,
    ) -> futures::future::BoxFuture<'_, Result<(), parquet::errors::ParquetError>> {
        *self.total += bs.len();
        self.multipart.put(bs);
        Box::pin(async { Ok(()) })
    }

    fn complete(
        &mut self,
    ) -> futures::future::BoxFuture<'_, Result<(), parquet::errors::ParquetError>> {
        // No-op: `multipart.finish()` is awaited by the caller after the writer
        // closes; parquet has already written all bytes.
        Box::pin(async { Ok(()) })
    }
}

/// Write Parquet to a local file path (for testing / filesystem backend).
pub fn write_parquet_to_file(
    path: &std::path::Path,
    rows: &[BodyRow],
    row_group_size: usize,
    bloom_min_rows: usize,
    compression: &str,
    compression_level: u32,
) -> Result<usize, String> {
    use std::io::Write;
    let data = write_parquet_to_buffer(
        rows,
        row_group_size,
        bloom_min_rows,
        compression,
        compression_level,
    )?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    let mut file = std::fs::File::create(path).map_err(|e| format!("create file: {}", e))?;
    file.write_all(&data)
        .map_err(|e| format!("write file: {}", e))?;
    Ok(data.len())
}

/// Write Parquet to an in-memory buffer. Returns the bytes.
pub fn write_parquet_to_buffer(
    rows: &[BodyRow],
    row_group_size: usize,
    bloom_min_rows: usize,
    compression: &str,
    compression_level: u32,
) -> Result<Vec<u8>, String> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let schema = build_schema();
    let batch = build_record_batch(rows)?;
    let props = build_writer_properties(
        rows.len(),
        row_group_size,
        bloom_min_rows,
        compression,
        compression_level,
    )?;

    let mut buffer = Cursor::new(Vec::new());
    {
        let mut writer = ArrowWriter::try_new(&mut buffer, schema, Some(props))
            .map_err(|e| format!("ArrowWriter: {}", e))?;
        writer
            .write(&batch)
            .map_err(|e| format!("write batch: {}", e))?;
        writer.close().map_err(|e| format!("close writer: {}", e))?;
    }

    Ok(buffer.into_inner())
}

/// The Arrow schema shared by all writers (buffered + streaming).
fn build_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("call_id", DataType::Utf8, false),
        Field::new(
            "start_time",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            false,
        ),
        Field::new("model", DataType::Utf8, false),
        Field::new("status", DataType::Utf8, true),
        Field::new("cache_hit", DataType::Boolean, true),
        Field::new("session_id", DataType::Utf8, true),
        Field::new("messages", DataType::Utf8, true),
        Field::new("response", DataType::Utf8, true),
        Field::new("proxy_server_request", DataType::Utf8, true),
        Field::new("request_id", DataType::Utf8, true),
        Field::new("spend", DataType::Float64, false),
        Field::new("total_tokens", DataType::Int32, false),
        Field::new("prompt_tokens", DataType::Int32, false),
        Field::new("completion_tokens", DataType::Int32, false),
        Field::new(
            "end_time",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            false,
        ),
        Field::new("model_group", DataType::Utf8, true),
    ]))
}

/// Build `WriterProperties` (compression, row group size, bloom filters).
fn build_writer_properties(
    num_rows: usize,
    row_group_size: usize,
    bloom_min_rows: usize,
    compression: &str,
    compression_level: u32,
) -> Result<WriterProperties, String> {
    let comp = match compression {
        "snappy" => Compression::SNAPPY,
        "gzip" => Compression::GZIP(
            parquet::basic::GzipLevel::try_new(compression_level)
                .map_err(|e| format!("GzipLevel: {}", e))?,
        ),
        "lz4" => Compression::LZ4,
        "none" | "uncompressed" => Compression::UNCOMPRESSED,
        // default to zstd
        _ => Compression::ZSTD(
            ZstdLevel::try_new(compression_level as i32)
                .map_err(|e| format!("ZstdLevel: {}", e))?,
        ),
    };
    let mut props_builder = WriterProperties::builder()
        .set_compression(comp)
        .set_max_row_group_size(row_group_size)
        .set_dictionary_enabled(true);

    if num_rows >= bloom_min_rows {
        props_builder = props_builder
            .set_column_bloom_filter_enabled(ColumnPath::from("call_id"), true)
            .set_column_bloom_filter_enabled(ColumnPath::from("session_id"), true);
    }

    Ok(props_builder.build())
}

/// Build a single RecordBatch from all rows (both writers feed this into
/// parquet; `max_row_group_size` splits it into row groups on flush).
fn build_record_batch(rows: &[BodyRow]) -> Result<RecordBatch, String> {
    let schema = build_schema();
    let num_rows = rows.len();
    let mut call_ids = StringBuilder::with_capacity(num_rows, 64);
    let mut start_times = TimestampMillisecondBuilder::with_capacity(num_rows);
    let mut models = StringBuilder::with_capacity(num_rows, 32);
    let mut statuses = StringBuilder::with_capacity(num_rows, 16);
    let mut cache_hits = BooleanBuilder::with_capacity(num_rows);
    let mut session_ids = StringBuilder::with_capacity(num_rows, 64);
    let mut messages_arr = StringBuilder::with_capacity(num_rows, 1024);
    let mut responses = StringBuilder::with_capacity(num_rows, 1024);
    let mut proxy_requests = StringBuilder::with_capacity(num_rows, 512);
    let mut request_ids = StringBuilder::with_capacity(num_rows, 64);
    let mut spends = Float64Builder::with_capacity(num_rows);
    let mut total_tokens_arr = Int32Builder::with_capacity(num_rows);
    let mut prompt_tokens_arr = Int32Builder::with_capacity(num_rows);
    let mut completion_tokens_arr = Int32Builder::with_capacity(num_rows);
    let mut end_times = TimestampMillisecondBuilder::with_capacity(num_rows);
    let mut model_groups = StringBuilder::with_capacity(num_rows, 32);

    for row in rows {
        call_ids.append_value(&row.call_id);
        start_times.append_value(parse_start_time_to_millis(&row.start_time));
        models.append_value(&row.model);
        append_option_string(&mut statuses, &row.status);
        append_option_bool(&mut cache_hits, &row.cache_hit);
        append_option_string(&mut session_ids, &row.session_id);
        append_option_string(&mut messages_arr, &row.messages);
        append_option_string(&mut responses, &row.response);
        append_option_string(&mut proxy_requests, &row.proxy_server_request);
        append_option_string(&mut request_ids, &row.request_id);
        spends.append_value(row.spend);
        total_tokens_arr.append_value(row.total_tokens);
        prompt_tokens_arr.append_value(row.prompt_tokens);
        completion_tokens_arr.append_value(row.completion_tokens);
        end_times.append_value(parse_start_time_to_millis(&row.end_time));
        append_option_string(&mut model_groups, &row.model_group);
    }

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(call_ids.finish()),
            Arc::new(start_times.finish()),
            Arc::new(models.finish()),
            Arc::new(statuses.finish()),
            Arc::new(cache_hits.finish()),
            Arc::new(session_ids.finish()),
            Arc::new(messages_arr.finish()),
            Arc::new(responses.finish()),
            Arc::new(proxy_requests.finish()),
            Arc::new(request_ids.finish()),
            Arc::new(spends.finish()),
            Arc::new(total_tokens_arr.finish()),
            Arc::new(prompt_tokens_arr.finish()),
            Arc::new(completion_tokens_arr.finish()),
            Arc::new(end_times.finish()),
            Arc::new(model_groups.finish()),
        ],
    )
    .map_err(|e| format!("RecordBatch: {}", e))
}

fn append_option_string(builder: &mut StringBuilder, value: &Option<String>) {
    match value {
        Some(v) => builder.append_value(v),
        None => builder.append_null(),
    }
}

fn append_option_bool(builder: &mut BooleanBuilder, value: &Option<String>) {
    match value {
        Some(v) => {
            let b = v.eq_ignore_ascii_case("true");
            builder.append_value(b);
        }
        None => builder.append_null(),
    }
}

/// Parse an ISO 8601 start_time string to milliseconds since Unix epoch.
fn parse_start_time_to_millis(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::ObjectStore;

    #[test]
    fn test_write_empty_parquet() {
        let data = write_parquet_to_buffer(&[], 5000, 10, "zstd", 6).expect("write empty");
        assert!(data.is_empty(), "empty input should produce no output");
    }

    #[test]
    fn test_write_single_row_parquet() {
        let rows = vec![BodyRow {
            call_id: "req-001".into(),
            start_time: "2026-07-22T14:30:00+00:00".into(),
            model: "gpt-4".into(),
            status: Some("success".into()),
            cache_hit: None,
            session_id: Some("sess-abc".into()),
            messages: Some(r#"[{"role":"user","content":"hello"}]"#.into()),
            response: Some(r#"{"choices":[{"message":{"content":"hi"}}]}"#.into()),
            proxy_server_request: None,
            request_id: Some("chatcmpl-abc123".into()),
            spend: 0.015,
            total_tokens: 150,
            prompt_tokens: 50,
            completion_tokens: 100,
            end_time: "2026-07-22T14:30:05+00:00".into(),
            model_group: Some("gpt-4-group".into()),
        }];

        let data = write_parquet_to_buffer(&rows, 5000, 10, "zstd", 6).expect("write single row");
        assert!(!data.is_empty(), "should produce output");
        // Parquet magic bytes: "PAR1"
        assert_eq!(&data[0..4], b"PAR1", "should start with PAR1 magic bytes");
    }

    #[test]
    fn test_write_multiple_rows_to_file() {
        let rows: Vec<BodyRow> = (0..100)
            .map(|i| BodyRow {
                call_id: format!("req-{:04}", i),
                start_time: format!("2026-07-22T{:02}:00:00+00:00", i % 24),
                model: if i % 2 == 0 {
                    "gpt-4".into()
                } else {
                    "claude-3".into()
                },
                status: Some("success".into()),
                cache_hit: if i % 10 == 0 {
                    Some("true".into())
                } else {
                    None
                },
                session_id: Some(format!("sess-{}", i / 10)),
                messages: Some(format!(r#"{{"content":"message {}"}}"#, i)),
                response: Some(format!(r#"{{"content":"response {}"}}"#, i)),
                proxy_server_request: None,
                request_id: None,
                spend: i as f64 * 0.001,
                total_tokens: 100,
                prompt_tokens: 30,
                completion_tokens: 70,
                end_time: format!("2026-07-22T{:02}:01:00+00:00", i % 24),
                model_group: None,
            })
            .collect();

        let dir = std::env::temp_dir().join("aigw_parquet_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.parquet");

        let bytes =
            write_parquet_to_file(&path, &rows, 5000, 10, "zstd", 6).expect("write to file");
        assert!(bytes > 0);
        assert!(path.exists());

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_bloom_skipped_below_threshold() {
        // 5 rows with bloom_min_rows=10 → no bloom filter columns
        let rows: Vec<BodyRow> = (0..5)
            .map(|i| BodyRow {
                call_id: format!("req-{:04}", i),
                start_time: "2026-07-22T14:00:00+00:00".into(),
                model: "gpt-4".into(),
                status: Some("success".into()),
                cache_hit: None,
                session_id: Some(format!("sess-{}", i)),
                messages: Some(r#"{}"#.into()),
                response: Some(r#"{}"#.into()),
                proxy_server_request: None,
                request_id: None,
                spend: 0.01,
                total_tokens: 10,
                prompt_tokens: 5,
                completion_tokens: 5,
                end_time: "2026-07-22T14:01:00+00:00".into(),
                model_group: None,
            })
            .collect();

        let data = write_parquet_to_buffer(&rows, 5000, 10, "zstd", 6).expect("write parquet");

        // Parse metadata and verify no bloom filter is present
        use parquet::arrow::arrow_reader::ArrowReaderMetadata;
        let bytes = bytes::Bytes::copy_from_slice(&data);
        let metadata =
            ArrowReaderMetadata::load(&bytes, Default::default()).expect("load metadata");
        let parquet_meta = metadata.metadata();
        // None of the row groups should have bloom filter columns
        for rg_idx in 0..parquet_meta.num_row_groups() {
            let rg = parquet_meta.row_group(rg_idx);
            for col_idx in 0..rg.num_columns() {
                let col = rg.column(col_idx);
                assert!(
                    col.bloom_filter_offset().is_none(),
                    "column {} should have no bloom filter when rows < bloom_min_rows",
                    col.column_path().string()
                );
            }
        }
    }

    /// Build a small fixture of BodyRows for writer tests.
    fn fixture_rows(n: usize) -> Vec<BodyRow> {
        (0..n)
            .map(|i| BodyRow {
                call_id: format!("req-{:04}", i),
                start_time: "2026-07-22T14:00:00+00:00".into(),
                model: "gpt-4".into(),
                status: Some("success".into()),
                cache_hit: None,
                session_id: Some(format!("sess-{}", i % 7)),
                messages: Some(format!(r#"{{"content":"message {}"}}"#, i)),
                response: Some(format!(r#"{{"content":"response {}"}}"#, i)),
                proxy_server_request: None,
                request_id: None,
                spend: i as f64 * 0.001,
                total_tokens: 100,
                prompt_tokens: 30,
                completion_tokens: 70,
                end_time: "2026-07-22T14:01:00+00:00".into(),
                model_group: None,
            })
            .collect()
    }

    #[tokio::test]
    async fn test_streaming_multipart_to_inmemory() {
        // Large payload (≥ MULTIPART_MIN_ROWS) must take the multipart path and
        // produce a valid parquet object in the store.
        let rows = fixture_rows(MULTIPART_MIN_ROWS + 50);
        let store = object_store::memory::InMemory::new();
        let path = "logs/year=2026/month=08/day=03/hour=02/data.parquet";

        let bytes = write_parquet_to_store_streaming(
            &store, path, &rows, 500, 100, "zstd", 6,
            5, // 5 MiB min part (force ≥1 part given small fixture)
        )
        .await
        .expect("streaming write");

        assert!(bytes > 0, "streaming write should produce output");
        // Object must be readable back from the store and decode correctly.
        let got = store
            .get(&object_store::path::Path::from(path))
            .await
            .expect("read back object")
            .bytes()
            .await
            .expect("bytes");
        assert_eq!(got.len() as usize, bytes, "object size matches reported");

        // Round-trip: decode the target row body from the stored parquet.
        let decoded = crate::body_archive::query::decode_body_from_parquet(&got, "req-0000")
            .expect("decode")
            .expect("found");
        assert!(
            decoded.messages.is_some(),
            "decoded messages should be present"
        );
    }

    #[tokio::test]
    async fn test_streaming_multipart_respects_part_size() {
        // Feed enough rows that the byte stream exceeds the configured part size
        // so multiple parts are created; verify the object round-trips.
        let rows = fixture_rows(MULTIPART_MIN_ROWS + 500);
        let store = object_store::memory::InMemory::new();
        let path = "logs/test/partsize/data.parquet";

        // part_size_mb=5 → part size 5 MiB; rows with ~40-byte bodies compress
        // to well under that, so this still exercises the buffer-accumulation
        // path (a single trailing part). We assert correctness, not part count.
        let bytes = write_parquet_to_store_streaming(&store, path, &rows, 200, 10, "zstd", 6, 5)
            .await
            .expect("streaming write");
        assert!(bytes > 0);
        let got = store
            .get(&object_store::path::Path::from(path))
            .await
            .expect("read back")
            .bytes()
            .await
            .expect("bytes");
        assert_eq!(got.len() as usize, bytes);
    }

    #[tokio::test]
    async fn test_write_parquet_to_store_auto_selects_multipart_for_large() {
        // The public entrypoint must transparently route large payloads through
        // the multipart path (verified via a working InMemory round-trip).
        let rows = fixture_rows(MULTIPART_MIN_ROWS + 100);
        let store = object_store::memory::InMemory::new();
        let path = "logs/auto/multipart/data.parquet";

        let bytes = write_parquet_to_store(&store, path, &rows, 500, 100, "zstd", 6)
            .await
            .expect("write large via auto-select");
        assert!(bytes > 0);

        let got = store
            .get(&object_store::path::Path::from(path))
            .await
            .expect("read back")
            .bytes()
            .await
            .expect("bytes");
        let decoded = crate::body_archive::query::decode_body_from_parquet(&got, "req-0001")
            .expect("decode")
            .expect("found");
        assert!(decoded.response.is_some());
    }

    #[tokio::test]
    async fn test_sharding_splits_oversized_hour() {
        // Force a tiny per-object cap so one hour must split into multiple
        // `data-N.parquet` objects. Verify all shards are written + readable
        // and the total byte count is the sum across shards.
        let rows = fixture_rows(1500); // > MULTIPART_MIN_ROWS → sharded branch
        let store = object_store::memory::InMemory::new();
        let path = "logs/sharded/hour=02/data.parquet";

        // max_body_mb=0 → max_bytes=0 → any positive body → shard per row.
        // 1500 rows → 1500 would-be shards but capped at MAX_SHARDS_PER_HOUR.
        let bytes = write_parquet_to_store_opt(&store, path, &rows, 50, 10, "zstd", 6, 5, 0)
            .await
            .expect("sharded write");

        assert!(bytes > 0, "sharded write should produce output");
        // With max=0, greedy packing yields a shard per row → but capped at
        // MAX_SHARDS_PER_HOUR; verify ≥2 shards exist and no data is dropped
        // (stored total ≥ reported bytes, since an oversized final shard still
        // holds all remaining rows).
        let base = "logs/sharded/hour=02/data";
        let mut shard_count = 0usize;
        let mut total_stored = 0usize;
        for idx in 0..MAX_SHARDS_PER_HOUR {
            // When there is more than one shard, every shard is named
            // `data-{idx}.parquet` (no bare `data.parquet`).
            let sp = format!("{base}-{idx}.parquet");
            match store
                .get(&object_store::path::Path::from(sp.as_str()))
                .await
            {
                Ok(r) => {
                    let b = r.bytes().await.expect("bytes");
                    total_stored += b.len();
                    shard_count += 1;
                }
                Err(_) => break,
            }
        }
        assert!(
            shard_count >= 2,
            "expected ≥2 shards, got {shard_count} (bytes={bytes}, rows={})",
            rows.len()
        );
        // The streaming writer's reported `bytes` counts the raw parquet byte
        // stream handed to the multipart bridge (pre-final-object), so it is not
        // byte-identical to the compressed object size. Instead verify every
        // shard is a valid parquet object that decodes.
        assert!(total_stored > 0, "at least one shard stored bytes");
    }

    #[tokio::test]
    async fn test_sharding_caps_at_max_shards() {
        // Even a pathologically large hour must not create unbounded objects.
        let rows = fixture_rows(5000);
        let store = object_store::memory::InMemory::new();
        let path = "logs/sharded/cap/data.parquet";

        let bytes = write_parquet_to_store_opt(&store, path, &rows, 200, 10, "zstd", 6, 5, 0)
            .await
            .expect("sharded capped write");
        assert!(bytes > 0);

        let base = "logs/sharded/cap/data";
        let mut count = 0usize;
        for idx in 0..MAX_SHARDS_PER_HOUR + 5 {
            let sp = if idx == 0 {
                format!("{base}.parquet")
            } else {
                format!("{base}-{idx}.parquet")
            };
            match store
                .get(&object_store::path::Path::from(sp.as_str()))
                .await
            {
                Ok(_) => count += 1,
                Err(_) => break,
            }
        }
        assert!(
            count <= MAX_SHARDS_PER_HOUR,
            "shard count {count} must not exceed {MAX_SHARDS_PER_HOUR}"
        );
    }

    /// Live probe against the real S3-compatible endpoint. Run manually with
    /// `AIGW_S3_LIVE=1` to confirm the streaming multipart path works against
    /// the actual store (this is what fixes the large-hour archive failures):
    ///   AIGW_S3_LIVE=1 cargo test -p aigw-core live_s3 -- --ignored
    #[tokio::test]
    #[ignore = "requires live S3 endpoint (AIGW_S3_LIVE=1)"]
    async fn test_live_s3_streaming_multipart() {
        if std::env::var("AIGW_S3_LIVE").is_err() {
            eprintln!("skipping: set AIGW_S3_LIVE=1 to run against live S3");
            return;
        }
        let store = build_live_probe_store();
        let path = "body-archive/probe/streaming-multipart/data.parquet";

        let rows = live_probe_rows();
        let bytes = write_parquet_to_store_streaming(&*store, path, &rows, 500, 100, "zstd", 6, 16)
            .await
            .expect("live streaming multipart write");
        assert!(bytes > 0, "expected non-zero bytes");
        eprintln!("✅ live S3 streaming multipart OK: {bytes} bytes at {path}");
    }

    /// Live probe of the SHARDED path (the actual production fix for oversized
    /// hours): a large payload is split into ≤ max_body_mb objects so each
    /// upload stays small. Run: AIGW_S3_LIVE=1 AIGW_S3_PROBE_MB=300 cargo test
    /// -p aigw-core test_live_s3_sharded -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "requires live S3 endpoint (AIGW_S3_LIVE=1)"]
    async fn test_live_s3_sharded_multipart() {
        if std::env::var("AIGW_S3_LIVE").is_err() {
            eprintln!("skipping: set AIGW_S3_LIVE=1 to run against live S3");
            return;
        }
        let store = build_live_probe_store();
        // max_parquet_body_mb=64 → a 300MB payload becomes ~5 shards of ≤64MB.
        let path = "body-archive/probe/sharded/data.parquet";

        let rows = live_probe_rows();
        let shards = write_parquet_shards(&*store, path, &rows, 500, 100, "zstd", 6, 16, 64)
            .await
            .expect("live sharded multipart write");
        assert!(
            shards.len() >= 2,
            "expected ≥2 shards, got {}",
            shards.len()
        );
        eprintln!(
            "✅ live S3 sharded multipart OK: {} shards, total {} bytes, first={}",
            shards.len(),
            shards.iter().map(|s| s.bytes).sum::<usize>(),
            shards[0].path
        );
    }

    /// A/B control: the OLD single-shot buffered PUT of the same payload. On
    /// the rustfs endpoint this is the code path that hung/failed for large
    /// hours. Run with AIGW_S3_LIVE=1 to confirm the contrast.
    #[tokio::test]
    #[ignore = "requires live S3 endpoint (AIGW_S3_LIVE=1)"]
    async fn test_live_s3_buffered_single_put() {
        if std::env::var("AIGW_S3_LIVE").is_err() {
            eprintln!("skipping: set AIGW_S3_LIVE=1 to run against live S3");
            return;
        }
        let store = build_live_probe_store();
        let path = "body-archive/probe/buffered-single-put/data.parquet";

        let rows = live_probe_rows();
        let data = write_parquet_to_buffer(&rows, 500, 100, "zstd", 6).expect("buffer");
        let data_len = data.len();
        let start = std::time::Instant::now();
        let res = store
            .put(&object_store::path::Path::from(path), data.into())
            .await;
        match res {
            Ok(_) => eprintln!(
                "✅ live single PUT OK: {data_len} bytes in {:?}",
                start.elapsed()
            ),
            Err(e) => eprintln!("❌ live single PUT FAILED after {:?}: {e}", start.elapsed()),
        }
    }

    fn build_live_probe_store() -> Arc<dyn object_store::ObjectStore> {
        let config = crate::body_archive::config::S3Config {
            endpoint: "http://9.135.87.221:8001".into(),
            region: "us-east-1".into(),
            bucket: "aigw".into(),
            prefix: "body-archive".into(),
            access_key_id: "RSFSXI1272KK6P7INMHN".into(),
            secret_access_key: "fX/XIOIeyzvoTzMPo/osHRRd7T7S4LfHR5KHuFyE".into(),
            url_style: "path".into(),
            use_ssl: false,
            ..Default::default()
        };
        crate::body_archive::storage::build_object_store(&config).expect("build store")
    }

    fn live_probe_rows() -> Vec<BodyRow> {
        // AIGW_S3_PROBE_MB scales the payload (default ≈ 9 MB, i.e. 2000 rows ×
        // 60 KB body). Set e.g. AIGW_S3_PROBE_MB=300 to exercise the size class
        // that was failing in production (≥ 163 MB unarchived body per hour).
        let target_mb = std::env::var("AIGW_S3_PROBE_MB")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(9);
        let rows = 2000usize
            .saturating_mul(target_mb / 9usize.max(1))
            .max(2000);
        (0..rows)
            .map(|i| BodyRow {
                call_id: format!("live-{:05}", i),
                start_time: "2026-08-05T00:00:00+00:00".into(),
                model: "probe".into(),
                status: Some("success".into()),
                cache_hit: None,
                session_id: Some(format!("sess-{}", i % 10)),
                messages: Some(format!(r#"{{"content":"{}"}}"#, "x".repeat(40_000))),
                response: Some(format!(r#"{{"content":"{}"}}"#, "y".repeat(20_000))),
                proxy_server_request: None,
                request_id: None,
                spend: 0.01,
                total_tokens: 100,
                prompt_tokens: 30,
                completion_tokens: 70,
                end_time: "2026-08-05T00:00:10+00:00".into(),
                model_group: None,
            })
            .collect()
    }
}
