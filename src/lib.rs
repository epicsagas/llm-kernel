pub mod config;
pub mod error;
pub mod llm;
pub mod store;

pub fn name() -> &'static str {
    "ec-kernel"
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
