//! Body archive configuration types.
//!
//! Defines BodyArchiveConfig, S3Config, ArchivePolicy, FooterCacheConfig,
//! ColChunkCacheConfig, and StorageBackend used by the BodyArchiver.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration for the body archive system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyArchiveConfig {
    /// Whether automatic cron-driven archiving is enabled. Default: false.
    #[serde(default, alias = "enabled")]
    pub auto_archive: bool,
    /// Storage backend — S3 or local filesystem.
    #[serde(default)]
    pub storage: StorageBackend,
    /// S3-compatible storage configuration (deprecated: use `storage` with type: s3 instead).
    #[serde(default)]
    pub s3: S3Config,
    /// Archive policy parameters.
    #[serde(default)]
    pub archive: ArchivePolicy,
    /// Footer cache mode.
    #[serde(default)]
    pub footer_cache: FooterCacheConfig,
    /// Column chunk cache mode.
    #[serde(default)]
    pub col_chunk_cache: ColChunkCacheConfig,
}

impl Default for BodyArchiveConfig {
    fn default() -> Self {
        Self {
            auto_archive: false,
            storage: StorageBackend::S3 {
                bucket: String::new(),
                region: String::new(),
                endpoint: None,
                access_key_id: String::new(),
                secret_access_key: String::new(),
                prefix: String::new(),
                use_ssl: true,
                compatibility_mode: false,
                url_style: default_url_style(),
            },
            s3: S3Config::default(),
            archive: ArchivePolicy::default(),
            footer_cache: FooterCacheConfig::default(),
            col_chunk_cache: ColChunkCacheConfig::default(),
        }
    }
}

/// S3-compatible storage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    /// S3 endpoint URL. Empty = AWS default.
    #[serde(default)]
    pub endpoint: String,
    /// AWS region.
    #[serde(default)]
    pub region: String,
    /// Bucket name.
    #[serde(default)]
    pub bucket: String,
    /// Key prefix in the bucket (e.g. "logs").
    #[serde(default)]
    pub prefix: String,
    /// Access key ID.
    #[serde(default)]
    pub access_key_id: String,
    /// Secret access key.
    #[serde(default)]
    pub secret_access_key: String,
    /// Optional session token.
    #[serde(default)]
    pub session_token: Option<String>,
    /// URL style: "vhost" or "path".
    #[serde(default = "default_url_style")]
    pub url_style: String,
    /// Enable compatibility mode for COS/R2.
    #[serde(default)]
    pub compatibility_mode: bool,
    /// Use SSL/TLS for connections.
    #[serde(default = "default_true")]
    pub use_ssl: bool,
}

fn default_url_style() -> String {
    "vhost".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            region: String::new(),
            bucket: String::new(),
            prefix: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            session_token: None,
            url_style: default_url_style(),
            compatibility_mode: false,
            use_ssl: true,
        }
    }
}

/// Archive policy parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivePolicy {
    /// Don't archive data newer than this many hours. Default: 1.
    #[serde(default = "default_archive_after_hours")]
    pub archive_after_hours: u32,
    /// Null-out body columns after this many days (if null_body_after_archive). Default: 7.
    #[serde(default = "default_null_body_after_days")]
    pub null_body_after_days: u32,
    /// Rows per batch during archive. Default: 5000.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Parquet row group size. Default: 10.
    #[serde(default = "default_row_group_size")]
    pub row_group_size: usize,
    /// Seconds between tick() calls. Default: 300 (5 min).
    #[serde(default = "default_check_interval")]
    pub check_interval_secs: u64,
    /// Whether to null body columns in DB after archive. Default: true.
    #[serde(default = "default_true")]
    pub null_body_after_archive: bool,
    /// Whether to VACUUM after nulling (SQLite only). Default: true.
    #[serde(default = "default_true")]
    pub vacuum_after_null: bool,
    /// Skip Bloom filter when fewer than this many rows. Default: 10.
    #[serde(default = "default_bloom_min_rows")]
    pub bloom_min_rows: usize,
    /// Parquet compression codec. Default: "zstd".
    #[serde(default = "default_compression")]
    pub compression: String,
    /// Compression level. zstd: 1-22, gzip: 1-9. Default: 6.
    #[serde(default = "default_compression_level")]
    pub compression_level: u32,
    /// S3 multipart upload part size (MiB). Hours with ≥ [`crate::body_archive::writer::MULTIPART_MIN_ROWS`]
    /// rows are streamed via a multipart upload with this part size so we
    /// never hold the whole compressed file in memory or issue one giant
    /// single-shot PUT. Must be ≥ 5 (S3 minimum). Default: 16.
    #[serde(default = "default_multipart_part_size_mb")]
    pub multipart_part_size_mb: u32,
    /// Per-object body-data cap (MiB) for a single hour's parquet. Hours whose
    /// body data exceeds this are split into multiple `data-N.parquet` shards
    /// so each upload stays small (flaky S3 endpoints hang on giant single PUTs).
    /// Default: 128.
    #[serde(default = "default_max_parquet_body_mb")]
    pub max_parquet_body_mb: u32,
}

