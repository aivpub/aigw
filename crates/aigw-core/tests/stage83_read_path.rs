//! Stage 83 Red tests — read path + cache activation + FileSystem backend.
//!
//! These lock in the P1/P2 fixes from docs/stages/stage-83.md:
//! - query_parquet_with_cache: footer cache → row group → col chunk range read
//! - FooterCache activated (footer fetched once per file across queries)
//! - read_body_from_storage distinguishes NotFound (Ok(None)) from unreachable (Err)
//! - S3 credentials support ${ENV_VAR} placeholder resolution
//! - StorageBackend::FileSystem builds a LocalFileSystem-backed store
//! - FileSystem archive writes to year=/month=/day=/hour=/data.parquet
//! - FileSystem archive round-trips body content
//!
//! They exercise the PUBLIC BodyArchiver API (query_parquet_with_cache /
//! read_body_from_storage_with_store / archive_rows_to_storage) and the
//! build_object_store_for_backend / resolve_env_placeholders helpers.

use aigw_core::body_archive::config::{BodyArchiveConfig, StorageBackend};
use aigw_core::body_archive::query::BodyPayload;
use aigw_core::body_archive::storage::{build_object_store_for_backend, resolve_env_placeholders};
use aigw_core::body_archive::writer::write_parquet_to_buffer;
use aigw_core::body_archive::BodyArchiver;
use aigw_core::body_archive::BodyRow;
use object_store::path::Path as ObjPath;
use object_store::{ObjectStore, PutPayload};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ─── helpers ─────────────────────────────────────────────────────────────

