//! Graph traversal: 1-hop neighbors and BFS via recursive CTEs.

use std::collections::HashSet;

use rusqlite::{Connection, params};

use super::store::{read_edges, read_nodes};
use super::types::{Graph, GraphEdge, GraphNodeSummary};

/// Maximum edges in a graph snapshot (prevents unbounded memory).
const MAX_GRAPH_EDGES: usize = 2000;

/// Maximum seed IDs per neighbor query (keeps SQLite bind variables under limit).
const MAX_SEED_IDS: usize = 100;

/// Get 1-hop neighbors from seed IDs. Returns `(neighbor_id, total_weight)` sorted by weight DESC.
///
/// Follows edges in both directions (source→target and target→source).
/// Seed nodes are excluded from results.
pub fn graph_neighbors(conn: &Connection, seed_ids: &[String]) -> Vec<(String, f64)> {
    if seed_ids.is_empty() {
        return vec![];
    }
    let seed_ids = if seed_ids.len() > MAX_SEED_IDS {
        &seed_ids[..MAX_SEED_IDS]
    } else {
        seed_ids
    };

    let ph: String = seed_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT target AS nb, SUM(weight) AS w FROM edges WHERE source IN ({ph}) GROUP BY target \
         UNION ALL \
         SELECT source AS nb, SUM(weight) AS w FROM edges WHERE target IN ({ph}) GROUP BY source"
    );

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let rows: Vec<(String, f64)> = stmt
        .query_map(
            rusqlite::params_from_iter(seed_ids.iter().chain(seed_ids.iter())),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
        )
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default();

    let seed_set: HashSet<&str> = seed_ids.iter().map(String::as_str).collect();
    let mut weights: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for (nid, w) in rows {
        if !seed_set.contains(nid.as_str()) {
            *weights.entry(nid).or_default() += w;
        }
    }

    let mut result: Vec<(String, f64)> = weights.into_iter().collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result
}

/// BFS traversal from `start_id` via SQL recursive CTE.
///
/// Returns all reachable node IDs (excluding start), capped at 500.
/// Follows edges in both directions.
pub fn related_nodes(conn: &Connection, start_id: &str, _depth: usize) -> Vec<String> {
    let sql = "
        WITH RECURSIVE bfs(node_id) AS (
            SELECT target FROM edges WHERE source = ?1
            UNION SELECT source FROM edges WHERE target = ?1
            UNION SELECT e.target FROM edges e JOIN bfs ON e.source = bfs.node_id WHERE e.target != ?1
            UNION SELECT e.source FROM edges e JOIN bfs ON e.target = bfs.node_id WHERE e.source != ?1
        )
        SELECT node_id FROM bfs
        LIMIT 500
    ";

    conn.prepare(sql)
        .and_then(|mut stmt| {
            stmt.query_map(params![start_id], |row| row.get::<_, String>(0))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default()
}

/// Build a graph snapshot (node summaries + edges) from the database.
pub fn build_graph(conn: &Connection) -> crate::error::Result<Graph> {
    let ids = super::store::list_node_ids(conn)?;
    let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let nodes: Vec<GraphNodeSummary> = read_nodes(conn, &id_refs)?
        .into_iter()
        .map(|node| GraphNodeSummary {
            id: node.id,
            title: node.title,
            node_type: node.node_type,
            tags: node.tags,
            importance: node.importance,
        })
        .collect();
    let edges: Vec<GraphEdge> = read_edges(conn, MAX_GRAPH_EDGES)?;
    Ok(Graph { nodes, edges })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::schema::init_graph_schema;
    use crate::graph::store::append_edge;
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        conn
    }

    fn insert_edge(conn: &Connection, id: &str, src: &str, tgt: &str) {
        let e = GraphEdge {
            id: id.to_string(),
            source: src.to_string(),
            target: tgt.to_string(),
            relation: "related".to_string(),
            weight: 1.0,
            ts: "2026-01-01T00:00:00Z".to_string(),
        };
        append_edge(conn, &e).unwrap();
    }

    #[test]
    fn neighbors_returns_direct_connections() {
        let conn = mem_db();
        insert_edge(&conn, "e1", "A", "B");
        insert_edge(&conn, "e2", "A", "C");
        insert_edge(&conn, "e3", "D", "A");

        let mut result = graph_neighbors(&conn, &["A".to_string()]);
        result.sort_by(|a, b| a.0.cmp(&b.0));
        let ids: Vec<&str> = result.iter().map(|r| r.0.as_str()).collect();
        assert!(ids.contains(&"B"));
        assert!(ids.contains(&"C"));
        assert!(ids.contains(&"D"));
        assert!(!ids.contains(&"A"));
    }

    #[test]
    fn neighbors_excludes_seeds() {
        let conn = mem_db();
        insert_edge(&conn, "e1", "A", "B");
        insert_edge(&conn, "e2", "B", "C");

        let result = graph_neighbors(&conn, &["A".to_string(), "B".to_string()]);
        let ids: Vec<&str> = result.iter().map(|r| r.0.as_str()).collect();
        assert!(ids.contains(&"C"));
        assert!(!ids.contains(&"A"));
        assert!(!ids.contains(&"B"));
    }

    #[test]
    fn neighbors_empty_seeds() {
        let conn = mem_db();
        assert!(graph_neighbors(&conn, &[]).is_empty());
    }

    #[test]
    fn related_nodes_recursive_bfs() {
        let conn = mem_db();
        insert_edge(&conn, "e1", "A", "B");
        insert_edge(&conn, "e2", "B", "C");

        let result = related_nodes(&conn, "A", 2);
        assert!(result.contains(&"B".to_string()));
        assert!(result.contains(&"C".to_string()));
        assert!(!result.contains(&"A".to_string()));
    }

    #[test]
    fn related_nodes_handles_cycles() {
        let conn = mem_db();
        insert_edge(&conn, "e1", "A", "B");
        insert_edge(&conn, "e2", "B", "C");
        insert_edge(&conn, "e3", "C", "A");

        let result = related_nodes(&conn, "A", 3);
        let unique: HashSet<_> = result.iter().collect();
        assert_eq!(result.len(), unique.len(), "no duplicates in cycle");
        assert!(!result.contains(&"A".to_string()));
    }

    #[test]
    fn neighbor_weight_accumulation() {
        let conn = mem_db();
        insert_edge(&conn, "e1", "A", "C");
        insert_edge(&conn, "e2", "B", "C");

        let result = graph_neighbors(&conn, &["A".to_string(), "B".to_string()]);
        let c_weight = result.iter().find(|(id, _)| id == "C").map(|(_, w)| *w);
        assert_eq!(c_weight, Some(2.0));
    }
}