fn default_archive_after_hours() -> u32 {
    1
}
fn default_null_body_after_days() -> u32 {
    7
}
fn default_batch_size() -> usize {
    5000
}
fn default_row_group_size() -> usize {
    10
}
fn default_check_interval() -> u64 {
    300
}
fn default_bloom_min_rows() -> usize {
    10
}
fn default_compression() -> String {
    "zstd".into()
}
fn default_compression_level() -> u32 {
    6
}
fn default_multipart_part_size_mb() -> u32 {
    16
}
fn default_max_parquet_body_mb() -> u32 {
    128
}

impl Default for ArchivePolicy {
    fn default() -> Self {
        Self {
            archive_after_hours: default_archive_after_hours(),
            null_body_after_days: default_null_body_after_days(),
            batch_size: default_batch_size(),
            row_group_size: default_row_group_size(),
            check_interval_secs: default_check_interval(),
            null_body_after_archive: true,
            vacuum_after_null: true,
            bloom_min_rows: default_bloom_min_rows(),
            compression: default_compression(),
            compression_level: default_compression_level(),
            multipart_part_size_mb: default_multipart_part_size_mb(),
            max_parquet_body_mb: default_max_parquet_body_mb(),
        }
    }
}

/// Footer cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum FooterCacheConfig {
    /// No caching — always re-fetch from S3.
    None,
    /// In-memory LRU cache (moka).
    Mem {
        /// Max number of entries. Default: 10000.
        #[serde(default = "default_cache_capacity")]
        max_capacity: u64,
        /// TTL in seconds. Default: 3600.
        #[serde(default = "default_footer_ttl")]
        ttl_secs: u64,
    },
    /// Redis-backed cache (not implemented yet).
    Redis {
        /// Redis connection URL.
        url: String,
        /// TTL in seconds. Default: 86400.
        #[serde(default = "default_redis_ttl")]
        ttl_secs: u64,
    },
}

fn default_cache_capacity() -> u64 {
    10000
}
fn default_footer_ttl() -> u64 {
    3600
}
fn default_redis_ttl() -> u64 {
    86400
}

impl Default for FooterCacheConfig {
    fn default() -> Self {
        Self::None
    }
}

/// Column chunk cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ColChunkCacheConfig {
    /// No caching.
    None,
    /// File-system backed LFU cache.
    Fs {
        /// Cache directory.
        dir: PathBuf,
        /// Max total size in MB. Default: 1024.
        #[serde(default = "default_max_size_mb")]
        max_size_mb: usize,
        /// Max single entry size in MB. Default: 100.
        #[serde(default = "default_max_entry_mb")]
        max_entry_mb: usize,
    },
}

fn default_max_size_mb() -> usize {
    1024
}
fn default_max_entry_mb() -> usize {
    100
}

impl Default for ColChunkCacheConfig {
    fn default() -> Self {
        Self::None
    }
}

/// Storage backend — S3-compatible or local filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageBackend {
    /// S3-compatible object storage.
    #[serde(rename = "s3")]
    S3 {
        bucket: String,
        region: String,
        #[serde(default)]
        endpoint: Option<String>,
        access_key_id: String,
        secret_access_key: String,
        #[serde(default)]
        prefix: String,
        #[serde(default = "default_true")]
        use_ssl: bool,
        #[serde(default)]
        compatibility_mode: bool,
        #[serde(default = "default_url_style")]
        url_style: String,
    },
    /// Local filesystem (for testing).
    #[serde(rename = "fs")]
    FileSystem { path: PathBuf },
}

