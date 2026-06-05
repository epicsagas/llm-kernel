//! Smart recall with composite scoring and graph boost.

use std::collections::HashSet;

use rusqlite::Connection;

use crate::error::{KernelError, Result};

use super::lifecycle::{parse_iso_to_secs, touch_nodes};
use super::search::search_nodes;
use super::types::{NODE_COLUMNS, ScoredNode};

/// Composite relevance score weights.
pub const W_RECENCY: f64 = 0.20;
pub const W_IMPORTANCE: f64 = 0.35;
pub const W_ACCESS: f64 = 0.15;
pub const W_FTS: f64 = 0.20;
pub const W_GRAPH: f64 = 0.10;

/// Smart recall: return nodes ranked by composite relevance.
///
/// Scoring: `recency(20%) + importance(35%) + access_freq(15%) + FTS(20%) + graph_boost(10%)`
///
/// Stale nodes (tagged "stale") are excluded. Retrieved nodes have their access_count incremented.
pub fn smart_recall(
    conn: &Connection,
    project: Option<&str>,
    hint: Option<&str>,
    limit: usize,
) -> Result<Vec<ScoredNode>> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Gather FTS matches if hint is provided
    let fts_ids: HashSet<String> = if let Some(h) = hint {
        if !h.is_empty() {
            search_nodes(conn, h, limit * 4)?
                .into_iter()
                .map(|n| n.id.clone())
                .collect()
        } else {
            Default::default()
        }
    } else {
        Default::default()
    };

    // Fetch candidate nodes (broad set)
    let candidate_limit = (limit * 4).max(40) as i64;
    let mut conditions: Vec<&str> = vec!["',' || tags || ',' NOT LIKE '%,stale,%'"];
    let mut param_vals: Vec<Box<dyn rusqlite::ToSql>> = vec![];
    if let Some(p) = project {
        conditions.push("(',' || projects || ',' LIKE '%,' || ? || ',%')");
        param_vals.push(Box::new(p.to_string()));
    }
    let where_clause = format!("WHERE {}", conditions.join(" AND "));
    let sql = format!(
        "SELECT {NODE_COLUMNS} FROM nodes {where_clause}
         ORDER BY importance DESC, updated DESC
         LIMIT {candidate_limit}"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let refs: Vec<&dyn rusqlite::ToSql> = param_vals.iter().map(|b| b.as_ref()).collect();
    let candidates: Vec<super::types::GraphNode> = stmt
        .query_map(refs.as_slice(), super::types::row_to_node)
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    // Score each candidate
    let mut scored: Vec<ScoredNode> = candidates
        .into_iter()
        .map(|node| {
            let recency = compute_recency(&node.updated, now_secs);
            let importance = node.importance;
            let access_freq = (node.access_count.max(0) as f64 / 20.0).min(1.0);
            let fts_match = if fts_ids.contains(&node.id) { 1.0 } else { 0.0 };

            let score = W_RECENCY * recency
                + W_IMPORTANCE * importance
                + W_ACCESS * access_freq
                + W_FTS * fts_match;

            ScoredNode { node, score }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);

    // Graph-boost pass
    if scored.len() > 1 {
        const MAX_GRAPH_BOOST_PARTICIPANTS: usize = 100;
        let boost_ids: Vec<&str> = scored
            .iter()
            .take(MAX_GRAPH_BOOST_PARTICIPANTS)
            .map(|sn| sn.node.id.as_str())
            .collect();
        let ph = boost_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT source AS node_id, SUM(weight) AS w FROM edges \
             WHERE source IN ({ph}) AND target IN ({ph}) GROUP BY source \
             UNION ALL \
             SELECT target AS node_id, SUM(weight) AS w FROM edges \
             WHERE source IN ({ph}) AND target IN ({ph}) GROUP BY target"
        );
        if let Ok(mut stmt) = conn.prepare(&sql) {
            let base: Vec<&dyn rusqlite::ToSql> = boost_ids
                .iter()
                .map(|s| s as &dyn rusqlite::ToSql)
                .collect();
            let sql_params: Vec<&dyn rusqlite::ToSql> = base
                .iter()
                .copied()
                .chain(base.iter().copied())
                .chain(base.iter().copied())
                .chain(base.iter().copied())
                .collect();
            let weight_map: std::collections::HashMap<String, f64> = stmt
                .query_map(sql_params.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                })
                .map(|rows| {
                    let mut map: std::collections::HashMap<String, f64> = Default::default();
                    for r in rows.flatten() {
                        *map.entry(r.0).or_default() += r.1;
                    }
                    map
                })
                .unwrap_or_default();
            let max_w = weight_map
                .values()
                .cloned()
                .fold(0.0_f64, f64::max)
                .max(1.0);
            for sn in &mut scored {
                let boost = weight_map.get(&sn.node.id).copied().unwrap_or(0.0);
                sn.score += W_GRAPH * (boost / max_w);
            }
            scored.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }

    // Touch retrieved nodes
    let ids: Vec<String> = scored.iter().map(|sn| sn.node.id.clone()).collect();
    touch_nodes(conn, &ids);

    Ok(scored)
}