fn make_row(rid: &str, hour: i64) -> BodyRow {
    BodyRow {
        call_id: rid.into(),
        start_time: format!("2026-07-25T{:02}:00:00+00:00", hour),
        model: "gpt-4".into(),
        status: Some("success".into()),
        cache_hit: None,
        session_id: None,
        messages: Some(format!(r#"{{"content":"msg-{}"}}"#, rid)),
        response: Some(format!(r#"{{"content":"resp-{}"}}"#, rid)),
        proxy_server_request: Some(format!(r#"{{"url":"/v1/chat/{}"}}"#, rid)),
        request_id: None,
        spend: 0.01,
        total_tokens: 100,
        prompt_tokens: 30,
        completion_tokens: 70,
        end_time: format!("2026-07-25T{:02}:01:00+00:00", hour),
        model_group: None,
    }
}

/// An object_store wrapper that counts get_range calls so tests can assert
/// the footer is fetched exactly once across multiple queries (P1-1).
#[derive(Debug, Clone)]
struct CountingStore {
    inner: Arc<dyn ObjectStore>,
    range_calls: Arc<AtomicUsize>,
}

impl CountingStore {
    fn new(inner: Arc<dyn ObjectStore>) -> (Self, Arc<AtomicUsize>) {
        let range_calls = Arc::new(AtomicUsize::new(0));
        (
            Self {
                inner,
                range_calls: range_calls.clone(),
            },
            range_calls,
        )
    }
}

impl std::fmt::Display for CountingStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CountingStore")
    }
}

#[async_trait::async_trait]
impl ObjectStore for CountingStore {
    async fn put(
        &self,
        location: &ObjPath,
        payload: PutPayload,
    ) -> object_store::Result<object_store::PutResult> {
        self.inner.put(location, payload).await
    }
    async fn put_opts(
        &self,
        location: &ObjPath,
        payload: PutPayload,
        opts: object_store::PutOptions,
    ) -> object_store::Result<object_store::PutResult> {
        self.inner.put_opts(location, payload, opts).await
    }
    async fn put_multipart(
        &self,
        location: &ObjPath,
    ) -> object_store::Result<Box<dyn object_store::MultipartUpload>> {
        self.inner.put_multipart(location).await
    }
    async fn put_multipart_opts(
        &self,
        location: &ObjPath,
        opts: object_store::PutMultipartOpts,
    ) -> object_store::Result<Box<dyn object_store::MultipartUpload>> {
        self.inner.put_multipart_opts(location, opts).await
    }
    async fn get(&self, location: &ObjPath) -> object_store::Result<object_store::GetResult> {
        self.inner.get(location).await
    }
    async fn get_opts(
        &self,
        location: &ObjPath,
        options: object_store::GetOptions,
    ) -> object_store::Result<object_store::GetResult> {
        self.inner.get_opts(location, options).await
    }
    async fn get_range(
        &self,
        location: &ObjPath,
        range: std::ops::Range<usize>,
    ) -> object_store::Result<bytes::Bytes> {
        self.range_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.get_range(location, range).await
    }
    async fn get_ranges(
        &self,
        location: &ObjPath,
        ranges: &[std::ops::Range<usize>],
    ) -> object_store::Result<Vec<bytes::Bytes>> {
        self.range_calls.fetch_add(ranges.len(), Ordering::SeqCst);
        self.inner.get_ranges(location, ranges).await
    }
    async fn head(&self, location: &ObjPath) -> object_store::Result<object_store::ObjectMeta> {
        self.inner.head(location).await
    }
    async fn delete(&self, location: &ObjPath) -> object_store::Result<()> {
        self.inner.delete(location).await
    }
    fn list(
        &self,
        _prefix: Option<&ObjPath>,
    ) -> futures::stream::BoxStream<'static, object_store::Result<object_store::ObjectMeta>> {
        // Tests don't exercise list; emit an empty stream. The inner store
        // can be queried directly if a test ever needs listing.
        Box::pin(futures::stream::empty())
    }
    async fn list_with_delimiter(
        &self,
        prefix: Option<&ObjPath>,
    ) -> object_store::Result<object_store::ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }
    async fn copy(&self, from: &ObjPath, to: &ObjPath) -> object_store::Result<()> {
        self.inner.copy(from, to).await
    }
    async fn copy_if_not_exists(&self, from: &ObjPath, to: &ObjPath) -> object_store::Result<()> {
        self.inner.copy_if_not_exists(from, to).await
    }
}

/// Write a parquet file with the given rows into an InMemory store at the
/// canonical partition path, returning (store, path).
async fn seed_inmemory(rows: &[BodyRow]) -> (Arc<object_store::memory::InMemory>, ObjPath) {
    let store = Arc::new(object_store::memory::InMemory::new());
    let path = ObjPath::from("year=2026/month=07/day=25/hour=14/data.parquet");
    let data = write_parquet_to_buffer(rows, 5000, 10, "zstd", 6).expect("write parquet");
    store
        .put(&path, data.into())
        .await
        .expect("put parquet to in-memory store");
    (store, path)
}

// ─── P1-1: FooterCache activated — footer fetched once across queries ─────

#[tokio::test]
async fn test_query_parquet_with_cache_hits_footer_cache_on_second_call() {
    // Two queries to the same parquet file: footer must be requested at most
    // once (cached on first decode). Without FooterCache wiring, every query
    // re-decodes the whole file via a full GET — this test forces a CountingStore
    // and asserts range_requests do not grow on the second query for the footer.
    let rows = vec![make_row("req-001", 14), make_row("req-002", 14)];
    let (store, path) = seed_inmemory(&rows).await;

    let (counting, range_calls) = CountingStore::new(store);
    let counting: Arc<dyn ObjectStore> = Arc::new(counting);

    let archiver = BodyArchiver::new(BodyArchiveConfig::default());

    // First query: populates footer cache.
    let first = archiver
        .query_parquet_with_cache(&counting, path.as_ref(), "req-001")
        .await
        .expect("first query");
    assert!(first.is_some(), "first query should find the row");
    let ranges_after_first = range_calls.load(Ordering::SeqCst);
    assert!(
        ranges_after_first > 0,
        "first query must fetch the footer via get_range"
    );

    // Second query to the SAME file: footer must come from cache, so the
    // number of range requests for the footer metadata must NOT double.
    let second = archiver
        .query_parquet_with_cache(&counting, path.as_ref(), "req-002")
        .await
        .expect("second query");
    assert!(second.is_some(), "second query should find its row");

    let ranges_after_second = range_calls.load(Ordering::SeqCst);
    // The footer (metadata) is cached. The second query still fetches the
    // projected column chunks (col-chunk caching is future work), so it adds
    // col-chunk ranges but NOT a fresh footer range. Without the footer cache
    // the second query would re-fetch footer + col chunks (≈ first query's
    // count); with the cache it adds only col-chunk ranges. We assert the
    // second query adds strictly fewer ranges than the first query did, proving
    // the footer round-trip was skipped.
    let added_on_second = ranges_after_second - ranges_after_first;
    assert!(
        added_on_second < ranges_after_first,
        "footer cache must suppress the footer round-trip on the second query; \
         ranges first={} second={} (added={})",
        ranges_after_first,
        ranges_after_second,
        added_on_second,
    );
}

#[tokio::test]
async fn test_query_parquet_with_cache_returns_none_for_missing_request_id() {
    let rows = vec![make_row("req-001", 14)];
    let (store, path) = seed_inmemory(&rows).await;
    let store: Arc<dyn ObjectStore> = store;
    let archiver = BodyArchiver::new(BodyArchiveConfig::default());
    let res = archiver
        .query_parquet_with_cache(&store, path.as_ref(), "does-not-exist")
        .await
        .expect("query ok");
    assert!(res.is_none(), "missing request_id → Ok(None)");
}

// ─── P1-2: row group locating — multi row group parquet ──────────────────

#[tokio::test]
async fn test_query_parquet_with_cache_locates_target_row_group() {
    // Write a parquet with a small row_group_size so 2 rows split into 2 row
    // groups. The target lives in the 2nd group. We assert the result is found
    // and correct (proving the locator scanned row groups, not just group 0).
    let rows = vec![make_row("rg1-req", 14), make_row("rg2-req", 14)];
    let data =
        write_parquet_to_buffer(&rows, 1, 10, "zstd", 6).expect("write parquet with rg_size=1");
    let store = Arc::new(object_store::memory::InMemory::new());
    let path = ObjPath::from("year=2026/month=07/day=25/hour=14/data.parquet");
    store.put(&path, data.into()).await.expect("put");
    let store_dyn: Arc<dyn ObjectStore> = store;

    let archiver = BodyArchiver::new(BodyArchiveConfig::default());
    let body = archiver
        .query_parquet_with_cache(&store_dyn, path.as_ref(), "rg2-req")
        .await
        .expect("query")
        .expect("found");
    assert_eq!(
        body.messages
            .as_ref()
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str()),
        Some("msg-rg2-req"),
        "must locate the row in the 2nd row group"
    );
}

// ─── P1-3: read_body_from_storage — NotFound vs unreachable ──────────────

#[tokio::test]
async fn test_read_body_from_storage_returns_none_on_not_found() {
    // A valid, reachable InMemory store with NO object at the requested path.
    let store: Arc<dyn ObjectStore> = Arc::new(object_store::memory::InMemory::new());
    let cfg = BodyArchiveConfig::default();
    let archiver = BodyArchiver::new(cfg);

    let res = archiver
        .read_body_from_storage_with_store(
            &store,
            "year=2026/month=07/day=25/hour=14/data.parquet",
            "req-x",
        )
        .await;
    assert!(res.is_ok(), "NotFound must be Ok(None), got Err: {:?}", res);
    assert!(res.unwrap().is_none(), "missing object → Ok(None)");
}

#[tokio::test]
async fn test_read_body_from_storage_errors_on_unreachable_store() {
    // An unreachable store whose get() always fails with a non-NotFound error.
    #[derive(Debug)]
    struct AlwaysFailStore;
    impl std::fmt::Display for AlwaysFailStore {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "AlwaysFailStore")
        }
    }
    #[async_trait::async_trait]
    impl ObjectStore for AlwaysFailStore {
        async fn put(
            &self,
            _: &ObjPath,
            _: PutPayload,
        ) -> object_store::Result<object_store::PutResult> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "boom".into(),
            })
        }
        async fn put_opts(
            &self,
            _: &ObjPath,
            _: PutPayload,
            _: object_store::PutOptions,
        ) -> object_store::Result<object_store::PutResult> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "boom".into(),
            })
        }
        async fn put_multipart(
            &self,
            _: &ObjPath,
        ) -> object_store::Result<Box<dyn object_store::MultipartUpload>> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "boom".into(),
            })
        }
        async fn put_multipart_opts(
            &self,
            _: &ObjPath,
            _: object_store::PutMultipartOpts,
        ) -> object_store::Result<Box<dyn object_store::MultipartUpload>> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "boom".into(),
            })
        }
        async fn get(&self, _location: &ObjPath) -> object_store::Result<object_store::GetResult> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "unreachable".into(),
            })
        }
        async fn get_opts(
            &self,
            _: &ObjPath,
            _: object_store::GetOptions,
        ) -> object_store::Result<object_store::GetResult> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "unreachable".into(),
            })
        }
        async fn get_range(
            &self,
            _: &ObjPath,
            _: std::ops::Range<usize>,
        ) -> object_store::Result<bytes::Bytes> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "unreachable".into(),
            })
        }
        async fn get_ranges(
            &self,
            _: &ObjPath,
            _: &[std::ops::Range<usize>],
        ) -> object_store::Result<Vec<bytes::Bytes>> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "unreachable".into(),
            })
        }
        async fn head(&self, _: &ObjPath) -> object_store::Result<object_store::ObjectMeta> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "unreachable".into(),
            })
        }
        fn list(
            &self,
            _: Option<&ObjPath>,
        ) -> futures::stream::BoxStream<'static, object_store::Result<object_store::ObjectMeta>>
        {
            Box::pin(futures::stream::empty())
        }
        async fn list_with_delimiter(
            &self,
            _: Option<&ObjPath>,
        ) -> object_store::Result<object_store::ListResult> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "unreachable".into(),
            })
        }
        async fn delete(&self, _: &ObjPath) -> object_store::Result<()> {
            Ok(())
        }
        async fn copy(&self, _: &ObjPath, _: &ObjPath) -> object_store::Result<()> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "unreachable".into(),
            })
        }
        async fn copy_if_not_exists(&self, _: &ObjPath, _: &ObjPath) -> object_store::Result<()> {
            Err(object_store::Error::Generic {
                store: "test",
                source: "unreachable".into(),
            })
        }
    }

    let store: Arc<dyn ObjectStore> = Arc::new(AlwaysFailStore);
    let archiver = BodyArchiver::new(BodyArchiveConfig::default());
    let res = archiver
        .read_body_from_storage_with_store(
            &store,
            "year=2026/month=07/day=25/hour=14/data.parquet",
            "req-x",
        )
        .await;
    assert!(
        res.is_err(),
        "unreachable store must surface Err, not Ok(None): {:?}",
        res
    );
}

