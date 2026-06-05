//! Core types for the knowledge graph.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default importance for new nodes.
pub fn default_importance() -> f64 {
    0.5
}

/// Importance score by node type. Used when creating nodes without explicit importance.
pub fn importance_for_type(node_type: &str) -> f64 {
    match node_type {
        "decision" => 0.9,
        "resolution" => 0.8,
        "psychographic" => 0.8,
        "instinct" => 0.7,
        "concept" => 0.7,
        "project" => 0.7,
        "pattern" => 0.5,
        "error" => 0.4,
        "session" => 0.05,
        _ => 0.5,
    }
}

/// A node in the knowledge graph.
///
/// Represents a discrete piece of knowledge — a decision, concept, pattern, etc.
/// Stored as a single row in the `nodes` SQLite table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    /// Node type (e.g. "decision", "concept", "pattern", "error", "session").
    #[serde(rename = "type")]
    pub node_type: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub projects: Vec<String>,
    #[serde(default)]
    pub agents: Vec<String>,
    /// ISO 8601 timestamp.
    pub created: String,
    /// ISO 8601 timestamp.
    pub updated: String,
    /// Importance score (0.0–1.0). Higher = more valuable for recall.
    #[serde(default = "default_importance")]
    pub importance: f64,
    /// How many times this node has been retrieved via recall/search.
    #[serde(default)]
    pub access_count: i64,
    /// Last time this node was accessed (ISO 8601).
    #[serde(default)]
    pub accessed_at: String,
}

/// A directed, weighted edge between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    /// Relationship type (e.g. "related", "solves", "derived_from").
    pub relation: String,
    pub weight: f64,
    /// ISO 8601 timestamp.
    pub ts: String,
}

/// Summary of a node for serialization (web viewer, API responses).
/// Omits body and metadata fields for compact payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNodeSummary {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_importance")]
    pub importance: f64,
}

/// A graph snapshot containing nodes (summaries) and edges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Graph {
    pub nodes: Vec<GraphNodeSummary>,
    pub edges: Vec<GraphEdge>,
}

/// A node scored by relevance for recall ranking.
#[derive(Debug, Clone)]
pub struct ScoredNode {
    pub node: GraphNode,
    pub score: f64,
}

/// Aggregate statistics about the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub total_nodes: i64,
    pub total_edges: i64,
    pub avg_importance: f64,
    pub by_type: HashMap<String, i64>,
}

// ── CSV helpers (tags, projects, agents stored as comma-separated strings) ──

pub(crate) fn join_csv(v: &[String]) -> String {
    v.join(",")
}

pub(crate) fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

// ── Row mapping ───────────────────────────────────────

/// Standard SELECT columns for node queries.
pub(crate) const NODE_COLUMNS: &str = "id, type, title, tags, projects, agents, created, updated, body, importance, access_count, accessed_at";

/// Same columns but table-prefixed for JOIN queries.
pub(crate) const NODE_COLUMNS_PREFIXED: &str = "id, n.type, n.title, n.tags, n.projects, n.agents, n.created, n.updated, n.body, n.importance, n.access_count, n.accessed_at";

pub(crate) fn row_to_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<GraphNode> {
    let tags: String = row.get(3)?;
    let projects: String = row.get(4)?;
    let agents: String = row.get(5)?;
    Ok(GraphNode {
        id: row.get(0)?,
        node_type: row.get(1)?,
        title: row.get(2)?,
        tags: split_csv(&tags),
        projects: split_csv(&projects),
        agents: split_csv(&agents),
        created: row.get(6)?,
        updated: row.get(7)?,
        body: row.get(8)?,
        importance: row.get(9).unwrap_or(0.5),
        access_count: row.get::<_, i64>(10).unwrap_or(0),
        accessed_at: row.get(11).unwrap_or_default(),
    })
}
