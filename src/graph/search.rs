//! FTS5 full-text search for knowledge graph nodes.

use rusqlite::{Connection, params};

use crate::error::{KernelError, Result};

use super::types::{GraphNode, NODE_COLUMNS_PREFIXED, escape_like, row_to_node};

/// Quote a raw user string as a single FTS5 phrase literal.
///
/// Callers pass free-form text (search boxes, LLM-generated hints). Handing that
/// straight to `MATCH` lets `"`, `*`, `NEAR`, `-` and friends be parsed as query
/// syntax — a stray quote turns the whole recall into a syntax error. Wrapping in
/// double quotes (with `"` doubled) makes the input an opaque phrase.
fn fts_phrase(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
}

/// Search nodes using FTS5 MATCH, ranked by BM25 relevance then importance.
///
/// The query is escaped as a phrase literal, so arbitrary user text is safe.
/// A malformed-query error is degraded to an empty result rather than an `Err`:
/// full-text is one signal among several in [`crate::graph::recall::smart_recall`], and a bad hint must
/// not take down the whole recall.
pub fn search_nodes(conn: &Connection, query: &str, limit: usize) -> Result<Vec<GraphNode>> {
    let sql = format!(
        "SELECT {NODE_COLUMNS_PREFIXED}
         FROM nodes n
         JOIN nodes_fts ON n.rowid = nodes_fts.rowid
         WHERE nodes_fts MATCH ?1
         ORDER BY bm25(nodes_fts), n.importance DESC
         LIMIT ?2"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| KernelError::Store(e.to_string()))?;
    let rows = match stmt.query_map(params![fts_phrase(query), limit as i64], row_to_node) {
        Ok(rows) => rows,
        // Malformed FTS5 expression — treat as "no lexical matches".
        Err(_) => return Ok(Vec::new()),
    };
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Search nodes across every available lexical backend, de-duplicated by id.
///
/// FTS5's `trigram` tokenizer cannot match queries shorter than three
/// characters, which silently breaks most CJK lookups (a 2-syllable Korean query
/// like `"매수"` returns nothing even when many nodes contain it). With the
/// `graph-cjk` feature the substring path in [`crate::graph::cjk::search_nodes_cjk`] covers exactly
/// that gap, so the union is strictly better than either source alone:
/// FTS contributes ranked multi-character hits, CJK contributes short and
/// mid-word ones.
///
/// FTS results keep their BM25 order and come first; CJK-only hits follow.
/// Without `graph-cjk` this is exactly [`search_nodes`].
pub fn search_nodes_hybrid(conn: &Connection, query: &str, limit: usize) -> Result<Vec<GraphNode>> {
    let mut out = search_nodes(conn, query, limit)?;

    #[cfg(feature = "graph-cjk")]
    {
        use std::collections::HashSet;
        let seen: HashSet<String> = out.iter().map(|n| n.id.clone()).collect();
        let cjk = super::cjk::search_nodes_cjk(conn, query, limit)?;
        out.extend(cjk.into_iter().filter(|n| !seen.contains(&n.id)));
        out.truncate(limit);
    }

    Ok(out)
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
        condition_strs.push("(',' || tags || ',' LIKE '%,' || ? || ',%' ESCAPE '\\')");
        param_vals.push(Box::new(escape_like(t)));
    }
    if let Some(nt) = node_type {
        condition_strs.push("type = ?");
        param_vals.push(Box::new(nt.to_string()));
    }
    if let Some(p) = project {
        condition_strs.push("(',' || projects || ',' LIKE '%,' || ? || ',%' ESCAPE '\\')");
        param_vals.push(Box::new(escape_like(p)));
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

/// Ordering for [`query_nodes_ex`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NodeOrder {
    /// `created DESC` — newest first. Activates `idx_nodes_created`.
    #[default]
    CreatedDesc,
    /// `created ASC` — oldest first (timeline reconstruction).
    CreatedAsc,
    /// `updated DESC`.
    UpdatedDesc,
    /// `importance DESC`.
    ImportanceDesc,
}

/// Structured node query with paging, time range, and ordering.
///
/// `#[non_exhaustive]` + `Default` lets callers add filters in future releases
/// without breaking struct-literal construction (`NodeQuery { limit: 50, ..Default::default() }`).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NodeQuery {
    /// Tag exact-match (CSV membership test). Use for `symbol`-scoped queries.
    pub tag: Option<String>,
    /// Node type filter (`decision`, `stock`, ...).
    pub node_type: Option<String>,
    /// Project scope filter.
    pub project: Option<String>,
    /// `created >=` (RFC3339/ISO8601). Uses `idx_nodes_created`.
    pub since: Option<String>,
    /// `created <` (RFC3339/ISO8601).
    pub until: Option<String>,
    /// Result ordering.
    pub order_by: NodeOrder,
    /// Max rows. Capped at 200 to bound work. Defaults to 50.
    pub limit: usize,
    /// Skip this many rows.
    pub offset: usize,
}

impl Default for NodeQuery {
    fn default() -> Self {
        Self {
            tag: None,
            node_type: None,
            project: None,
            since: None,
            until: None,
            order_by: NodeOrder::default(),
            limit: 50,
            offset: 0,
        }
    }
}

impl NodeQuery {
    fn order_clause(&self) -> &'static str {
        match self.order_by {
            NodeOrder::CreatedDesc => "created DESC",
            NodeOrder::CreatedAsc => "created ASC",
            NodeOrder::UpdatedDesc => "updated DESC",
            NodeOrder::ImportanceDesc => "importance DESC",
        }
    }
}