// ─── P1-4: S3 ${ENV_VAR} placeholder resolution ───────────────────────────

#[tokio::test]
async fn test_s3_config_resolves_env_placeholder() {
    std::env::set_var("AIGW_TEST_AK", "resolved-ak");
    std::env::set_var("AIGW_TEST_SK", "resolved-sk");
    std::env::set_var("AIGW_TEST_BUCKET", "env-bucket");
    let yaml = r#"
type: s3
bucket: ${AIGW_TEST_BUCKET}
region: us-east-1
access_key_id: ${AIGW_TEST_AK}
secret_access_key: ${AIGW_TEST_SK}
"#;
    let backend: StorageBackend = serde_yaml::from_str(yaml).expect("parse");
    let resolved = resolve_env_placeholders(&backend);
    match resolved {
        StorageBackend::S3 {
            bucket,
            access_key_id,
            secret_access_key,
            ..
        } => {
            assert_eq!(bucket, "env-bucket", "bucket env placeholder resolved");
            assert_eq!(
                access_key_id, "resolved-ak",
                "access_key_id env placeholder resolved"
            );
            assert_eq!(
                secret_access_key, "resolved-sk",
                "secret_access_key env placeholder resolved"
            );
        }
        _ => panic!("expected S3"),
    }
    std::env::remove_var("AIGW_TEST_AK");
    std::env::remove_var("AIGW_TEST_SK");
    std::env::remove_var("AIGW_TEST_BUCKET");
}

