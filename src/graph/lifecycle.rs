//! Node lifecycle: access tracking, importance decay, stale tagging, and stats.

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::error::{KernelError, Result};

use super::types::GraphStats;

// ── Access tracking ───────────────────────────────────

/// Record an access event: increment access_count and update accessed_at.
pub fn touch_node(conn: &Connection, id: &str) {
    let now = now_iso();
    let _ = conn.execute(
        "UPDATE nodes SET access_count = access_count + 1, accessed_at = ?1 WHERE id = ?2",
        params![now, id],
    );
}

/// Batch-touch multiple nodes.
pub fn touch_nodes(conn: &Connection, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let _ = conn.execute_batch("SAVEPOINT touch_batch");
    for id in ids {
        touch_node(conn, id);
    }
    let _ = conn.execute_batch("RELEASE touch_batch");
}

// ── Importance decay ──────────────────────────────────

/// Gradually decay importance for nodes not accessed in `days`.
///
/// Reduces importance by `factor` (e.g. 0.9 = 10% decay).
/// Nodes with importance at or below `floor` are not decayed further.
/// Nodes with the `pinned` tag are protected from decay.
///
/// Returns the number of nodes decayed.
pub fn decay_importance(conn: &Connection, days: u64, factor: f64, floor: f64) -> Result<u64> {
    let cutoff = compute_cutoff_timestamp(days);
    let changed = conn
        .execute(
            "UPDATE nodes SET importance = MAX(?3, importance * ?2)
             WHERE (accessed_at < ?1 OR accessed_at = '')
               AND updated < ?1
               AND importance > ?3
               AND ',' || tags || ',' NOT LIKE '%,pinned,%'",
            params![cutoff, factor, floor],
        )
        .map_err(|e| KernelError::Store(e.to_string()))?;
    Ok(changed as u64)
}

/// Tag nodes not updated within `days` as stale by appending "stale" to tags.
pub fn tag_stale_nodes(conn: &Connection, days: u64) -> Result<u64> {
    let cutoff = compute_cutoff_timestamp(days);
    let changed = conn
        .execute(
            "UPDATE nodes SET tags = CASE
                WHEN tags = '' THEN 'stale'
                WHEN ',' || tags || ',' NOT LIKE '%,stale,%' THEN tags || ',stale'
                ELSE tags
             END
             WHERE updated < ?1
               AND ',' || tags || ',' NOT LIKE '%,stale,%'",
            params![cutoff],
        )
        .map_err(|e| KernelError::Store(e.to_string()))?;
    Ok(changed as u64)
}

// ── Statistics ────────────────────────────────────────