/// Dynamic filter query: tag, node_type, project, time range, paging, ordering.
///
/// `limit` is capped at 200 server-side but supports `offset` for true paging
/// without materializing the full table client-side.
pub fn query_nodes_ex(conn: &Connection, q: &NodeQuery) -> Result<Vec<GraphNode>> {
    let limit = q.limit.min(200) as i64;
    let offset = q.offset as i64;

    let mut condition_strs: Vec<&str> = vec![];
    let mut param_vals: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(t) = &q.tag {
        condition_strs.push("(',' || tags || ',' LIKE '%,' || ? || ',%' ESCAPE '\\')");
        param_vals.push(Box::new(escape_like(t)));
    }
    if let Some(nt) = &q.node_type {
        condition_strs.push("type = ?");
        param_vals.push(Box::new(nt.clone()));
    }
    if let Some(p) = &q.project {
        condition_strs.push("(',' || projects || ',' LIKE '%,' || ? || ',%' ESCAPE '\\')");
        param_vals.push(Box::new(escape_like(p)));
    }
    if let Some(s) = &q.since {
        condition_strs.push("created >= ?");
        param_vals.push(Box::new(s.clone()));
    }
    if let Some(u) = &q.until {
        condition_strs.push("created < ?");
        param_vals.push(Box::new(u.clone()));
    }

    let where_clause = if condition_strs.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", condition_strs.join(" AND "))
    };
    let order = q.order_clause();
    let node_columns = super::types::NODE_COLUMNS;
    let sql = format!(
        "SELECT {node_columns} FROM nodes {where_clause} ORDER BY {order} LIMIT ? OFFSET ?"
    );

    param_vals.push(Box::new(limit));
    param_vals.push(Box::new(offset));

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

    #[test]
    fn query_tag_wildcard_is_escaped() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", "A", "body", vec!["rust"])).unwrap();
        // "ru%t" would match "rust" as a LIKE wildcard, but escape_like prevents it
        let results = query_nodes(&conn, Some("ru%t"), None, None, 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn query_project_wildcard_is_escaped() {
        let conn = mem_db();
        let mut n1 = test_node("n1", "A", "body", vec![]);
        n1.projects = vec!["myproj".to_string()];
        upsert_node(&conn, &n1).unwrap();
        let results = query_nodes(&conn, None, None, Some("my%"), 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn fts_query_with_quotes_does_not_error() {
        // A raw `"` used to be parsed as FTS5 syntax and blew up the whole recall.
        let conn = mem_db();
        upsert_node(&conn, &test_node("n1", "quoted", "body", vec![])).unwrap();
        assert!(search_nodes(&conn, "say \"hello\"", 10).is_ok());
        assert!(search_nodes(&conn, "trailing *", 10).is_ok());
        assert!(search_nodes(&conn, "NEAR OR AND", 10).is_ok());
    }

    #[cfg(feature = "graph-cjk")]
    #[test]
    fn hybrid_matches_short_korean_that_trigram_misses() {
        // Regression baseline from the TradingAgentOS KB: a 2-syllable Korean
        // query is invisible to the trigram tokenizer but present in the corpus.
        let conn = mem_db();
        upsert_node(
            &conn,
            &test_node(
                "d1",
                "SK하이닉스 판정",
                "매수 의견을 유지한다",
                vec!["hold"],
            ),
        )
        .unwrap();

        // trigram alone: no hit for the 2-char query.
        assert!(search_nodes(&conn, "매수", 10).unwrap().is_empty());
        // hybrid: CJK substring path finds it.
        assert_eq!(search_nodes_hybrid(&conn, "매수", 10).unwrap().len(), 1);
        assert_eq!(search_nodes_hybrid(&conn, "SK", 10).unwrap().len(), 1);
    }

    #[cfg(feature = "graph-cjk")]
    #[test]
    fn hybrid_negative_control_absent_term_stays_empty() {
        // Guards against a "LIKE matches anything" regression: a term that is
        // genuinely not in the corpus must still return nothing.
        let conn = mem_db();
        upsert_node(&conn, &test_node("d1", "SK하이닉스", "매수 의견", vec![])).unwrap();
        assert!(search_nodes_hybrid(&conn, "반도체", 10).unwrap().is_empty());
        assert!(
            search_nodes_hybrid(&conn, "존재하지않는단어", 10)
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(feature = "graph-cjk")]
    #[test]
    fn hybrid_dedups_nodes_found_by_both_paths() {
        let conn = mem_db();
        upsert_node(&conn, &test_node("d1", "삼성전자", "반도체 실적", vec![])).unwrap();
        // "삼성전자" is long enough for trigram AND matches the CJK substring path.
        assert_eq!(search_nodes_hybrid(&conn, "삼성전자", 10).unwrap().len(), 1);
    }
}
