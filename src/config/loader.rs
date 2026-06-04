use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::{KernelError, Result};

/// Load a TOML config file and deserialize it into T.
///
/// If the file doesn't exist and `template` is provided, writes the template
/// to the path first, then loads it.
pub fn load_toml_config<T: DeserializeOwned>(path: &Path, template: Option<&str>) -> Result<T> {
    if !path.exists()
        && let Some(tmpl) = template
    {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, tmpl)?;
    }

    let content = std::fs::read_to_string(path)?;
    toml::from_str(&content).map_err(|e| KernelError::Config(format!("{}: {}", path.display(), e)))
}

/// Load a TOML config from a string (useful for testing).
pub fn parse_toml_config<T: DeserializeOwned>(content: &str) -> Result<T> {
    toml::from_str(content).map_err(|e| KernelError::Config(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use tempfile::TempDir;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestConfig {
        name: String,
        #[serde(default = "default_port")]
        port: u16,
    }

    fn default_port() -> u16 {
        8080
    }

    #[test]
    fn test_load_existing_config() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.toml");
        std::fs::write(&path, "name = \"test\"\nport = 3000\n").unwrap();

        let config: TestConfig = load_toml_config(&path, None).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.port, 3000);
    }

    #[test]
    fn test_load_creates_from_template() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("new.toml");
        let template = "name = \"default\"\nport = 9090\n";

        let config: TestConfig = load_toml_config(&path, Some(template)).unwrap();
        assert_eq!(config.name, "default");
        assert_eq!(config.port, 9090);
        assert!(path.exists());
    }

    #[test]
    fn test_parse_toml_config() {
        let config: TestConfig = parse_toml_config("name = \"hello\"").unwrap();
        assert_eq!(config.name, "hello");
        assert_eq!(config.port, 8080); // default
    }
}
