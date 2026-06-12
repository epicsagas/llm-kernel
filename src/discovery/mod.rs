//! Dynamic model discovery from remote sources.
//!
//! Fetches model listings from [models.dev](https://github.com/anomalyco/models.dev),
//! Ollama instances, and OpenAI-compatible endpoints. Results are cached locally.

/// Client for the models.dev model catalog API.
pub mod models_dev;
/// Client for discovering models from a running Ollama instance.
pub mod ollama;
/// Client for discovering models from any OpenAI-compatible endpoint.
pub mod openai_compat;

pub use models_dev::{ModelEntry, ModelLimits, ModelsDevPayload, fetch_and_cache, load_cache};
pub use ollama::fetch_ollama_models;
pub use openai_compat::fetch_openai_compatible_models;
