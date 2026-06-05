use std::path::Path;

use rusqlite::Connection;

use crate::error::{KernelError, Result};

/// Migration callback: given a connection and the current schema version,
/// apply incremental migrations and return the new version.
pub type MigrationFn = fn(&Connection, current_version: u32) -> Result<u32>;

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

    // Check current schema version (stored as TEXT — parse to u32)
    let current_version: u32 = conn
        .query_row(
            "SELECT value FROM _meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse().ok())
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

/// Initialize a SQLite database with optional incremental migration support.
///
/// Like [`init_schema`], but when the current version is behind `expected_version`,
/// calls the `migrate` callback instead of replaying the full DDL. Falls back to
/// full DDL when `migrate` is `None` or the DB is fresh (version 0).
pub fn init_schema_with_migrations(
    path: &Path,
    schema_ddl: &str,
    expected_version: u32,
    migrate: Option<MigrationFn>,
) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(path).map_err(|e| KernelError::Store(e.to_string()))?;
    apply_pragma(&conn)?;

    // Query version — _meta may not exist yet (fresh DB), treated as version 0
    let current_version: u32 = conn
        .query_row(
            "SELECT value FROM _meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if current_version == expected_version {
        return Ok(conn);
    }

    // Fresh DB or no migration callback → full DDL
    if current_version == 0 || migrate.is_none() {
        conn.execute_batch(schema_ddl)
            .map_err(|e| KernelError::Store(format!("Schema DDL failed: {}", e)))?;

        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', ?1)",
            [expected_version.to_string()],
        )
        .map_err(|e| KernelError::Store(e.to_string()))?;

        return Ok(conn);
    }

    // Incremental migration
    let new_version = migrate.unwrap()(&conn, current_version)?;
    conn.execute(
        "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', ?1)",
        [new_version.to_string()],
    )
    .map_err(|e| KernelError::Store(e.to_string()))?;

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

    fn noop_migrate(_conn: &Connection, _current: u32) -> Result<u32> {
        Ok(2)
    }

    #[test]
    fn migration_fresh_db_uses_full_ddl() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let ddl = "CREATE TABLE _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                   INSERT OR IGNORE INTO _meta (key, value) VALUES ('schema_version', '0');
                   CREATE TABLE items (id TEXT PRIMARY KEY);";

        let conn = init_schema_with_migrations(&path, ddl, 2, Some(noop_migrate)).unwrap();
        let version: String = conn
            .query_row(
                "SELECT value FROM _meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // Fresh DB → full DDL sets version to 2 directly (migration not invoked)
        assert_eq!(version, "2");
    }

    #[test]
    fn migration_same_version_skips() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let ddl = "CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                   INSERT OR IGNORE INTO _meta (key, value) VALUES ('schema_version', '0');
                   CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY);";

        let conn1 = init_schema(&path, ddl, 2).unwrap();
        drop(conn1);

        let conn2 = init_schema_with_migrations(&path, ddl, 2, Some(noop_migrate)).unwrap();
        let version: String = conn2
            .query_row(
                "SELECT value FROM _meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "2");
    }

    #[test]
    fn migration_callback_invoked_on_version_mismatch() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let ddl = "CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                   INSERT OR IGNORE INTO _meta (key, value) VALUES ('schema_version', '0');
                   CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY);";

        // First init at version 1
        let conn1 = init_schema(&path, ddl, 1).unwrap();
        drop(conn1);

        // Now "upgrade" to version 3 via migration
        let migrate = |conn: &Connection, current: u32| -> Result<u32> {
            assert_eq!(current, 1);
            conn.execute_batch("ALTER TABLE items ADD COLUMN label TEXT DEFAULT ''")
                .map_err(|e| KernelError::Store(e.to_string()))?;
            Ok(3)
        };

        let conn2 =
            init_schema_with_migrations(&path, ddl, 3, Some(migrate as MigrationFn)).unwrap();
        let version: String = conn2
            .query_row(
                "SELECT value FROM _meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "3");

        // Verify the migration actually added the column
        let cols: Vec<String> = conn2
            .prepare("PRAGMA table_info(items)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .flatten()
            .collect();
        assert!(cols.contains(&"label".to_string()));
    }

    #[test]
    fn migration_none_falls_back_to_full_ddl() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let ddl = "CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                   INSERT OR IGNORE INTO _meta (key, value) VALUES ('schema_version', '0');
                   CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY);";

        let conn1 = init_schema(&path, ddl, 1).unwrap();
        drop(conn1);

        let conn2 = init_schema_with_migrations(&path, ddl, 2, None).unwrap();
        let version: String = conn2
            .query_row(
                "SELECT value FROM _meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, "2");
    }
}
