//! Node and edge CRUD operations.

use rusqlite::{Connection, params};

use crate::error::{KernelError, Result};

use super::types::{EdgeDirection, GraphEdge, GraphNode, NODE_COLUMNS, join_csv, row_to_node};

// ── Node CRUD ─────────────────────────────────────────

/// Insert or replace a node.
pub fn upsert_node(conn: &Connection, node: &GraphNode) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO nodes
         (id, type, title, tags, projects, agents, created, updated, body, importance, access_count, accessed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            node.id,
            node.node_type,
            node.title,
            join_csv(&node.tags),
            join_csv(&node.projects),
            join_csv(&node.agents),
            node.created,
            node.updated,
            node.body,
            node.importance,
            node.access_count,
            node.accessed_at,
        ],
    )
    .map_err(|e| KernelError::Store(e.to_string()))?;
    Ok(())
}

/// Read a single node by ID. Returns `None` if not found.
pub fn read_node(conn: &Connection, id: &str) -> Result<Option<GraphNode>> {
    let sql = format!("SELECT {NODE_COLUMNS} FROM nodes WHERE id = ?1");
    match conn.query_row(&sql, params![id], row_to_node) {
        Ok(node) => Ok(Some(node)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(KernelError::Store(e.to_string())),
    }
}

/// Batch-read multiple nodes by ID.
pub fn read_nodes(conn: &Connection, ids: &[&str]) -> Result<Vec<GraphNode>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("SELECT {NODE_COLUMNS} FROM nodes WHERE id IN ({ph})");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let nodes: Vec<GraphNode> = stmt
        .query_map(rusqlite::params_from_iter(ids.iter()), row_to_node)
        .map_err(|e| KernelError::Store(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(nodes)
}

/// Delete a node by ID. Returns whether a row was deleted.
pub fn delete_node(conn: &Connection, id: &str) -> Result<bool> {
    let changed = conn
        .execute("DELETE FROM nodes WHERE id = ?1", params![id])
        .map_err(|e| KernelError::Store(e.to_string()))?;
    Ok(changed > 0)
}

/// List all node IDs.
pub fn list_node_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT id FROM nodes")
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| KernelError::Store(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(ids)
}

/// Read nodes with optional limit, ordered by updated DESC.
pub fn read_nodes_limited(conn: &Connection, limit: usize) -> Result<Vec<GraphNode>> {
    let sql = format!("SELECT {NODE_COLUMNS} FROM nodes ORDER BY updated DESC LIMIT ?");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let nodes: Vec<GraphNode> = stmt
        .query_map(params![limit as i64], row_to_node)
        .map_err(|e| KernelError::Store(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(nodes)
}

// ── Edge CRUD ─────────────────────────────────────────

/// Append an edge (INSERT OR IGNORE — duplicates by ID are skipped).
pub fn append_edge(conn: &Connection, edge: &GraphEdge) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO edges (id, source, target, relation, weight, ts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            edge.id,
            edge.source,
            edge.target,
            edge.relation,
            edge.weight,
            edge.ts
        ],
    )
    .map_err(|e| KernelError::Store(e.to_string()))?;
    Ok(())
}

/// Append many edges in a single transaction (INSERT OR IGNORE — duplicates
/// by ID *or* by the `(source, target, relation)` unique index are skipped).
///
/// Equivalent to calling [`append_edge`] per edge, but commits once and reuses
/// one prepared statement, so it scales to hundreds of thousands of edges
/// (e.g. building a citation graph during indexing).
pub fn append_edges(conn: &Connection, edges: &[GraphEdge]) -> Result<()> {
    if edges.is_empty() {
        return Ok(());
    }
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| KernelError::Store(e.to_string()))?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT OR IGNORE INTO edges (id, source, target, relation, weight, ts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(|e| KernelError::Store(e.to_string()))?;
        for edge in edges {
            stmt.execute(params![
                edge.id,
                edge.source,
                edge.target,
                edge.relation,
                edge.weight,
                edge.ts
            ])
            .map_err(|e| KernelError::Store(e.to_string()))?;
        }
    }
    tx.commit().map_err(|e| KernelError::Store(e.to_string()))?;
    Ok(())
}

/// Read edges touching `node_id`, filtered by direction and optional relation.
///
/// - [`EdgeDirection::Both`] matches `source = node_id OR target = node_id`
///   (the historical behavior).
/// - [`EdgeDirection::Out`] matches `source = node_id` only (out-edges).
/// - [`EdgeDirection::In`] matches `target = node_id` only (in-edges).
///
/// When `relation` is `Some(r)`, additionally filters `relation = r`, using the
/// `idx_edges_src_rel` / `idx_edges_tgt_rel` v3 indexes for the directional cases.
pub(crate) fn edges_for_node_dir(
    conn: &Connection,
    node_id: &str,
    dir: EdgeDirection,
    relation: Option<&str>,
) -> Result<Vec<GraphEdge>> {
    let mut sql = String::from("SELECT id, source, target, relation, weight, ts FROM edges WHERE ");
    match dir {
        EdgeDirection::Out => sql.push_str("source = ?1"),
        EdgeDirection::In => sql.push_str("target = ?1"),
        EdgeDirection::Both => sql.push_str("(source = ?1 OR target = ?1)"),
    }
    if relation.is_some() {
        sql.push_str(" AND relation = ?2");
    }
    sql.push_str(" ORDER BY weight DESC");
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let mapper = |row: &rusqlite::Row<'_>| {
        Ok(GraphEdge {
            id: row.get(0)?,
            source: row.get(1)?,
            target: row.get(2)?,
            relation: row.get(3)?,
            weight: row.get(4)?,
            ts: row.get(5)?,
        })
    };
    let edges: Vec<GraphEdge> = if let Some(r) = relation {
        stmt.query_map(params![node_id, r], mapper)
    } else {
        stmt.query_map(params![node_id], mapper)
    }
    .map_err(|e| KernelError::Store(e.to_string()))?
    .filter_map(|r| r.ok())
    .collect();
    Ok(edges)
}

