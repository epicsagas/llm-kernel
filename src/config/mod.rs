//! TOML config loader with auto-create from template.
//!
//! [`load_toml_config`] reads a TOML file and deserializes it into a typed
//! config struct. If the file doesn't exist and a template is provided, it
//! creates the file first.

/// TOML config loader with auto-create from template.
pub mod loader;
/// Default config template generator.
pub mod template;

pub use loader::{FieldError, load_toml_config, validate_config};
pub use template::default_config_template;
