//! Re-exports of the most commonly used types.
//!
//! ```no_run
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
    json_extract::{JsonExtractor, extract_json, parse_json},
    prompt::render_prompt,
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

// --- MCP ---

#[cfg(feature = "mcp")]
pub use crate::mcp::{
    BearerAuth, Handler, JsonRpcDispatcher, McpServer, ResourceDescription, ToolDescription,
};

// --- Tokens ---

#[cfg(feature = "tokens")]
pub use crate::tokens::estimate_tokens;

// --- Search ---

#[cfg(feature = "search")]
pub use crate::search::{SearchResult, rrf_fuse};

// --- Embedding ---

#[cfg(feature = "embedding")]
pub use crate::embedding::{EmbeddingProvider, EmbeddingResult, cosine_similarity};

// --- Telemetry ---

#[cfg(feature = "telemetry")]
pub use crate::telemetry::{
    ConsoleSink, FailureClass, FeatureName, NoopSink, ProviderCategory, TelemetryEvent,
    TelemetrySink, ToolName,
};

// --- Safety ---

#[cfg(feature = "safety")]
pub use crate::safety::{
    FailureCategory, classify_failure, mask_secrets, sanitize_output, strip_ansi,
};

// --- Install ---

#[cfg(feature = "install")]
pub use crate::install::{AgentKind, McpConfig, generate_mcp_config};
