//! Schema initialization for the knowledge graph SQLite database.

use rusqlite::{Connection, params};

use crate::error::{KernelError, Result};

/// Current graph schema version. Increment when adding migrations.
pub const GRAPH_SCHEMA_VERSION: u32 = 2;

/// Read the recorded graph schema version from `_meta`, or `0` if unset.
pub fn schema_version(conn: &Connection) -> Result<u32> {
    Ok(conn
        .query_row(
            "SELECT value FROM _meta WHERE key = 'graph_schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
}

/// Incremental migration for the knowledge graph schema.
///
/// Applies version-to-version migrations from `current` up to
/// [`GRAPH_SCHEMA_VERSION`] inside a single transaction. On failure the
/// transaction rolls back and the recorded version is unchanged. Returns the
/// new version (equal to `current` when already up to date).
pub fn migrate_graph(conn: &Connection, current: u32) -> Result<u32> {
    if current >= GRAPH_SCHEMA_VERSION {
        return Ok(current);
    }
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| KernelError::Store(format!("migration begin failed: {e}")))?;
    let mut v = current;
    // v1 -> v2: index nodes by creation timestamp (used by recency ordering).
    if v < 2 {
        tx.execute_batch("CREATE INDEX IF NOT EXISTS idx_nodes_created ON nodes(created);")
            .map_err(|e| KernelError::Store(format!("migration v1->v2 failed: {e}")))?;
        v = 2;
    }
    tx.execute(
        "UPDATE _meta SET value = ?1 WHERE key = 'graph_schema_version'",
        params![v.to_string()],
    )
    .map_err(|e| KernelError::Store(e.to_string()))?;
    tx.commit()
        .map_err(|e| KernelError::Store(format!("migration commit failed: {e}")))?;
    Ok(v)
}

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
        CREATE INDEX IF NOT EXISTS idx_nodes_created ON nodes(created);

        -- Schema version tracking
        CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        INSERT OR IGNORE INTO _meta (key, value) VALUES ('graph_schema_version', '2');
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

    /// Helper: force the recorded version down to `v` (simulates an older DB).
    fn set_version(conn: &Connection, v: u32) {
        conn.execute(
            "UPDATE _meta SET value = ?1 WHERE key = 'graph_schema_version'",
            params![v.to_string()],
        )
        .unwrap();
    }

    fn has_index(conn: &Connection, name: &str) -> bool {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name = ?1",
                params![name],
                |r| r.get(0),
            )
            .unwrap();
        count > 0
    }

    #[test]
    fn schema_version_reads_meta() {
        let conn = mem_db();
        assert_eq!(schema_version(&conn).unwrap(), GRAPH_SCHEMA_VERSION);
        set_version(&conn, 1);
        assert_eq!(schema_version(&conn).unwrap(), 1);
    }

    /// AC5: a v1 database migrates up to the current version, applying the
    /// v1→v2 step (the `idx_nodes_created` index becomes observable).
    #[test]
    fn migrate_advances_v1_to_current() {
        let conn = mem_db();
        // Drop the v2 index, then rewind the version to simulate a v1 DB.
        conn.execute_batch("DROP INDEX IF EXISTS idx_nodes_created;")
            .unwrap();
        set_version(&conn, 1);
        assert!(!has_index(&conn, "idx_nodes_created"));

        let new_version = migrate_graph(&conn, 1).unwrap();
        assert_eq!(new_version, GRAPH_SCHEMA_VERSION);
        assert_eq!(schema_version(&conn).unwrap(), GRAPH_SCHEMA_VERSION);
        assert!(has_index(&conn, "idx_nodes_created"));
    }

    /// AC5: migrating an already-current DB is a no-op.
    #[test]
    fn migrate_is_noop_when_current() {
        let conn = mem_db();
        let v = migrate_graph(&conn, GRAPH_SCHEMA_VERSION).unwrap();
        assert_eq!(v, GRAPH_SCHEMA_VERSION);
    }

    /// AC5: a migration whose step fails rolls back, leaving the version
    /// unchanged. We force failure by dropping the `nodes` table so the
    /// `CREATE INDEX … ON nodes` step cannot succeed.
    #[test]
    fn migrate_rolls_back_on_failure() {
        let conn = mem_db();
        set_version(&conn, 1);
        // Sabotage the migration target so the v1→v2 CREATE INDEX fails.
        conn.execute_batch("DROP TABLE nodes;").unwrap();

        let result = migrate_graph(&conn, 1);
        assert!(result.is_err(), "expected migration to fail");
        // Version unchanged because the transaction rolled back.
        assert_eq!(schema_version(&conn).unwrap(), 1);
    }
}
