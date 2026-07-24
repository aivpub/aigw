//! Footer cache — moka-based LRU cache for parsed Parquet metadata.
//!
//! Caches `ParquetMetaData` (row group offsets, column statistics, Bloom filter
//! locations) keyed by S3 path. Eliminates the first S3 round-trip (footer read)
//! on repeated queries to the same file.

use moka::sync::Cache;
use parquet::file::metadata::ParquetMetaData;
use std::sync::Arc;
use std::time::Duration;

/// Footer cache wrapping moka's sync LRU cache.
pub struct FooterCache {
    cache: Cache<String, Arc<ParquetMetaData>>,
}

impl FooterCache {
    pub fn new(max_capacity: u64, ttl_secs: u64) -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(Duration::from_secs(ttl_secs))
                .build(),
        }
    }

    pub fn get(&self, path: &str) -> Option<Arc<ParquetMetaData>> {
        self.cache.get(path)
    }

    pub fn put(&self, path: &str, meta: Arc<ParquetMetaData>) {
        self.cache.insert(path.to_string(), meta);
    }
}

impl Default for FooterCache {
    fn default() -> Self {
        Self::new(10000, 3600)
    }
}

impl std::fmt::Debug for FooterCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FooterCache").finish()
    }
}
