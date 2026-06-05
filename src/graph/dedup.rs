//! Node deduplication: prevent duplicate nodes by title within a time window.

use rusqlite::{Connection, params};

use crate::error::{KernelError, Result};

use super::lifecycle::days_to_ymd;
use super::store::upsert_node;
use super::types::GraphNode;

/// Write with deduplication. If a node with the same `title` was written within
/// `window_hours`, return the existing ID instead. Otherwise, insert and return
/// the new ID.
///
/// Returns `(id, was_deduplicated)`.
pub fn upsert_node_dedup(
    conn: &Connection,
    node: &GraphNode,
    window_hours: u64,
) -> Result<(String, bool)> {
    if let Some(existing_id) = find_duplicate(conn, &node.title, window_hours)? {
        return Ok((existing_id, true));
    }
    upsert_node(conn, node)?;
    Ok((node.id.clone(), false))
}

/// Find a node with the given title updated within `window_hours`.
pub fn find_duplicate(conn: &Connection, title: &str, window_hours: u64) -> Result<Option<String>> {
    let cutoff = compute_cutoff_timestamp(window_hours);
    let result = conn.query_row(
        "SELECT id FROM nodes WHERE title = ?1 AND updated > ?2 ORDER BY updated DESC LIMIT 1",
        params![title, cutoff],
        |row| row.get::<_, String>(0),
    );
    match result {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(KernelError::Store(e.to_string())),
    }
}

fn compute_cutoff_timestamp(window_hours: u64) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(window_hours * 3600);
    let (y, m, d) = days_to_ymd(secs / 86400);
    let hh = (secs / 3600) % 24;
    let mm = (secs / 60) % 60;
    let ss = secs % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::schema::init_graph_schema;
    use crate::graph::store::read_node;
    use crate::graph::types::GraphNode;
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        conn
    }

    fn now_iso() -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let (y, m, d) = days_to_ymd(secs / 86400);
        let hh = (secs / 3600) % 24;
        let mm = (secs / 60) % 60;
        let ss = secs % 60;
        format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
    }

    fn test_node(id: &str, title: &str) -> GraphNode {
        let now = now_iso();
        GraphNode {
            id: id.to_string(),
            node_type: "concept".to_string(),
            title: title.to_string(),
            body: String::new(),
            tags: vec![],
            projects: vec![],
            agents: vec![],
            created: now.clone(),
            updated: now,
            importance: 0.5,
            access_count: 0,
            accessed_at: String::new(),
        }
    }

    #[test]
    fn dedup_inserts_new_node() {
        let conn = mem_db();
        let (id, was_dup) = upsert_node_dedup(&conn, &test_node("n1", "My Title"), 24).unwrap();
        assert_eq!(id, "n1");
        assert!(!was_dup);
        assert!(read_node(&conn, "n1").unwrap().is_some());
    }

    #[test]
    fn dedup_returns_existing_for_same_title() {
        let conn = mem_db();
        upsert_node_dedup(&conn, &test_node("n1", "Same Title"), 24).unwrap();

        // Second write with same title but different ID
        let (id, was_dup) = upsert_node_dedup(&conn, &test_node("n2", "Same Title"), 24).unwrap();
        assert_eq!(id, "n1");
        assert!(was_dup);

        // Original node still exists, second was NOT written
        assert!(read_node(&conn, "n1").unwrap().is_some());
        assert!(read_node(&conn, "n2").unwrap().is_none());
    }

    #[test]
    fn dedup_allows_after_window_expires() {
        let conn = mem_db();
        // Insert with a very old timestamp (before the window)
        let mut old_node = test_node("n1", "Old Title");
        old_node.updated = "2020-01-01T00:00:00Z".to_string();
        upsert_node(&conn, &old_node).unwrap();

        // New write with same title should succeed (old one is outside window)
        let (id, was_dup) = upsert_node_dedup(&conn, &test_node("n2", "Old Title"), 24).unwrap();
        assert_eq!(id, "n2");
        assert!(!was_dup);
    }

    #[test]
    fn find_duplicate_returns_none_when_empty() {
        let conn = mem_db();
        let result = find_duplicate(&conn, "nonexistent", 24).unwrap();
        assert!(result.is_none());
    }
}
