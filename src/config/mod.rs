//! TOML config loader with auto-create from template.
//!
//! [`load_toml_config`] reads a TOML file and deserializes it into a typed
//! config struct. If the file doesn't exist and a template is provided, it
//! creates the file first.

pub mod loader;
pub mod template;

pub use loader::load_toml_config;
pub use template::default_config_template;