/// Read edges, capped at `limit`.
pub fn read_edges(conn: &Connection, limit: usize) -> Result<Vec<GraphEdge>> {
    let mut stmt = conn
        .prepare("SELECT id, source, target, relation, weight, ts FROM edges LIMIT ?1")
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let edges: Vec<GraphEdge> = stmt
        .query_map(params![limit as i64], |row| {
            Ok(GraphEdge {
                id: row.get(0)?,
                source: row.get(1)?,
                target: row.get(2)?,
                relation: row.get(3)?,
                weight: row.get(4)?,
                ts: row.get(5)?,
            })
        })
        .map_err(|e| KernelError::Store(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(edges)
}

/// Read edges whose source AND target are both in `ids` — the induced subgraph
/// over a candidate node set.
///
/// Used to build the candidate subgraph for PageRank boosting in
/// [`smart_recall`](super::recall::smart_recall). `ids.len()` must stay under
/// SQLite's bind-variable limit (999 by default); `smart_recall` caps at 100.
pub(crate) fn edges_among(conn: &Connection, ids: &[&str]) -> Result<Vec<GraphEdge>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let ph = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, source, target, relation, weight, ts FROM edges \
         WHERE source IN ({ph}) AND target IN ({ph})"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let edges: Vec<GraphEdge> = stmt
        .query_map(
            rusqlite::params_from_iter(ids.iter().chain(ids.iter()).copied()),
            |row| {
                Ok(GraphEdge {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    target: row.get(2)?,
                    relation: row.get(3)?,
                    weight: row.get(4)?,
                    ts: row.get(5)?,
                })
            },
        )
        .map_err(|e| KernelError::Store(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(edges)
}

/// Delete an edge by ID.
pub fn delete_edge(conn: &Connection, id: &str) -> Result<bool> {
    let changed = conn
        .execute("DELETE FROM edges WHERE id = ?1", params![id])
        .map_err(|e| KernelError::Store(e.to_string()))?;
    Ok(changed > 0)
}

/// Delete all edges connected to a node (source or target).
pub(crate) fn remove_edges_for_node(conn: &Connection, node_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM edges WHERE source = ?1 OR target = ?1",
        params![node_id],
    )
    .map_err(|e| KernelError::Store(e.to_string()))?;
    Ok(())
}

/// Read edges where the given node is source or target.
pub(crate) fn edges_for_node(conn: &Connection, node_id: &str) -> Result<Vec<GraphEdge>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, source, target, relation, weight, ts FROM edges WHERE source = ?1 OR target = ?1",
        )
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let edges: Vec<GraphEdge> = stmt
        .query_map(params![node_id], |row| {
            Ok(GraphEdge {
                id: row.get(0)?,
                source: row.get(1)?,
                target: row.get(2)?,
                relation: row.get(3)?,
                weight: row.get(4)?,
                ts: row.get(5)?,
            })
        })
        .map_err(|e| KernelError::Store(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::schema::init_graph_schema;
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        conn
    }

    fn test_node(id: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            node_type: "concept".to_string(),
            title: format!("Node {id}"),
            body: "test body".to_string(),
            tags: vec!["test".to_string()],
            projects: vec![],
            agents: vec![],
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
            importance: 0.7,
            access_count: 0,
            accessed_at: String::new(),
        }
    }

    #[test]
    fn upsert_and_read_node() {
        let conn = mem_db();
        let node = test_node("n1");
        upsert_node(&conn, &node).unwrap();
        let loaded = read_node(&conn, "n1").unwrap().unwrap();
        assert_eq!(loaded.id, "n1");
        assert_eq!(loaded.title, "Node n1");
        assert_eq!(loaded.tags, vec!["test"]);
    }

    #[test]
    fn read_missing_node_returns_none() {
        let conn = mem_db();
        assert!(read_node(&conn, "nope").unwrap().is_none());
    }

    #[test]
    fn delete_node_returns_true_when_exists() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1")).unwrap();
        assert!(delete_node(&conn, "n1").unwrap());
        assert!(!delete_node(&conn, "n1").unwrap());
    }

    #[test]
    fn list_node_ids_returns_all() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("a")).unwrap();
        upsert_node(&conn, &test_node("b")).unwrap();
        let ids = list_node_ids(&conn).unwrap();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn append_and_read_edges() {
        let conn = mem_db();
        let edge = GraphEdge {
            id: "e1".to_string(),
            source: "a".to_string(),
            target: "b".to_string(),
            relation: "related".to_string(),
            weight: 1.0,
            ts: "2026-01-01T00:00:00Z".to_string(),
        };
        append_edge(&conn, &edge).unwrap();
        let edges = read_edges(&conn, 10).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, "a");
    }

    #[test]
    fn edges_for_node_returns_both_directions() {
        let conn = mem_db();
        append_edge(
            &conn,
            &GraphEdge {
                id: "e1".into(),
                source: "a".into(),
                target: "b".into(),
                relation: "related".into(),
                weight: 1.0,
                ts: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        append_edge(
            &conn,
            &GraphEdge {
                id: "e2".into(),
                source: "c".into(),
                target: "a".into(),
                relation: "related".into(),
                weight: 1.0,
                ts: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        let edges = edges_for_node(&conn, "a").unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn test_remove_edges_for_node() {
        let conn = mem_db();
        append_edge(
            &conn,
            &GraphEdge {
                id: "e1".into(),
                source: "a".into(),
                target: "b".into(),
                relation: "related".into(),
                weight: 1.0,
                ts: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        remove_edges_for_node(&conn, "a").unwrap();
        assert!(read_edges(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn append_edges_inserts_batch() {
        let conn = mem_db();
        let edges = vec![
            GraphEdge {
                id: "e1".into(),
                source: "a".into(),
                target: "b".into(),
                relation: "cites".into(),
                weight: 1.0,
                ts: "2026-01-01T00:00:00Z".into(),
            },
            GraphEdge {
                id: "e2".into(),
                source: "a".into(),
                target: "c".into(),
                relation: "cites".into(),
                weight: 0.5,
                ts: "2026-01-01T00:00:00Z".into(),
            },
        ];
        append_edges(&conn, &edges).unwrap();
        assert_eq!(read_edges(&conn, 10).unwrap().len(), 2);
    }

    #[test]
    fn append_edges_empty_is_noop() {
        let conn = mem_db();
        append_edges(&conn, &[]).unwrap();
        assert!(read_edges(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn append_edges_ignores_duplicate_id_and_unique() {
        let conn = mem_db();
        let e = GraphEdge {
            id: "e1".into(),
            source: "a".into(),
            target: "b".into(),
            relation: "cites".into(),
            weight: 1.0,
            ts: "2026-01-01T00:00:00Z".into(),
        };
        append_edges(&conn, &[e.clone()]).unwrap();
        // Same id (INSERT OR IGNORE) and same (source, target, relation) with a
        // different id (unique index) are both skipped.
        append_edges(
            &conn,
            &[
                e,
                GraphEdge {
                    id: "e2".into(),
                    source: "a".into(),
                    target: "b".into(),
                    relation: "cites".into(),
                    weight: 1.0,
                    ts: "2026-01-01T00:00:00Z".into(),
                },
            ],
        )
        .unwrap();
        assert_eq!(read_edges(&conn, 10).unwrap().len(), 1);
    }

    #[test]
    fn edges_for_node_dir_filters_direction_and_relation() {
        let conn = mem_db();
        append_edges(
            &conn,
            &[
                GraphEdge {
                    id: "e1".into(),
                    source: "a".into(),
                    target: "b".into(),
                    relation: "cites".into(),
                    weight: 1.0,
                    ts: "t".into(),
                },
                GraphEdge {
                    id: "e2".into(),
                    source: "c".into(),
                    target: "a".into(),
                    relation: "cites".into(),
                    weight: 1.0,
                    ts: "t".into(),
                },
                GraphEdge {
                    id: "e3".into(),
                    source: "a".into(),
                    target: "d".into(),
                    relation: "see_also".into(),
                    weight: 1.0,
                    ts: "t".into(),
                },
            ],
        )
        .unwrap();
        // Out-edges of `a`: b (cites) and d (see_also).
        assert_eq!(
            edges_for_node_dir(&conn, "a", EdgeDirection::Out, None)
                .unwrap()
                .len(),
            2
        );
        // Out-edges of `a` restricted to `cites`: only b.
        let out_cites = edges_for_node_dir(&conn, "a", EdgeDirection::Out, Some("cites")).unwrap();
        assert_eq!(out_cites.len(), 1);
        assert_eq!(out_cites[0].target, "b");
        // In-edges of `a`: c→a.
        let inc = edges_for_node_dir(&conn, "a", EdgeDirection::In, None).unwrap();
        assert_eq!(inc.len(), 1);
        assert_eq!(inc[0].source, "c");
        // Both directions: b, d, c → 3.
        assert_eq!(
            edges_for_node_dir(&conn, "a", EdgeDirection::Both, None)
                .unwrap()
                .len(),
            3
        );
    }
}
