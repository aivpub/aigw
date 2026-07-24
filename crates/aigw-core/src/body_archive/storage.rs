//! Object storage backend builder for body archive.
//!
//! Builds an object_store::ObjectStore from S3 config for Parquet file uploads.

use std::sync::Arc;
use object_store::aws::AmazonS3Builder;
use object_store::ObjectStore;

use crate::body_archive::config::S3Config;

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
