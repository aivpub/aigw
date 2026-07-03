use std::path::Path;

fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

#[test]
fn test_docker_compose_exists() {
    let f = project_root().join("docker-compose.yml");
    assert!(
        f.exists(),
        "docker-compose.yml should exist at project root"
    );
}

#[test]
fn test_config_example_exists() {
    let f = project_root().join("config.example.yaml");
    assert!(f.exists(), "config.example.yaml should exist");
}

#[test]
fn test_env_example_exists() {
    let f = project_root().join(".env.example");
    assert!(f.exists(), ".env.example should exist");
}

#[test]
fn test_config_is_valid_yaml() {
    let content = std::fs::read_to_string(project_root().join("config.example.yaml")).unwrap();
    let _: serde_yaml::Value =
        serde_yaml::from_str(&content).expect("config.example.yaml should be valid YAML");
}

#[test]
fn test_docker_compose_has_aigw_service() {
    let content = std::fs::read_to_string(project_root().join("docker-compose.yml")).unwrap();
    assert!(
        content.contains("aigw:"),
        "docker-compose should have aigw service"
    );
    assert!(
        content.contains("4000"),
        "docker-compose should expose port 4000"
    );
}

#[test]
fn test_env_example_has_required_vars() {
    let content = std::fs::read_to_string(project_root().join(".env.example")).unwrap();
    assert!(content.contains("MASTER_KEY"), "should have MASTER_KEY");
    assert!(content.contains("DATABASE_URL"), "should have DATABASE_URL");
    assert!(
        content.contains("OPENAI_API_KEY"),
        "should have OPENAI_API_KEY"
    );
}
