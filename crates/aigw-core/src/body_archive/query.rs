//! Parquet query engine — cold-storage read path.
//!
//! Reads Parquet files from S3/local storage and extracts specific body fields
//! for a given request_id. Uses column projection to minimize I/O.

use arrow::array::{Array, StringArray};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;

/// Body payload returned from Parquet cold storage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BodyPayload {
    pub messages: Option<serde_json::Value>,
    pub response: Option<serde_json::Value>,
    pub proxy_server_request: Option<serde_json::Value>,
}

/// Decode a Parquet file (in-memory bytes) and extract a specific request_id's body.
pub fn decode_body_from_parquet(
    parquet_data: &[u8],
    target_request_id: &str,
) -> Result<Option<BodyPayload>, String> {
    if parquet_data.is_empty() {
        return Err("empty parquet data".into());
    }

    // Use Bytes for ChunkReader compatibility
    let bytes = bytes::Bytes::copy_from_slice(parquet_data);
    let builder = ParquetRecordBatchReaderBuilder::try_new(bytes)
        .map_err(|e| format!("Parquet reader: {}", e))?;

    // Build projection: only read request_id, messages, response, proxy_server_request
    let schema_desc = builder.metadata().file_metadata().schema_descr();
    let mask = parquet::arrow::ProjectionMask::columns(
        &schema_desc,
        ["request_id", "messages", "response", "proxy_server_request"],
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
            .ok_or("request_id not StringArray")?;

        for row in 0..num_rows {
            if request_col.value(row) == target_request_id {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body_archive::writer::write_parquet_to_buffer;
    use crate::body_archive::BodyRow;

    #[test]
    fn test_decode_finds_target_request_id() {
        let rows = vec![
            BodyRow {
                request_id: "req-001".into(),
                start_time: "2026-07-22T14:00:00+00:00".into(),
                model: "gpt-4".into(),
                status: Some("success".into()),
                cache_hit: None,
                session_id: None,
                messages: Some(r#"{"role":"user","content":"hello"}"#.into()),
                response: Some(r#"{"choices":[{"text":"hi"}]}"#.into()),
                proxy_server_request: Some(r#"{"url":"/v1/chat"}"#.into()),
            },
            BodyRow {
                request_id: "req-002".into(),
                start_time: "2026-07-22T14:01:00+00:00".into(),
                model: "claude-3".into(),
                status: Some("success".into()),
                cache_hit: None,
                session_id: None,
                messages: Some(r#"{"role":"user","content":"test"}"#.into()),
                response: Some(r#"{"content":[{"text":"r"}]}"#.into()),
                proxy_server_request: None,
            },
        ];

        let data = write_parquet_to_buffer(&rows, 5000).expect("write parquet");

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
            request_id: "only-one".into(),
            start_time: "2026-07-22T14:00:00+00:00".into(),
            model: "gpt-4".into(),
            status: None,
            cache_hit: None,
            session_id: None,
            messages: Some(r#"{}"#.into()),
            response: None,
            proxy_server_request: None,
        }];

        let data = write_parquet_to_buffer(&rows, 5000).expect("write");
        let result = decode_body_from_parquet(&data, "nonexistent").expect("decode");
        assert!(result.is_none(), "should return None for missing request_id");
    }

    #[test]
    fn test_decode_empty_parquet() {
        let result = decode_body_from_parquet(&[], "req-001");
        assert!(result.is_err(), "empty data should error");
    }

    #[test]
    fn test_decode_body_with_null_json_values() {
        let rows = vec![BodyRow {
            request_id: "req-null".into(),
            start_time: "2026-07-22T14:00:00+00:00".into(),
            model: "gpt-4".into(),
            status: None,
            cache_hit: None,
            session_id: None,
            messages: None, // null messages
            response: None,  // null response
            proxy_server_request: None,
        }];

        let data = write_parquet_to_buffer(&rows, 5000).expect("write");
        let body = decode_body_from_parquet(&data, "req-null")
            .expect("decode")
            .expect("found");
        assert!(body.messages.is_none());
        assert!(body.response.is_none());
    }
}
