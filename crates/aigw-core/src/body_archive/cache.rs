//! Footer cache — moka-based LRU cache for parsed Parquet metadata + bloom
//! filters.
//!
//! Caches `ParquetMetaData` (row group offsets, column statistics, Bloom filter
//! locations) along with lazily-populated bloom filters, keyed by object-store
//! path. Eliminates footer + bloom-filter round-trips on repeated queries to the
//! same file.

use moka::sync::Cache;
use parquet::bloom_filter::Sbbf;
use parquet::file::metadata::ParquetMetaData;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Cached per-file metadata: footer + bloom filters.
///
/// Bloom filters are populated lazily on first cold read and reused for all
/// subsequent queries to the same file — zero IO for bloom probe after cache
/// warm.
#[derive(Clone)]
pub struct CachedMeta {
    pub metadata: Arc<ParquetMetaData>,
    /// Bloom filters keyed by (row_group_index, column_index).
    pub bloom_filters: HashMap<(usize, usize), Sbbf>,
}

/// Footer cache wrapping moka's sync LRU cache.
///
/// Memory footprint per entry (~2 KB for a typical footer + ~0.3–6 KB for
/// bloom filters, per column per row group). For a busy deployment (1000
/// requests/hour × 720 hour files × ~50 KB/file) the cache footprint is ≤
/// ~40 MB — acceptable alongside the existing 10K capacity (~20 MB for
/// footers alone).
pub struct FooterCache {
    cache: Cache<String, CachedMeta>,
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

    pub fn get(&self, path: &str) -> Option<CachedMeta> {
        self.cache.get(path)
    }

    pub fn put(&self, path: &str, meta: CachedMeta) {
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
