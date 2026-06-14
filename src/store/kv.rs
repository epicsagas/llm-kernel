//! Generic key-value store trait and a SQLite implementation.
//!
//! [`KvStore`] is a small, sync, byte-oriented abstraction so that callers
//! (LLM response cache, embedding cache, session state, rate-limit counters)
//! can depend on `Arc<dyn KvStore>` without binding to SQLite. [`SqliteKvStore`]
//! is the bundled implementation over a single guarded connection.

use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::{KernelError, Result};

/// Generic byte-oriented key-value store.
///
/// Sync by design — SQLite is synchronous, and local-disk calls inside an async
/// context are acceptable. The trait is object-safe, so a cache or session
/// store can hold a `Box<dyn KvStore>` / `Arc<dyn KvStore>`.
pub trait KvStore: Send + Sync {
    /// Fetch the value for `key`, or `None` if it is absent.
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    /// Store `value` under `key`, replacing any existing value.
    fn put(&self, key: &str, value: &[u8]) -> Result<()>;
    /// Remove `key`. Returns `true` if a value was present.
    fn delete(&self, key: &str) -> Result<bool>;
}

/// DDL for the single `kv` table. Idempotent.
const KV_DDL: &str = "CREATE TABLE IF NOT EXISTS kv (
    key   TEXT PRIMARY KEY,
    value BLOB NOT NULL
);";

/// SQLite-backed [`KvStore`] over one connection guarded by a mutex.
///
/// The connection is opened with WAL journaling and a 5 s busy timeout for
/// safe concurrent access from multiple threads in the same process.
pub struct SqliteKvStore {
    conn: Mutex<Connection>,
}

impl SqliteKvStore {
    /// Open (or create) a KV store at `path`, applying the schema if needed.
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| KernelError::Store(e.to_string()))?;
        Self::init(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Create an in-memory KV store — useful for tests and ephemeral caches.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| KernelError::Store(e.to_string()))?;
        Self::init(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init(conn: &Connection) -> Result<()> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")
            .map_err(|e| KernelError::Store(e.to_string()))?;
        conn.execute_batch(KV_DDL)
            .map_err(|e| KernelError::Store(e.to_string()))?;
        Ok(())
    }

    /// Recover the connection guard even if a previous holder panicked, so a
    /// poisoned mutex never permanently wedges the store.
    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl KvStore for SqliteKvStore {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.lock();
        match conn.query_row(
            "SELECT value FROM kv WHERE key = ?1",
            rusqlite::params![key],
            |r| r.get::<_, Vec<u8>>(0),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(KernelError::Store(e.to_string())),
        }
    }

    fn put(&self, key: &str, value: &[u8]) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)",
            rusqlite::params![key, value],
        )
        .map_err(|e| KernelError::Store(e.to_string()))?;
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<bool> {
        let conn = self.lock();
        let n = conn
            .execute("DELETE FROM kv WHERE key = ?1", rusqlite::params![key])
            .map_err(|e| KernelError::Store(e.to_string()))?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_in_memory() {
        let kv = SqliteKvStore::open_in_memory().unwrap();
        assert!(kv.get("missing").unwrap().is_none());
        kv.put("a", b"value-a").unwrap();
        assert_eq!(kv.get("a").unwrap(), Some(b"value-a".to_vec()));
        // Overwrite.
        kv.put("a", b"value-a2").unwrap();
        assert_eq!(kv.get("a").unwrap(), Some(b"value-a2".to_vec()));
        // Delete.
        assert!(kv.delete("a").unwrap());
        assert!(kv.get("a").unwrap().is_none());
        // Delete missing returns false.
        assert!(!kv.delete("a").unwrap());
    }

    #[test]
    fn open_on_file_persists() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("kv.db");
        {
            let kv = SqliteKvStore::open(&path).unwrap();
            kv.put("k", b"v").unwrap();
        }
        let kv = SqliteKvStore::open(&path).unwrap();
        assert_eq!(kv.get("k").unwrap(), Some(b"v".to_vec()));
    }

    /// A blanket `dyn KvStore` works (object-safety).
    #[test]
    fn trait_object_round_trip() {
        let kv: Box<dyn KvStore> = Box::new(SqliteKvStore::open_in_memory().unwrap());
        kv.put("x", b"y").unwrap();
        assert_eq!(kv.get("x").unwrap(), Some(b"y".to_vec()));
    }
}
