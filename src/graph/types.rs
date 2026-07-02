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
///
/// Derives `Default` so callers can future-proof field additions with struct
/// update syntax: `GraphNode { id, node_type, ..Default::default() }`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphNode {
    /// Unique node identifier (UUID).
    pub id: String,
    /// Node type (e.g. "decision", "concept", "pattern", "error", "session").
    #[serde(rename = "type")]
    pub node_type: String,
    /// Short title summarizing the node's content.
    pub title: String,
    /// Full text body of the node.
    #[serde(default)]
    pub body: String,
    /// Classification tags for filtering and search.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Projects this node belongs to.
    #[serde(default)]
    pub projects: Vec<String>,
    /// Agents that contributed to or own this node.
    #[serde(default)]
    pub agents: Vec<String>,
    /// ISO 8601 creation timestamp.
    pub created: String,
    /// ISO 8601 last-updated timestamp.
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphEdge {
    /// Unique edge identifier (UUID).
    pub id: String,
    /// Source node ID.
    pub source: String,
    /// Target node ID.
    pub target: String,
    /// Relationship type (e.g. "related", "solves", "derived_from").
    pub relation: String,
    /// Edge weight (0.0–1.0); higher values indicate stronger relationships.
    pub weight: f64,
    /// ISO 8601 creation timestamp.
    pub ts: String,
}

/// Summary of a node for serialization (web viewer, API responses).
/// Omits body and metadata fields for compact payloads.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphNodeSummary {
    /// Unique node identifier.
    pub id: String,
    /// Short title.
    pub title: String,
    /// Node type.
    #[serde(rename = "type")]
    pub node_type: String,
    /// Classification tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Importance score (0.0–1.0).
    #[serde(default = "default_importance")]
    pub importance: f64,
}

/// A graph snapshot containing nodes (summaries) and edges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Graph {
    /// All nodes in the snapshot.
    pub nodes: Vec<GraphNodeSummary>,
    /// All edges in the snapshot.
    pub edges: Vec<GraphEdge>,
}

/// A node scored by relevance for recall ranking.
#[derive(Debug, Clone)]
pub struct ScoredNode {
    /// The graph node.
    pub node: GraphNode,
    /// Composite relevance score.
    pub score: f64,
}

/// Aggregate statistics about the knowledge graph.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphStats {
    /// Total number of nodes.
    pub total_nodes: i64,
    /// Total number of edges.
    pub total_edges: i64,
    /// Mean importance score across all nodes.
    pub avg_importance: f64,
    /// Node count broken down by node type.
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

/// Escape SQL LIKE wildcards (`%`, `_`, `\`) in a bound value.
///
/// Used with `LIKE '%,' || ? ESCAPE '\' || ',%'` patterns to prevent
/// project names or tags containing `%` or `_` from matching unintended rows.
pub(crate) fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Validate a UUID v4 string (strict format: `xxxxxxxx-xxxx-4xxx-[89ab]xxx-xxxxxxxxxxxx`).
///
/// Pure byte inspection — zero dependencies. Useful for validating node and edge IDs
/// before writing to the graph.
pub fn validate_uuid(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 36
        && b[8] == b'-'
        && b[13] == b'-'
        && b[18] == b'-'
        && b[23] == b'-'
        && b[14] == b'4'
        && matches!(b[19], b'8'..=b'9' | b'a'..=b'b' | b'A'..=b'B')
        && b.iter()
            .enumerate()
            .all(|(i, &c)| matches!(i, 8 | 13 | 18 | 23) || c.is_ascii_hexdigit())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_like_escapes_percent() {
        assert_eq!(escape_like("100%"), r"100\%");
    }

    #[test]
    fn escape_like_escapes_underscore() {
        assert_eq!(escape_like("a_b"), r"a\_b");
    }

    #[test]
    fn escape_like_passthrough_normal() {
        assert_eq!(escape_like("hello"), "hello");
    }

    #[test]
    fn escape_like_escapes_backslash() {
        assert_eq!(escape_like(r"\"), r"\\");
    }

    #[test]
    fn escape_like_empty() {
        assert_eq!(escape_like(""), "");
    }

    #[test]
    fn validate_uuid_accepts_valid_v4() {
        assert!(validate_uuid("550e8400-e29b-41d4-a716-446655440000"));
        assert!(validate_uuid("00000000-0000-4000-8000-000000000000"));
        assert!(validate_uuid("ffffffff-ffff-4fff-bfff-ffffffffffff"));
    }

    #[test]
    fn validate_uuid_rejects_wrong_version() {
        // version byte (position 14) is '3', not '4'
        assert!(!validate_uuid("550e8400-e29b-31d4-a716-446655440000"));
    }

    #[test]
    fn validate_uuid_rejects_wrong_variant() {
        // variant byte (position 19) is 'c', not in [89abAB]
        assert!(!validate_uuid("550e8400-e29b-41d4-c716-446655440000"));
    }

    #[test]
    fn validate_uuid_rejects_short() {
        assert!(!validate_uuid(""));
        assert!(!validate_uuid("550e8400"));
    }

    #[test]
    fn validate_uuid_rejects_missing_dashes() {
        assert!(!validate_uuid("550e8400e29b41d4a716446655440000"));
    }

    #[test]
    fn validate_uuid_rejects_non_hex() {
        assert!(!validate_uuid("550g8400-e29b-41d4-a716-446655440000"));
    }
}
