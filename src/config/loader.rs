use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::{KernelError, Result};

/// A field-level validation error with structured context.
///
/// Unlike a raw serde error string, this type extracts the field path,
/// expected type, and the invalid value for programmatic handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// Dot-separated path to the field (e.g., `"server.port"`).
    pub path: String,
    /// What the deserializer expected (e.g., `"u16"`, `"string"`).
    pub expected: String,
    /// The invalid value as it appeared in the TOML source.
    pub value: String,
}

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "field '{}': expected {}, got {}",
            self.path, self.expected, self.value
        )
    }
}

impl std::error::Error for FieldError {}

/// Parse a serde error message into structured [`FieldError`]s.
///
/// Serde's error messages follow predictable patterns. This function extracts
/// the field path, expected type, and the invalid value from common formats:
///
/// - `"invalid type: found string \"x\", expected u16"` → `FieldError { path: ".", expected: "u16", value: "\"x\"" }`
/// - `"missing field `port`"` → `FieldError { path: "port", expected: "unknown", value: "missing" }`
fn parse_serde_errors(msg: &str) -> Vec<FieldError> {
    let mut errors = Vec::new();

    // TOML crate wraps serde errors with context lines (line numbers, source
    // pointers). The actual serde error is always on the last non-empty line.
    // Only try to match lines that look like serde patterns.
    for line in msg.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Skip TOML context lines: "TOML parse error...", "|", "1 | ...", etc.
        if line.starts_with("TOML parse error")
            || line.starts_with('|')
            || line.starts_with("+-")
            || line
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() && line.contains('|'))
        {
            continue;
        }

        // Pattern: "invalid type: found string \"x\", expected u16"
        if let Some(rest) = line.strip_prefix("invalid type: ") {
            if let Some((found_part, expected_part)) = rest.split_once(", expected ") {
                let value = found_part
                    .strip_prefix("found ")
                    .unwrap_or(found_part)
                    .trim();
                errors.push(FieldError {
                    path: ".".into(),
                    expected: expected_part.trim().to_string(),
                    value: value.to_string(),
                });
            }
            continue;
        }

        // Pattern: "invalid value: found string \"x\", expected ..."
        if let Some(rest) = line.strip_prefix("invalid value: ") {
            if let Some((found_part, expected_part)) = rest.split_once(", expected ") {
                let value = found_part
                    .strip_prefix("found ")
                    .unwrap_or(found_part)
                    .trim();
                errors.push(FieldError {
                    path: ".".into(),
                    expected: expected_part.trim().to_string(),
                    value: value.to_string(),
                });
            }
            continue;
        }

        // Pattern: "missing field `port`"
        if let Some(rest) = line.strip_prefix("missing field `") {
            if let Some(field) = rest.strip_suffix('`') {
                errors.push(FieldError {
                    path: field.to_string(),
                    expected: "unknown".into(),
                    value: "missing".into(),
                });
            }
            continue;
        }

        // Pattern: "unknown field `extra`, expected one of `name`, `port`"
        if let Some(rest) = line.strip_prefix("unknown field `") {
            if let Some((field, rest2)) = rest.split_once('`') {
                errors.push(FieldError {
                    path: field.to_string(),
                    expected: rest2.trim_start_matches(", expected ").to_string(),
                    value: "unknown field".into(),
                });
            }
            continue;
        }

        // Fallback: return the raw message
        errors.push(FieldError {
            path: ".".into(),
            expected: "unknown".into(),
            value: line.to_string(),
        });
    }

    if errors.is_empty() {
        errors.push(FieldError {
            path: ".".into(),
            expected: "unknown".into(),
            value: msg.to_string(),
        });
    }

    errors
}

/// Validate a TOML config string and return structured field-level errors.
///
/// On success, returns the parsed config. On failure, returns a vec of
/// [`FieldError`] instead of a raw string error, making it possible to
/// display targeted error messages per field.
pub fn validate_config<T: DeserializeOwned>(
    toml_str: &str,
) -> std::result::Result<T, Vec<FieldError>> {
    match toml::from_str::<T>(toml_str) {
        Ok(v) => Ok(v),
        Err(e) => Err(parse_serde_errors(&e.to_string())),
    }
}

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

    #[test]
    fn validate_config_success() {
        let result = validate_config::<TestConfig>("name = \"test\"\nport = 3000");
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.port, 3000);
    }

    #[test]
    fn validate_config_wrong_type() {
        let result = validate_config::<TestConfig>("name = \"test\"\nport = \"not_a_number\"");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].expected, "u16");
        assert!(errors[0].value.contains("not_a_number"));
    }

    #[test]
    fn validate_config_missing_field() {
        let result = validate_config::<TestConfig>("port = 3000");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, "name");
        assert_eq!(errors[0].value, "missing");
    }

    #[test]
    fn validate_config_unknown_field() {
        let result = validate_config::<TestConfig>("name = \"test\"\nextra = true");
        // serde by default ignores unknown fields with #[serde(deny_unknown_fields)]
        // but without that attribute, this succeeds
        assert!(result.is_ok());
    }
}
