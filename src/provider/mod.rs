//! LLM provider catalog and capability profiles.
//!
//! The embedded catalog (`catalog.json`) contains 20 providers with model
//! metadata aligned to the [models.dev](https://github.com/anomalyco/models.dev)
//! schema: pricing, token limits, modalities, and capability flags.
//!
//! ```
//! use llm_kernel::provider::ProviderIndex;
//!
//! let catalog = ProviderIndex::embedded();
//! assert!(!catalog.ids().is_empty());
//! ```

/// Capability profiles and authentication strategies for providers.
pub mod capability;
/// Provider catalog, model descriptors, and pricing data.
pub mod catalog;
/// Provider-id mapping between the catalog and the models.dev upstream.
pub mod mapping;
/// Catalog sync tooling — merge models.dev into the embedded catalog.
#[cfg(feature = "catalog-sync")]
pub mod sync;

pub use capability::{AuthStrategy, CapabilityProfile};
pub use catalog::{
    ModelCapabilities, ModelCost, ModelDescriptor, ModelLimit, ModelModalities, ProviderIndex,
    ServiceDescriptor,
};
pub use mapping::{Mapping, resolve};
