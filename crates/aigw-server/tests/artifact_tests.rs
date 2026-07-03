#[test]
fn test_deployment_doc_exists() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("docs")
        .join("deployment.md");
    assert!(path.exists(), "docs/deployment.md should exist");
}

#[test]
fn test_dockerfile_exists() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Dockerfile");
    assert!(path.exists(), "Dockerfile should exist");
}

#[test]
fn test_docker_compose_exists() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("docker-compose.yml");
    assert!(path.exists(), "docker-compose.yml should exist");
}

#[test]
fn test_config_example_exists() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config.example.yaml");
    assert!(path.exists(), "config.example.yaml should exist");
}
