pub mod error;

#[cfg(feature = "provider")]
pub mod provider;

#[cfg(feature = "discovery")]
pub mod discovery;

#[cfg(feature = "secrets")]
pub mod secrets;

#[cfg(feature = "client-async")]
pub mod llm;

#[cfg(feature = "store")]
pub mod store;

#[cfg(feature = "config")]
pub mod config;

pub fn name() -> &'static str {
    "ec-kernel"
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
