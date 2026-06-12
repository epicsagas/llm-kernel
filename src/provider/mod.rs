//! LLM provider catalog and capability profiles.
//!
//! The embedded catalog (`catalog.json`) contains 16 providers with model
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

pub use capability::{AuthStrategy, CapabilityProfile};
pub use catalog::{
    ModelCapabilities, ModelCost, ModelDescriptor, ModelLimit, ModelModalities, ProviderIndex,
    ServiceDescriptor,
};