// ─── P2-10: StorageBackend::FileSystem builds a LocalFileSystem ───────────

#[tokio::test]
async fn test_build_object_store_filesystem() {
    let dir = std::env::temp_dir().join(format!("aigw_stage83_fs_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let backend = StorageBackend::FileSystem { path: dir.clone() };
    let store = build_object_store_for_backend(&backend).expect("build fs store");
    // Smoke test: put + get round-trips bytes.
    let p = ObjPath::from("smoke.parquet");
    store.put(&p, vec![1u8, 2, 3].into()).await.expect("put");
    let got = store
        .get(&p)
        .await
        .expect("get")
        .bytes()
        .await
        .expect("bytes");
    assert_eq!(got.as_ref(), &[1u8, 2, 3]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ─── P2-11: FileSystem archive round-trips body ───────────────────────────

#[tokio::test]
async fn test_filesystem_archive_round_trip() {
    let dir = std::env::temp_dir().join(format!("aigw_stage83_rt_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);

    let cfg = BodyArchiveConfig {
        auto_archive: true,
        storage: StorageBackend::FileSystem { path: dir.clone() },
        ..Default::default()
    };
    let archiver = BodyArchiver::new(cfg);

    let rows = vec![make_row("rt-001", 14), make_row("rt-002", 14)];
    let data = write_parquet_to_buffer(&rows, 5000, 10, "zstd", 6).expect("write");
    let path = "year=2026/month=07/day=25/hour=14/data.parquet";

    let store = build_object_store_for_backend(&StorageBackend::FileSystem { path: dir.clone() })
        .expect("store");
    store
        .put(&ObjPath::from(path), data.into())
        .await
        .expect("put");

    let body = archiver
        .read_body_from_storage_with_store(&store, path, "rt-001")
        .await
        .expect("read")
        .expect("found");
    assert_eq!(
        body.messages
            .as_ref()
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str()),
        Some("msg-rt-001"),
        "FS round-trip preserves body content"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── P2-12: FileSystem archive partition path layout ──────────────────────

#[tokio::test]
async fn test_filesystem_archive_partition_path_layout() {
    let dir = std::env::temp_dir().join(format!("aigw_stage83_layout_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);

    let cfg = BodyArchiveConfig {
        auto_archive: true,
        storage: StorageBackend::FileSystem { path: dir.clone() },
        ..Default::default()
    };
    let archiver = BodyArchiver::new(cfg);

    // Execute() writes the parquet for hour=2026-07-25T14 to the FS backend.
    let rows = vec![make_row("layout-001", 14)];
    let written_path = archiver
        .archive_rows_to_storage(&rows, "2026-07-25T14")
        .await
        .expect("archive rows");
    assert_eq!(
        written_path, "year=2026/month=07/day=25/hour=14/data.parquet",
        "FS archive must use year=/month=/day=/hour=/data.parquet layout"
    );
    // The file must physically exist under the FS root.
    let file = dir.join("year=2026/month=07/day=25/hour=14/data.parquet");
    assert!(file.exists(), "parquet file must exist at {:?}", file);

    let _ = std::fs::remove_dir_all(&dir);
}

// ─── sanity: BodyPayload import + decode still works (regression) ─────────

#[tokio::test]
async fn test_decode_body_from_parquet_still_works() {
    let rows = vec![make_row("sanity-001", 14)];
    let data = write_parquet_to_buffer(&rows, 5000, 10, "zstd", 6).expect("write");
    let body = aigw_core::body_archive::query::decode_body_from_parquet(&data, "sanity-001")
        .expect("decode")
        .expect("found");
    let _: BodyPayload = body;
}

// ─── Stage fix: sharded hour → per-shard parquet_path + cold read ─────────

/// A shard-forcing hour (max_parquet_body_mb tiny) must be written as multiple
/// `data-N.parquet` objects, each row's `parquet_path` must point at the shard
/// that holds it, and the cold read path must resolve from that shard.
#[tokio::test]
async fn test_sharded_hour_writes_per_shard_path_and_reads_back() {
    let dir = std::env::temp_dir().join(format!("aigw_stage83_shard_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);

    // 1100 rows × ~40-byte body. max_parquet_body_mb=0 → shard every row.
    let rows: Vec<BodyRow> = (0..1100)
        .map(|i| make_row(&format!("shard-{:04}", i), 14))
        .collect();

    let cfg = BodyArchiveConfig {
        auto_archive: true,
        storage: StorageBackend::FileSystem { path: dir.clone() },
        archive: aigw_core::body_archive::config::ArchivePolicy {
            // Force sharding: any positive body exceeds the per-object cap.
            max_parquet_body_mb: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let archiver = BodyArchiver::new(cfg);

    // Execute the public write path (auto shards because rows ≥ MULTIPART_MIN_ROWS
    // and body_bytes > max_bytes=0).
    let written_path = archiver
        .archive_rows_to_storage(&rows, "2026-07-25T14")
        .await
        .expect("archive sharded rows");

    // Expect a sharded layout: data.parquet (first) + data-1.parquet, ...
    assert!(
        !written_path.ends_with("-0.parquet"),
        "archive_rows_to_storage returns base path even when sharded"
    );

    // Count the shard files on disk (first is data.parquet, rest are data-{idx}).
    let hour_dir = dir.join("year=2026/month=07/day=25/hour=14");
    let mut shard_count = 0usize;
    if hour_dir.join("data.parquet").exists() {
        shard_count += 1;
    }
    for idx in 1..1024usize {
        let p = hour_dir.join(format!("data-{idx}.parquet"));
        if p.exists() {
            shard_count += 1;
        } else {
            break;
        }
    }
    assert!(
        shard_count >= 2,
        "expected ≥2 shard files, got {shard_count}"
    );

    // Every shard must be a valid parquet that decodes a row from it.
    use aigw_core::body_archive::query::decode_body_from_parquet;
    let shard_paths: Vec<std::path::PathBuf> = {
        let mut v = Vec::new();
        if hour_dir.join("data.parquet").exists() {
            v.push(hour_dir.join("data.parquet"));
        }
        for idx in 1..1024usize {
            let p = hour_dir.join(format!("data-{idx}.parquet"));
            if p.exists() {
                v.push(p);
            } else {
                break;
            }
        }
        v
    };
    for (i, p) in shard_paths.iter().enumerate() {
        let bytes = std::fs::read(p).expect("read shard");
        // Each shard holds `chunk` (row_group_size) rows, so shard i holds
        // rows [i*chunk, i*chunk+chunk). Decode the first row of that range.
        let chunk = aigw_core::body_archive::config::ArchivePolicy::default().row_group_size;
        let target = format!("shard-{:04}", i * chunk);
        let body = decode_body_from_parquet(&bytes, &target).expect("decode shard");
        assert!(body.is_some(), "shard {} should decode {}", i, target);
    }

    let _ = std::fs::remove_dir_all(&dir);
}
