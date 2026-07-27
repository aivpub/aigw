//! Object storage backend builder for body archive.
//!
//! Builds an `object_store::ObjectStore` from either an S3 config or a
//! `StorageBackend` enum (S3 / FileSystem). For FileSystem we use
//! `object_store::local::LocalFileSystem` so CI can run the full archive
//! round-trip without an S3 account. Also provides `${ENV_VAR}` placeholder
//! resolution for S3 credentials.

use std::sync::Arc;

use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;
use object_store::ObjectStore;

use crate::body_archive::config::{S3Config, StorageBackend};

/// Build an object_store ObjectStore from S3 configuration.
/// Returns an Arc<dyn ObjectStore> for use by the Parquet writer.
pub fn build_object_store(config: &S3Config) -> Result<Arc<dyn ObjectStore>, String> {
    let base_builder = AmazonS3Builder::new()
        .with_bucket_name(&config.bucket)
        .with_region(&config.region)
        .with_access_key_id(&config.access_key_id)
        .with_secret_access_key(&config.secret_access_key);

    let builder = if config.use_ssl {
        base_builder.with_allow_http(false)
    } else {
        base_builder.with_allow_http(true)
    };

    let builder = if !config.endpoint.is_empty() {
        builder.with_endpoint(&config.endpoint)
    } else {
        builder
    };

    let builder = if config.url_style == "path" {
        builder.with_virtual_hosted_style_request(false)
    } else {
        builder
    };

    let builder = if let Some(ref token) = config.session_token {
        builder.with_token(token)
    } else {
        builder
    };

    let store = builder.build().map_err(|e| format!("S3 builder error: {}", e))?;
    Ok(Arc::new(store))
}

/// Resolve `${ENV_VAR}` placeholders in a string against `std::env`.
///
/// - `${VAR}` → value of env `VAR` (empty string if unset)
/// - literal text without `${}` is returned verbatim
///
/// Lets operators keep S3 credentials out of config files by referencing
/// environment variables instead.
pub fn resolve_env_in_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            // find closing '}'
            if let Some(end) = bytes[i + 2..].iter().position(|&b| b == b'}') {
                let name = &s[i + 2..i + 2 + end];
                let val = std::env::var(name).unwrap_or_default();
                out.push_str(&val);
                i = i + 2 + end + 1;
                continue;
            }
        }
        // copy one byte as UTF-8 boundary (safe: '$' and '{' are ASCII)
        let ch = s[i..].chars().next().expect("non-empty");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Return a copy of `backend` with `${ENV_VAR}` placeholders resolved in all
/// S3 string fields. FileSystem is returned unchanged (no env fields).
pub fn resolve_env_placeholders(backend: &StorageBackend) -> StorageBackend {
    match backend {
        StorageBackend::S3 {
            bucket,
            region,
            endpoint,
            access_key_id,
            secret_access_key,
            prefix,
            use_ssl,
            compatibility_mode,
            url_style,
        } => StorageBackend::S3 {
            bucket: resolve_env_in_string(bucket),
            region: resolve_env_in_string(region),
            endpoint: endpoint.as_ref().map(|e| resolve_env_in_string(e)),
            access_key_id: resolve_env_in_string(access_key_id),
            secret_access_key: resolve_env_in_string(secret_access_key),
            prefix: resolve_env_in_string(prefix),
            use_ssl: *use_ssl,
            compatibility_mode: *compatibility_mode,
            url_style: url_style.clone(),
        },
        other => other.clone(),
    }
}

/// Build an object_store ObjectStore from a `StorageBackend` enum.
///
/// - `StorageBackend::S3 { .. }` → AmazonS3 (with env placeholders resolved)
/// - `StorageBackend::FileSystem { path }` → LocalFileSystem rooted at `path`
///
/// `FileSystem` is the CI-friendly backend: no S3 account needed, and the
/// archive round-trip can be exercised end-to-end in tests.
pub fn build_object_store_for_backend(
    backend: &StorageBackend,
) -> Result<Arc<dyn ObjectStore>, String> {
    match backend {
        StorageBackend::S3 { .. } => {
            let resolved = resolve_env_placeholders(backend);
            let s3_config = match resolved {
                StorageBackend::S3 {
                    bucket,
                    region,
                    endpoint,
                    access_key_id,
                    secret_access_key,
                    prefix,
                    use_ssl,
                    compatibility_mode,
                    url_style,
                } => S3Config {
                    bucket,
                    region,
                    endpoint: endpoint.unwrap_or_default(),
                    access_key_id,
                    secret_access_key,
                    prefix,
                    use_ssl,
                    compatibility_mode,
                    url_style,
                    ..Default::default()
                },
                _ => unreachable!("matched S3 above"),
            };
            build_object_store(&s3_config)
        }
        StorageBackend::FileSystem { path } => {
            let store = LocalFileSystem::new_with_prefix(path)
                .map_err(|e| format!("LocalFileSystem init: {}", e))?;
            Ok(Arc::new(store))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_object_store_minimal() {
        let config = S3Config {
            bucket: "test-bucket".into(),
            region: "us-east-1".into(),
            access_key_id: "test-key".into(),
            secret_access_key: "test-secret".into(),
            endpoint: "http://localhost:9000".into(),
            use_ssl: false,
            url_style: "path".into(),
            ..Default::default()
        };
        let store = build_object_store(&config);
        // Store builds lazily, so it won't actually connect until used
        assert!(store.is_ok());
    }
}
