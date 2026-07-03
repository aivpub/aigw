use std::path::Path;

fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

#[test]
fn test_dockerfile_exists() {
    let dockerfile = project_root().join("Dockerfile");
    assert!(
        dockerfile.exists(),
        "Dockerfile should exist at project root"
    );
}

#[test]
fn test_dockerfile_structure() {
    let dockerfile = project_root().join("Dockerfile");
    let content = std::fs::read_to_string(&dockerfile).unwrap();

    // Multi-stage build
    assert!(
        content.contains("FROM"),
        "Dockerfile should have FROM instructions"
    );
    assert!(
        content.contains("rust"),
        "Dockerfile should use Rust base image in builder stage"
    );
    assert!(
        content.matches("FROM").count() >= 2,
        "Dockerfile should be multi-stage (at least 2 FROM instructions)"
    );
    assert!(
        content.contains("builder"),
        "Dockerfile should have a builder stage"
    );

    // Runtime image
    assert!(
        content.contains("debian:bookworm-slim") || content.contains("debian:bookworm"),
        "Dockerfile should use debian bookworm for runtime"
    );

    // Binaries
    assert!(
        content.contains("aigw"),
        "Dockerfile should reference aigw binary"
    );
    assert!(
        content.contains("aigw-migrate"),
        "Dockerfile should include aigw-migrate binary"
    );

    // Port exposure
    assert!(content.contains("EXPOSE"), "Dockerfile should expose port");
    assert!(
        content.contains("4000"),
        "Dockerfile should expose port 4000"
    );

    // Health check
    assert!(
        content.contains("HEALTHCHECK"),
        "Dockerfile should have a health check"
    );
    assert!(
        content.contains("health"),
        "HEALTHCHECK should hit /health endpoint"
    );

    // Environment defaults
    assert!(
        content.contains("DATABASE_URL"),
        "Dockerfile should set DATABASE_URL"
    );
    assert!(
        content.contains("RUST_LOG"),
        "Dockerfile should set RUST_LOG"
    );

    // OCI labels
    assert!(
        content.contains("org.opencontainers.image.title"),
        "Dockerfile should have OCI labels"
    );

    // Build caching optimization
    assert!(
        content.contains("Cargo.toml") && content.contains("Cargo.lock"),
        "Dockerfile should copy manifests for build caching"
    );

    // Data directory
    assert!(
        content.contains("/app/data"),
        "Dockerfile should have app data directory"
    );

    // Entrypoint
    assert!(
        content.contains("ENTRYPOINT"),
        "Dockerfile should have ENTRYPOINT"
    );

    // Cargo build in release mode
    assert!(
        content.contains("--release"),
        "Dockerfile should build in release mode"
    );
}

#[test]
fn test_dockerfile_has_migrations() {
    let dockerfile = project_root().join("Dockerfile");
    let content = std::fs::read_to_string(&dockerfile).unwrap();

    // Migrations are included for aigw-migrate CLI tool
    assert!(
        content.contains("migrations"),
        "Dockerfile should copy migrations directory"
    );
}

#[test]
fn test_dockerfile_labels_valid() {
    let dockerfile = project_root().join("Dockerfile");
    let content = std::fs::read_to_string(&dockerfile).unwrap();

    assert!(
        content.contains("org.opencontainers.image.title=\"aigw\""),
        "Dockerfile should have correct image title label"
    );
    assert!(
        content.contains("org.opencontainers.image.description="),
        "Dockerfile should have image description label"
    );
    assert!(
        content.contains("https://github.com/aivpub/aigw"),
        "Dockerfile should reference correct source URL"
    );
}
