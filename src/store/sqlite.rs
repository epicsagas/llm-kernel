use std::path::Path;

use rusqlite::Connection;

use crate::error::{KernelError, Result};

/// Initialize a SQLite database with WAL mode, foreign keys, and a custom schema.
///
/// Creates parent directories if needed. Applies standard PRAGMAs for safety
/// and performance. Runs the provided DDL to create tables, then validates
/// the schema version matches expectations.
pub fn init_schema(path: &Path, schema_ddl: &str, expected_version: u32) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(path).map_err(|e| KernelError::Store(e.to_string()))?;
    apply_pragma(&conn)?;

    // Check current schema version
    let current_version: u32 = conn
        .query_row(
            "SELECT value FROM _meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version != expected_version {
        // Fresh or outdated — apply full DDL
        conn.execute_batch(schema_ddl)
            .map_err(|e| KernelError::Store(format!("Schema DDL failed: {}", e)))?;

        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', ?1)",
            [expected_version.to_string()],
        )
        .map_err(|e| KernelError::Store(e.to_string()))?;
    }

    Ok(conn)
}

/// Create an in-memory SQLite connection with PRAGMAs applied.
/// Useful for testing.
pub fn init_in_memory(schema_ddl: &str) -> Result<Connection> {
    let conn = Connection::open_in_memory().map_err(|e| KernelError::Store(e.to_string()))?;
    apply_pragma(&conn)?;
    conn.execute_batch(schema_ddl)
        .map_err(|e| KernelError::Store(format!("Schema DDL failed: {}", e)))?;
    Ok(conn)
}

fn apply_pragma(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA synchronous = NORMAL;",
    )
    .map_err(|e| KernelError::Store(format!("PRAGMA failed: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_schema_creates_db() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let ddl = "CREATE TABLE _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                   INSERT OR IGNORE INTO _meta (key, value) VALUES ('schema_version', '0');
                   CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL);";

        let conn = init_schema(&path, ddl, 1).unwrap();
        let version: String = conn
            .query_row(
                "SELECT value FROM _meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "1");
    }

    #[test]
    fn test_init_in_memory() {
        let ddl = "CREATE TABLE _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                   INSERT OR IGNORE INTO _meta (key, value) VALUES ('schema_version', '0');
                   CREATE TABLE items (id TEXT PRIMARY KEY);";
        let conn = init_in_memory(ddl).unwrap();
        conn.execute("INSERT INTO items (id) VALUES ('a')", [])
            .unwrap();
    }

    #[test]
    fn test_init_schema_idempotent() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let ddl = "CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                   INSERT OR IGNORE INTO _meta (key, value) VALUES ('schema_version', '0');
                   CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY);";

        let conn1 = init_schema(&path, ddl, 1).unwrap();
        drop(conn1);
        let conn2 = init_schema(&path, ddl, 1).unwrap();

        let count: u32 = conn2
            .query_row(
                "SELECT COUNT(*) FROM _meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
