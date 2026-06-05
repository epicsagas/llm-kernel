//! Re-exports of the most commonly used types.
//!
//! ```ignore
//! use llm_kernel::prelude::*;
//! ```

pub use crate::error::{KernelError, Result};

// --- Provider ---

#[cfg(feature = "provider")]
pub use crate::provider::{
    AuthStrategy, ModelCapabilities, ModelCost, ModelDescriptor, ModelLimit, ModelModalities,
    ProviderIndex, ServiceDescriptor,
};

// --- Client-async ---

#[cfg(feature = "client-async")]
pub use crate::llm::{
    AnthropicClient, ChatMessage, LLMClient, LLMRequest, LLMResponse, LLMStream, ModelConfig,
    OpenAIClient, StreamEvent, TokenUsage,
};

// --- Secrets ---

#[cfg(feature = "secrets")]
pub use crate::secrets::{SecretVault, redact_credential};

// --- Graph ---

#[cfg(feature = "graph")]
pub use crate::graph::{
    Graph, GraphEdge, GraphNode, GraphNodeSummary, GraphStats, ScoredNode, append_edge,
    build_graph, compute_stats, decay_importance, delete_node, graph_neighbors, init_graph_schema,
    query_nodes, read_node, related_nodes, search_nodes, smart_recall, tag_stale_nodes,
    upsert_node,
};
