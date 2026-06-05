//! Schema initialization for the knowledge graph SQLite database.

use rusqlite::Connection;

use crate::error::{KernelError, Result};

/// Apply the full knowledge graph schema (tables, indexes, FTS5 triggers) to a connection.
///
/// Idempotent — uses `IF NOT EXISTS` for all DDL. Safe to call on every startup.
pub fn init_graph_schema(conn: &Connection) -> Result<()> {
    // WAL auto-checkpoint for better concurrency
    let _ = conn.execute_batch("PRAGMA wal_autocheckpoint=100;");

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS nodes (
            id           TEXT PRIMARY KEY,
            type         TEXT NOT NULL,
            title        TEXT NOT NULL,
            tags         TEXT NOT NULL DEFAULT '',
            projects     TEXT NOT NULL DEFAULT '',
            agents       TEXT NOT NULL DEFAULT '',
            created      TEXT NOT NULL,
            updated      TEXT NOT NULL,
            body         TEXT NOT NULL DEFAULT '',
            importance   REAL NOT NULL DEFAULT 0.5,
            access_count INTEGER NOT NULL DEFAULT 0,
            accessed_at  TEXT NOT NULL DEFAULT ''
        );

        -- FTS5 full-text search with trigram tokenizer
        CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts
            USING fts5(title, body, tags, content=nodes, content_rowid=rowid, tokenize='trigram');

        -- Keep FTS in sync with node changes
        CREATE TRIGGER IF NOT EXISTS nodes_ai AFTER INSERT ON nodes BEGIN
            INSERT INTO nodes_fts(rowid, title, body, tags)
            VALUES (new.rowid, new.title, new.body, new.tags);
        END;
        CREATE TRIGGER IF NOT EXISTS nodes_ad AFTER DELETE ON nodes BEGIN
            INSERT INTO nodes_fts(nodes_fts, rowid, title, body, tags)
            VALUES('delete', old.rowid, old.title, old.body, old.tags);
        END;
        CREATE TRIGGER IF NOT EXISTS nodes_au AFTER UPDATE ON nodes BEGIN
            INSERT INTO nodes_fts(nodes_fts, rowid, title, body, tags)
            VALUES('delete', old.rowid, old.title, old.body, old.tags);
            INSERT INTO nodes_fts(rowid, title, body, tags)
            VALUES (new.rowid, new.title, new.body, new.tags);
        END;

        CREATE TABLE IF NOT EXISTS edges (
            id       TEXT PRIMARY KEY,
            source   TEXT NOT NULL,
            target   TEXT NOT NULL,
            relation TEXT NOT NULL DEFAULT 'related',
            weight   REAL NOT NULL DEFAULT 1.0,
            ts       TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_edges_source  ON edges(source);
        CREATE INDEX IF NOT EXISTS idx_edges_target  ON edges(target);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_edges_src_tgt_rel ON edges(source, target, relation);
        CREATE INDEX IF NOT EXISTS idx_nodes_type    ON nodes(type);
        CREATE INDEX IF NOT EXISTS idx_nodes_updated ON nodes(updated DESC);
        CREATE INDEX IF NOT EXISTS idx_nodes_title_updated ON nodes(title, updated DESC);
        CREATE INDEX IF NOT EXISTS idx_nodes_importance ON nodes(importance DESC);
        CREATE INDEX IF NOT EXISTS idx_nodes_accessed ON nodes(accessed_at DESC);

        -- Schema version tracking
        CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT OR IGNORE INTO _meta (key, value) VALUES ('graph_schema_version', '1');
        ",
    )
    .map_err(|e| KernelError::Store(format!("Graph schema init failed: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn schema_creates_tables() {
        let conn = mem_db();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect();
        assert!(tables.contains(&"nodes".to_string()));
        assert!(tables.contains(&"edges".to_string()));
        assert!(tables.contains(&"_meta".to_string()));
    }

    #[test]
    fn schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_graph_schema(&conn).unwrap();
        init_graph_schema(&conn).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _meta WHERE key = 'graph_schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn fts_table_exists() {
        let conn = mem_db();
        let name: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='nodes_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "nodes_fts");
    }
}