/// Compute aggregate statistics about the knowledge graph.
pub fn compute_stats(conn: &Connection) -> Result<GraphStats> {
    let total_nodes: i64 = conn
        .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
        .unwrap_or(0);
    let total_edges: i64 = conn
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
        .unwrap_or(0);
    let avg_importance: f64 = conn
        .query_row("SELECT AVG(importance) FROM nodes", [], |r| r.get(0))
        .unwrap_or(0.0);

    let mut stmt = conn
        .prepare("SELECT type, COUNT(*) FROM nodes GROUP BY type")
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let by_type: HashMap<String, i64> = stmt
        .query_map([], |row| {
            let t: String = row.get(0)?;
            let c: i64 = row.get(1)?;
            Ok((t, c))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default();

    Ok(GraphStats {
        total_nodes,
        total_edges,
        avg_importance: (avg_importance * 100.0).round() / 100.0,
        by_type,
    })
}

// ── Timestamp helpers ─────────────────────────────────

fn compute_cutoff_timestamp(days: u64) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(days * 86400);
    let (y, m, d) = days_to_ymd(secs / 86400);
    let hh = (secs / 3600) % 24;
    let mm = (secs / 60) % 60;
    let ss = secs % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days = [
        31u64,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for md in &month_days {
        if days < *md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
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

/// Parse ISO 8601 timestamp to seconds since epoch (best-effort).
pub(crate) fn parse_iso_to_secs(ts: &str) -> u64 {
    if ts.len() < 19 {
        return 0;
    }
    let year: u64 = ts[0..4].parse().unwrap_or(0);
    let month: u64 = ts[5..7].parse().unwrap_or(1);
    let day: u64 = ts[8..10].parse().unwrap_or(1);
    let hour: u64 = ts[11..13].parse().unwrap_or(0);
    let min: u64 = ts[14..16].parse().unwrap_or(0);
    let sec: u64 = ts[17..19].parse().unwrap_or(0);

    let total_days = days_since_epoch(year, month, day);
    total_days * 86400 + hour * 3600 + min * 60 + sec
}

fn days_since_epoch(year: u64, month: u64, day: u64) -> u64 {
    let y = year as i64 - 1;
    let base = 1969i64;
    let leaps = (y / 4 - y / 100 + y / 400) - (base / 4 - base / 100 + base / 400);
    let days_from_years = (year as i64 - 1970) * 365 + leaps;

    const MONTH_DAYS: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let mut days_from_months: u64 = 0;
    let prior_months = (month.saturating_sub(1) as usize).min(12);
    for (m, &md) in MONTH_DAYS.iter().enumerate().take(prior_months) {
        days_from_months += md;
        if m == 1 && leap {
            days_from_months += 1;
        }
    }
    (days_from_years as u64) + days_from_months + day.saturating_sub(1)
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

    fn test_node(id: &str, importance: f64, tags: Vec<&str>) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            node_type: "concept".to_string(),
            title: format!("Node {id}"),
            body: String::new(),
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
            projects: vec![],
            agents: vec![],
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
            importance,
            access_count: 0,
            accessed_at: String::new(),
        }
    }

    #[test]
    fn touch_node_increments_count() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", 0.7, vec![])).unwrap();
        touch_node(&conn, "n1");
        touch_node(&conn, "n1");
        let node = crate::graph::store::read_node(&conn, "n1")
            .unwrap()
            .unwrap();
        assert_eq!(node.access_count, 2);
        assert!(!node.accessed_at.is_empty());
    }

    #[test]
    fn decay_reduces_importance() {
        let conn = mem_db();
        // Node updated 60 days ago → should decay
        upsert_node(&conn, &test_node("n1", 0.8, vec![])).unwrap();
        let changed = decay_importance(&conn, 30, 0.9, 0.05).unwrap();
        assert!(changed > 0);
        let node = crate::graph::store::read_node(&conn, "n1")
            .unwrap()
            .unwrap();
        assert!(node.importance < 0.8);
    }

    #[test]
    fn decay_skips_pinned() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", 0.9, vec!["pinned"])).unwrap();
        let changed = decay_importance(&conn, 30, 0.9, 0.05).unwrap();
        assert_eq!(changed, 0);
    }

    #[test]
    fn tag_stale_marks_old_nodes() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", 0.5, vec![])).unwrap();
        let changed = tag_stale_nodes(&conn, 30).unwrap();
        assert!(changed > 0);
        let node = crate::graph::store::read_node(&conn, "n1")
            .unwrap()
            .unwrap();
        assert!(node.tags.contains(&"stale".to_string()));
    }

    #[test]
    fn compute_stats_returns_counts() {
        let conn = mem_db();
        let mut n1 = test_node("n1", 0.7, vec![]);
        n1.node_type = "decision".to_string();
        upsert_node(&conn, &n1).unwrap();
        upsert_node(&conn, &test_node("n2", 0.5, vec![])).unwrap();

        let stats = compute_stats(&conn).unwrap();
        assert_eq!(stats.total_nodes, 2);
        assert_eq!(stats.total_edges, 0);
        assert!(stats.by_type.contains_key("decision"));
    }

    #[test]
    fn parse_iso_roundtrip() {
        let secs = parse_iso_to_secs("2026-01-15T12:30:45Z");
        assert!(secs > 0);
    }
}
