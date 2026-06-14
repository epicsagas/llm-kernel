//! SQLite initialization helpers.
//!
//! Provides [`init_schema`] for creating a database with WAL mode, busy timeout,
//! and schema versioning built in.

/// Generic key-value store trait and SQLite implementation.
pub mod kv;
/// Schema versioning helpers for SQLite databases.
pub mod schema_version;
/// SQLite initialization, WAL setup, and migration runner.
pub mod sqlite;

pub use kv::{KvStore, SqliteKvStore};
pub use schema_version::SchemaVersion;
pub use sqlite::{MigrationFn, init_in_memory, init_schema, init_schema_with_migrations};
