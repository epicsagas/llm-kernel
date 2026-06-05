//! FTS5 full-text search for knowledge graph nodes.

use rusqlite::{Connection, params};

use crate::error::{KernelError, Result};

use super::types::{GraphNode, NODE_COLUMNS_PREFIXED, row_to_node};

/// Search nodes using FTS5 MATCH. Results ranked by importance DESC.
pub fn search_nodes(conn: &Connection, query: &str, limit: usize) -> Result<Vec<GraphNode>> {
    let sql = format!(
        "SELECT {NODE_COLUMNS_PREFIXED}
         FROM nodes n
         JOIN nodes_fts ON n.rowid = nodes_fts.rowid
         WHERE nodes_fts MATCH ?1
         ORDER BY n.importance DESC
         LIMIT ?2"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let nodes: Vec<GraphNode> = stmt
        .query_map(params![query, limit as i64], row_to_node)
        .map_err(|e| KernelError::Store(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(nodes)
}

/// Dynamic filter query: filter by tag, node_type, and/or project.
pub fn query_nodes(
    conn: &Connection,
    tag: Option<&str>,
    node_type: Option<&str>,
    project: Option<&str>,
    limit: usize,
) -> Result<Vec<GraphNode>> {
    let limit = limit.min(200);

    let mut condition_strs: Vec<&str> = vec![];
    let mut param_vals: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(t) = tag {
        condition_strs.push("(',' || tags || ',' LIKE '%,' || ? || ',%')");
        param_vals.push(Box::new(t.to_string()));
    }
    if let Some(nt) = node_type {
        condition_strs.push("type = ?");
        param_vals.push(Box::new(nt.to_string()));
    }
    if let Some(p) = project {
        condition_strs.push("(',' || projects || ',' LIKE '%,' || ? || ',%')");
        param_vals.push(Box::new(p.to_string()));
    }

    let where_clause = if condition_strs.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", condition_strs.join(" AND "))
    };

    let node_columns = super::types::NODE_COLUMNS;
    let sql = format!(
        "SELECT {node_columns} FROM nodes {where_clause} ORDER BY updated DESC LIMIT {}",
        limit as i64,
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let refs: Vec<&dyn rusqlite::ToSql> = param_vals.iter().map(|b| b.as_ref()).collect();
    let nodes: Vec<GraphNode> = stmt
        .query_map(refs.as_slice(), row_to_node)
        .map_err(|e| KernelError::Store(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::schema::init_graph_schema;
    use crate::graph::store::upsert_node;
    use crate::graph::types::GraphNode;
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        conn
    }

    fn test_node(id: &str, title: &str, body: &str, tags: Vec<&str>) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            node_type: "concept".to_string(),
            title: title.to_string(),
            body: body.to_string(),
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
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
    fn search_finds_by_title() {
        let conn = mem_db();
        upsert_node(
            &conn,
            &test_node("n1", "Rust ownership", "borrow checker", vec![]),
        )
        .unwrap();
        upsert_node(&conn, &test_node("n2", "Python GIL", "global lock", vec![])).unwrap();
        let results = search_nodes(&conn, "Rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "n1");
    }

    #[test]
    fn search_finds_by_body() {
        let conn = mem_db();
        upsert_node(
            &conn,
            &test_node("n1", "Title", "machine learning models", vec![]),
        )
        .unwrap();
        let results = search_nodes(&conn, "machine learning", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn query_filters_by_tag() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", "A", "body", vec!["rust", "async"])).unwrap();
        upsert_node(&conn, &test_node("n2", "B", "body", vec!["python"])).unwrap();
        let results = query_nodes(&conn, Some("rust"), None, None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "n1");
    }

    #[test]
    fn query_filters_by_type() {
        let conn = mem_db();
        let mut n1 = test_node("n1", "A", "body", vec![]);
        n1.node_type = "decision".to_string();
        upsert_node(&conn, &n1).unwrap();
        let results = query_nodes(&conn, None, Some("decision"), None, 10).unwrap();
        assert_eq!(results.len(), 1);
    }
}