impl Default for StorageBackend {
    fn default() -> Self {
        Self::S3 {
            bucket: String::new(),
            region: String::new(),
            endpoint: None,
            access_key_id: String::new(),
            secret_access_key: String::new(),
            prefix: String::new(),
            use_ssl: true,
            compatibility_mode: false,
            url_style: default_url_style(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_body_archive_config() {
        let cfg = BodyArchiveConfig::default();
        assert!(!cfg.auto_archive);
        assert_eq!(cfg.archive.archive_after_hours, 1);
        assert_eq!(cfg.archive.null_body_after_days, 7);
        assert_eq!(cfg.archive.batch_size, 5000);
        assert_eq!(cfg.archive.row_group_size, 10);
        assert_eq!(cfg.archive.check_interval_secs, 300);
        assert!(cfg.archive.null_body_after_archive);
        assert_eq!(cfg.archive.multipart_part_size_mb, 16);
        assert_eq!(cfg.archive.max_parquet_body_mb, 128);
    }

    #[test]
    fn test_deserialize_body_archive_config_full() {
        let yaml = r#"
auto_archive: true
s3:
  endpoint: "cos.ap-guangzhou.myqcloud.com"
  region: "ap-guangzhou"
  bucket: "aigw-logs"
  prefix: "logs"
  access_key_id: "AKIDxxx"
  secret_access_key: "secretxxx"
  url_style: "vhost"
  compatibility_mode: true
  use_ssl: true
archive:
  archive_after_hours: 1
  null_body_after_days: 7
  batch_size: 5000
  row_group_size: 10
  check_interval_secs: 300
  null_body_after_archive: true
  vacuum_after_null: true
  bloom_min_rows: 10
  compression: "zstd"
  compression_level: 6
footer_cache:
  mode: "mem"
  max_capacity: 10000
  ttl_secs: 3600
col_chunk_cache:
  mode: "none"
"#;
        let cfg: BodyArchiveConfig = serde_yaml::from_str(yaml).expect("parse yaml");
        assert!(cfg.auto_archive);
        assert_eq!(cfg.s3.bucket, "aigw-logs");
        assert_eq!(cfg.s3.endpoint, "cos.ap-guangzhou.myqcloud.com");
        assert!(cfg.s3.compatibility_mode);
        assert_eq!(cfg.archive.batch_size, 5000);
        assert_eq!(cfg.archive.row_group_size, 10);
        match cfg.footer_cache {
            FooterCacheConfig::Mem {
                max_capacity,
                ttl_secs,
            } => {
                assert_eq!(max_capacity, 10000);
                assert_eq!(ttl_secs, 3600);
            }
            _ => panic!("expected Mem footer_cache"),
        }
        match cfg.col_chunk_cache {
            ColChunkCacheConfig::None => {}
            _ => panic!("expected None col_chunk_cache"),
        }
    }

    #[test]
    fn test_deserialize_backward_compat_enabled_field() {
        // Old `enabled` field should map to `auto_archive` via #[serde(alias)].
        let yaml = r#"
enabled: true
storage:
  type: fs
  path: /data/archive
"#;
        let cfg: BodyArchiveConfig = serde_yaml::from_str(yaml).expect("parse yaml");
        assert!(cfg.auto_archive, "old 'enabled' should map to auto_archive");
    }

    #[test]
    fn test_storage_backend_s3() {
        let yaml = r#"
type: s3
bucket: my-bucket
region: us-east-1
endpoint: "https://s3.amazonaws.com"
access_key_id: "key"
secret_access_key: "secret"
prefix: "logs"
use_ssl: true
compatibility_mode: false
url_style: vhost
"#;
        let backend: StorageBackend = serde_yaml::from_str(yaml).expect("parse s3");
        match backend {
            StorageBackend::S3 { bucket, region, .. } => {
                assert_eq!(bucket, "my-bucket");
                assert_eq!(region, "us-east-1");
            }
            _ => panic!("expected S3"),
        }
    }

    #[test]
    fn test_storage_backend_fs() {
        let yaml = r#"
type: fs
path: /tmp/archive
"#;
        let backend: StorageBackend = serde_yaml::from_str(yaml).expect("parse fs");
        match backend {
            StorageBackend::FileSystem { path } => {
                assert_eq!(path, PathBuf::from("/tmp/archive"));
            }
            _ => panic!("expected FileSystem"),
        }
    }
}
