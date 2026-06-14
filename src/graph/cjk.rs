//! CJK-aware graph search (Rust-side segmentation, **no schema change**).
//!
//! The bundled FTS5 table uses the `trigram` tokenizer, which matches poorly
//! for short CJK queries (a 1–2 character Japanese/Korean query produces no
//! usable trigram). This module adds a parallel CJK search path that:
//!
//! 1. Segments the query in Rust — each CJK character becomes its own token,
//!    Latin/digit runs stay intact ([`segment_cjk`]).
//! 2. Matches tokens as substrings (`LIKE '%tok%'`) against `title`, `body`,
//!    and `tags`, requiring every token to be present (AND semantics), ranked by
//!    importance.
//!
//! Because this is plain `LIKE` over the existing columns, it touches **no
//! SQLite DDL** — `init_graph_schema` is identical with or without `graph-cjk`,
//! so a database created without the feature is safe to use with it and
//! vice-versa. The cost is a scan per query (no FTS index), acceptable for the
//! small-to-medium graphs this foundation targets.

use rusqlite::Connection;

use crate::error::{KernelError, Result};

use super::types::{GraphNode, NODE_COLUMNS, escape_like, row_to_node};

/// Returns `true` for CJK ideographs, kana, Hangul, and fullwidth forms.
fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{3040}'..='\u{30FF}'   // Hiragana, Katakana, Katakana phonetic extensions
            | '\u{3400}'..='\u{4DBF}' // CJK Unified Ideographs Extension A
            | '\u{4E00}'..='\u{9FFF}' // CJK Unified Ideographs
            | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
            | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
            | '\u{FF00}'..='\u{FFEF}' // Fullwidth ASCII / Halfwidth Fullwidth forms
    )
}

/// Insert a space after every CJK character so whitespace tokenization treats
/// each CJK character as a separate token. Latin/digit/punctuation runs and
/// existing whitespace are left untouched.
///
/// Examples: `"知識グラフ"` → `"知 識 グ ラ フ "`,
/// `"rust ownership"` → `"rust ownership"`, `"Rustで非同期"` → `"Rust で 非 同 期 "`.
pub fn segment_cjk(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        out.push(ch);
        if is_cjk_char(ch) {
            out.push(' ');
        }
    }
    out
}

/// Search nodes for a CJK (or mixed) query via **contiguous substring** matching.
///
/// The query is split on whitespace into terms; every term must appear as a
/// contiguous, case-insensitive substring of `title`, `body`, or `tags`
/// (AND semantics). A single-token CJK query like `"グラフ"` therefore matches
/// only nodes containing that exact character run — not any node with the three
/// characters scattered anywhere — which keeps precision high. Callers that
/// want character-level recall for an unsegmented multi-word CJK query may
/// pre-process it with [`segment_cjk`]. Results are ranked by importance DESC
/// and capped at `limit`.
pub fn search_nodes_cjk(conn: &Connection, query: &str, limit: usize) -> Result<Vec<GraphNode>> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    // Each term contributes a (title LIKE ? OR body LIKE ? OR tags LIKE ?) group;
    // groups are AND-ed so every token must be present.
    let term_cond = "(lower(title) LIKE ? ESCAPE '\\' OR lower(body) LIKE ? ESCAPE '\\' OR lower(tags) LIKE ? ESCAPE '\\')";
    let where_clause = terms
        .iter()
        .map(|_| term_cond)
        .collect::<Vec<_>>()
        .join(" AND ");

    let mut bind: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(terms.len() * 3 + 1);
    for t in &terms {
        let pat = format!("%{}%", escape_like(t));
        bind.push(Box::new(pat.clone()));
        bind.push(Box::new(pat.clone()));
        bind.push(Box::new(pat));
    }
    bind.push(Box::new(limit as i64));

    let sql = format!(
        "SELECT {NODE_COLUMNS} FROM nodes WHERE {where_clause} ORDER BY importance DESC LIMIT ?"
    );
    let refs: Vec<&dyn rusqlite::ToSql> = bind.iter().map(|b| b.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| KernelError::Store(e.to_string()))?;
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

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        conn
    }

    fn node(id: &str, title: &str, body: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            node_type: "concept".to_string(),
            title: title.to_string(),
            body: body.to_string(),
            tags: vec![],
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
    fn segment_separates_cjk_chars() {
        assert_eq!(segment_cjk("知識"), "知 識 ");
        // Latin runs are untouched.
        assert_eq!(segment_cjk("rust async"), "rust async");
        // Mixed: each CJK character is followed by a space; the Latin run is intact.
        let seg = segment_cjk("Rustでトークン");
        assert!(seg.starts_with("Rustで "), "got: {seg:?}");
        assert!(seg.contains("ト "), "got: {seg:?}");
    }

    /// AC1: a CJK-titled node is found by a CJK query via the CJK search path.
    #[test]
    fn cjk_search_finds_cjk_node() {
        let conn = mem_db();
        upsert_node(
            &conn,
            &node("k1", "知識グラフの構築", "ナレッジベースをグラフ化する"),
        )
        .unwrap();
        upsert_node(&conn, &node("k2", "Python GIL", "global interpreter lock")).unwrap();

        // A short CJK query that trigram would not match reliably.
        let hits = search_nodes_cjk(&conn, "グラフ", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "k1");
    }

    /// AC1: multi-token CJK query requires every token.
    #[test]
    fn cjk_search_and_semantics() {
        let conn = mem_db();
        upsert_node(&conn, &node("k1", "知識グラフ", "説明本文")).unwrap();
        // "グラフ 存在" — k1 has グラフ but not 存在 → no match.
        let hits = search_nodes_cjk(&conn, "グラフ 存在", 10).unwrap();
        assert!(hits.is_empty());
        // "知識 グラフ" — both present → match.
        let hits = search_nodes_cjk(&conn, "知識 グラフ", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn empty_query_returns_nothing() {
        let conn = mem_db();
        upsert_node(&conn, &node("k1", "知識", "body")).unwrap();
        assert!(search_nodes_cjk(&conn, "   ", 10).unwrap().is_empty());
    }
}