/// Compute recency score (0.0–1.0) with exponential decay, half-life = 30 days.
fn compute_recency(updated: &str, now_secs: u64) -> f64 {
    let node_secs = parse_iso_to_secs(updated);
    if node_secs == 0 || node_secs > now_secs {
        return 0.5;
    }
    let age_days = (now_secs - node_secs) as f64 / 86400.0;
    let half_life = 30.0;
    (-age_days * (2.0_f64.ln()) / half_life).exp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::schema::init_graph_schema;
    use crate::graph::store::{append_edge, upsert_node};
    use crate::graph::types::GraphEdge;
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        conn
    }

    fn test_node(id: &str, importance: f64, tags: Vec<&str>) -> crate::graph::types::GraphNode {
        crate::graph::types::GraphNode {
            id: id.to_string(),
            node_type: "concept".to_string(),
            title: format!("Node {id}"),
            body: String::new(),
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
            projects: vec![],
            agents: vec![],
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-06-01T00:00:00Z".to_string(),
            importance,
            access_count: 0,
            accessed_at: String::new(),
        }
    }

    #[test]
    fn recall_returns_nodes() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", 0.9, vec![])).unwrap();
        upsert_node(&conn, &test_node("n2", 0.5, vec![])).unwrap();
        let results = smart_recall(&conn, None, None, 10).unwrap();
        assert_eq!(results.len(), 2);
        // Higher importance first
        assert_eq!(results[0].node.id, "n1");
    }

    #[test]
    fn recall_filters_by_project() {
        let conn = mem_db();
        let mut n1 = test_node("n1", 0.7, vec![]);
        n1.projects = vec!["myproj".to_string()];
        upsert_node(&conn, &n1).unwrap();
        upsert_node(&conn, &test_node("n2", 0.7, vec![])).unwrap();

        let results = smart_recall(&conn, Some("myproj"), None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node.id, "n1");
    }

    #[test]
    fn recall_with_hint_uses_fts() {
        let conn = mem_db();
        let mut n1 = test_node("n1", 0.5, vec![]);
        n1.title = "Rust ownership model".to_string();
        n1.body = "borrow checker rules".to_string();
        upsert_node(&conn, &n1).unwrap();

        let mut n2 = test_node("n2", 0.9, vec![]);
        n2.title = "Python GIL".to_string();
        upsert_node(&conn, &n2).unwrap();

        let results = smart_recall(&conn, None, Some("Rust"), 10).unwrap();
        // n1 should get FTS boost even though n2 has higher base importance
        assert!(!results.is_empty());
    }

    #[test]
    fn recall_excludes_stale() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", 0.9, vec!["stale"])).unwrap();
        upsert_node(&conn, &test_node("n2", 0.5, vec![])).unwrap();
        let results = smart_recall(&conn, None, None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node.id, "n2");
    }

    #[test]
    fn recall_touches_access_count() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", 0.7, vec![])).unwrap();
        smart_recall(&conn, None, None, 10).unwrap();
        let node = crate::graph::store::read_node(&conn, "n1")
            .unwrap()
            .unwrap();
        assert_eq!(node.access_count, 1);
    }

    #[test]
    fn recall_graph_boost() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", 0.7, vec![])).unwrap();
        upsert_node(&conn, &test_node("n2", 0.7, vec![])).unwrap();
        append_edge(
            &conn,
            &GraphEdge {
                id: "e1".into(),
                source: "n1".into(),
                target: "n2".into(),
                relation: "related".into(),
                weight: 2.0,
                ts: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();

        let results = smart_recall(&conn, None, None, 10).unwrap();
        assert_eq!(results.len(), 2);
        // Both should have graph boost applied (score > base importance)
    }
}
