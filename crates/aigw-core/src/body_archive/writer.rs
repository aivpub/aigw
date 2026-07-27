//! Parquet writer for body archive data.
//!
//! Converts BodyRow records into an Arrow RecordBatch, writes as Parquet
//! with ZSTD compression and Bloom filters on call_id, and uploads to storage.

use arrow::array::{BooleanBuilder, RecordBatch, StringBuilder, TimestampMillisecondBuilder};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use parquet::schema::types::ColumnPath;
use std::io::Cursor;
use std::sync::Arc;

use crate::body_archive::BodyRow;

/// Write body rows to a Parquet buffer and upload to object store.
/// Returns the number of bytes written.
pub async fn write_parquet_to_store(
    store: &dyn object_store::ObjectStore,
    path: &str,
    rows: &[BodyRow],
    row_group_size: usize,
) -> Result<usize, String> {
    if rows.is_empty() {
        return Ok(0);
    }

    let data = write_parquet_to_buffer(rows, row_group_size)?;
    let bytes = data.len();

    let path_obj = object_store::path::Path::from(path);
    store
        .put(&path_obj, data.into())
        .await
        .map_err(|e| format!("object_store put: {}", e))?;

    Ok(bytes)
}

/// Write Parquet to a local file path (for testing / filesystem backend).
pub fn write_parquet_to_file(
    path: &std::path::Path,
    rows: &[BodyRow],
    row_group_size: usize,
) -> Result<usize, String> {
    use std::io::Write;
    let data = write_parquet_to_buffer(rows, row_group_size)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
    }
    let mut file = std::fs::File::create(path).map_err(|e| format!("create file: {}", e))?;
    file.write_all(&data).map_err(|e| format!("write file: {}", e))?;
    Ok(data.len())
}

/// Write Parquet to an in-memory buffer. Returns the bytes.
pub fn write_parquet_to_buffer(rows: &[BodyRow], row_group_size: usize) -> Result<Vec<u8>, String> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let schema = Arc::new(Schema::new(vec![
        Field::new("call_id", DataType::Utf8, false),
        Field::new("start_time", DataType::Timestamp(TimeUnit::Millisecond, None), false),
        Field::new("model", DataType::Utf8, false),
        Field::new("status", DataType::Utf8, true),
        Field::new("cache_hit", DataType::Boolean, true),
        Field::new("session_id", DataType::Utf8, true),
        Field::new("messages", DataType::Utf8, true),
        Field::new("response", DataType::Utf8, true),
        Field::new("proxy_server_request", DataType::Utf8, true),
    ]));

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
    }

    let batch = RecordBatch::try_new(
        schema.clone(),
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
        ],
    )
    .map_err(|e| format!("RecordBatch: {}", e))?;

    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(
            ZstdLevel::try_new(3).map_err(|e| format!("ZstdLevel: {}", e))?
        ))
        .set_max_row_group_size(row_group_size)
        .set_column_bloom_filter_enabled(ColumnPath::from("call_id"), true)
        .set_column_bloom_filter_enabled(ColumnPath::from("session_id"), true)
        .set_dictionary_enabled(true)
        .build();

    let mut buffer = Cursor::new(Vec::new());
    {
        let mut writer = ArrowWriter::try_new(&mut buffer, schema, Some(props))
            .map_err(|e| format!("ArrowWriter: {}", e))?;
        writer.write(&batch).map_err(|e| format!("write batch: {}", e))?;
        writer.close().map_err(|e| format!("close writer: {}", e))?;
    }

    Ok(buffer.into_inner())
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

    #[test]
    fn test_write_empty_parquet() {
        let data = write_parquet_to_buffer(&[], 5000).expect("write empty");
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
        }];

        let data = write_parquet_to_buffer(&rows, 5000).expect("write single row");
        assert!(!data.is_empty(), "should produce output");
        // Parquet magic bytes: "PAR1"
        assert_eq!(&data[0..4], b"PAR1", "should start with PAR1 magic bytes");
    }

    #[test]
    fn test_write_multiple_rows_to_file() {
        let rows: Vec<BodyRow> = (0..100).map(|i| BodyRow {
            call_id: format!("req-{:04}", i),
            start_time: format!("2026-07-22T{:02}:00:00+00:00", i % 24),
            model: if i % 2 == 0 { "gpt-4".into() } else { "claude-3".into() },
            status: Some("success".into()),
            cache_hit: if i % 10 == 0 { Some("true".into()) } else { None },
            session_id: Some(format!("sess-{}", i / 10)),
            messages: Some(format!(r#"{{"content":"message {}"}}"#, i)),
            response: Some(format!(r#"{{"content":"response {}"}}"#, i)),
            proxy_server_request: None,
        }).collect();

        let dir = std::env::temp_dir().join("aigw_parquet_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.parquet");

        let bytes = write_parquet_to_file(&path, &rows, 5000).expect("write to file");
        assert!(bytes > 0);
        assert!(path.exists());

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }
}
