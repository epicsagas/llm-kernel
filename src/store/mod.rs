//! SQLite initialization helpers.
//!
//! Provides [`init_schema`] for creating a database with WAL mode, busy timeout,
//! and schema versioning built in.

pub mod schema_version;
pub mod sqlite;

pub use schema_version::SchemaVersion;
pub use sqlite::{MigrationFn, init_in_memory, init_schema, init_schema_with_migrations};
