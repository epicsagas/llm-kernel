//! PostgreSQL `GraphBackend` (`graph-pg` feature).
//!
//! `PgGraph` mirrors the bundled SQLite backend over a single mutex-guarded
//! synchronous `postgres::Client`. Every `GraphBackend` method matches the
//! SQLite semantics — the composite `smart_recall` reuses `super::recall`'s
//! weights and `compute_recency` for zero drift across backends. Full-text
//! search uses ILIKE substring matching (**no PostgreSQL extension required**,
//! so the backend runs on any vanilla install), and schema versioning flows
//! through the trait's `current_version` / `migrate`.
//!
//! # Performance note
//!
//! ILIKE with a leading wildcard (`'%term%'`) is a sequential scan — the BTREE
//! indexes cannot serve it. This is intentional: keeping the backend
//! extension-free preserves portability (the same rationale as the CJK feature).
//! For very large graphs, callers may opt into indexed substring search by
//! enabling `pg_trgm` out-of-band (`CREATE EXTENSION pg_trgm;
//! CREATE INDEX nodes_trgm ON nodes USING gin ((title || ' ' || body || ' '
//! || tags) gin_trgm_ops)`); the ILIKE queries then use it transparently.
//! (With a non-empty [`PgGraph`] table prefix, substitute the prefixed table
//! name in that out-of-band DDL.)
//!
//! # TLS (`graph-pg-tls` feature)
//!
//! [`PgGraph::connect`] / [`PgGraph::connect_config`] always use
//! `postgres::NoTls` — servers requiring `sslmode=require` or stricter reject
//! that handshake. Enabling `graph-pg-tls` adds [`PgGraph::connect_native_tls`]
//! (system trust store, one call) plus [`PgGraph::connect_tls`] /
//! [`PgGraph::connect_config_tls`] for a caller-supplied
//! `postgres::tls::MakeTlsConnect` implementor.
//!
//! # Table prefix
//!
//! Every constructor has a `*_with_prefix` variant that namespaces the backing
//! `nodes` / `edges` / `_meta` tables — and every index name — under a
//! caller-chosen prefix, so several graphs (or a graph and unrelated service
//! tables) can coexist in one database. The default empty prefix preserves the
//! original `nodes` / `edges` / `_meta` names exactly: existing databases and
//! tests are unaffected. Because the prefix is interpolated into DDL/DML as a
//! bare identifier (PostgreSQL does not accept bind parameters for
//! identifiers), it is validated by [`is_identifier_safe`] before any SQL is
//! emitted, keeping the interpolation injection-safe.

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use postgres::row::Row;
use postgres::types::ToSql;
use postgres::{Client, Config, NoTls};

use super::algo::{CsrGraph, pagerank_default};
use super::lifecycle::now_iso;
use super::recall::{W_ACCESS, W_FTS, W_GRAPH, W_IMPORTANCE, W_RECENCY, compute_recency};
use super::schema::GRAPH_SCHEMA_VERSION;
use super::types::{EdgeDirection, escape_like, join_csv, split_csv};
use super::{GraphBackend, GraphEdge, GraphNode, ScoredNode};
use crate::error::{KernelError, Result};

/// Standard node SELECT columns (positional order — keep in sync with [`row_to_node`]).
const NODE_COLUMNS: &str = "id, node_type, title, tags, projects, agents, \
     created, updated, body, importance, access_count, accessed_at";

/// Map a `postgres` error into a [`KernelError::Store`].
fn pg_err(e: postgres::Error) -> KernelError {
    KernelError::Store(format!("postgres: {e:?}"))
}

/// Map a `nodes` SELECT row into a [`GraphNode`]. Column order matches [`NODE_COLUMNS`].
fn row_to_node(row: &Row) -> GraphNode {
    GraphNode {
        id: row.get(0),
        node_type: row.get(1),
        title: row.get(2),
        tags: split_csv(&row.get::<_, String>(3)),
        projects: split_csv(&row.get::<_, String>(4)),
        agents: split_csv(&row.get::<_, String>(5)),
        created: row.get(6),
        updated: row.get(7),
        body: row.get(8),
        importance: row.get(9),
        access_count: row.get(10),
        accessed_at: row.get(11),
    }
}

/// Map an `edges` SELECT row into a [`GraphEdge`].
///
/// Column order: `id, source, target, relation, weight, ts`.
fn row_to_edge(row: &Row) -> GraphEdge {
    GraphEdge {
        id: row.get(0),
        source: row.get(1),
        target: row.get(2),
        relation: row.get(3),
        weight: row.get(4),
        ts: row.get(5),
    }
}

/// Build escaped, wrapped ILIKE patterns for each whitespace-separated term in
/// `query` — e.g. `"rust db"` → `["%rust%", "%db%"]`, `"100%"` → `["%100\\%%"]`.
/// Pure (no connection) so the SQL-input transform is unit-testable offline.
fn search_patterns(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|t| format!("%{}%", escape_like(t)))
        .collect()
}

/// Validate a PostgreSQL table-name prefix: empty (default, backward-compatible)
/// or a non-empty run of ASCII alphanumeric / underscore characters. The prefix
/// is interpolated into DDL/DML as a bare identifier (identifiers cannot be
/// passed as bind parameters), so rejecting anything outside this charset is
/// what keeps the interpolation injection-safe. A digit-leading prefix is also
/// rejected because a PostgreSQL identifier may not start with a digit — this
/// guards the combined `{prefix}nodes` / `{prefix}_meta` names against forming
/// an invalid unquoted identifier at runtime.
fn is_identifier_safe(prefix: &str) -> bool {
    let mut chars = prefix.chars();
    match chars.next() {
        None => true,
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        Some(_) => false,
    }
}

/// Create the graph schema if absent. Idempotent — safe on every connect.
/// `prefix` namespaces every table/index name (empty = original names).
fn init_schema(client: &mut Client, prefix: &str) -> Result<()> {
    let nodes = format!("{prefix}nodes");
    let edges = format!("{prefix}edges");
    let meta = format!("{prefix}_meta");
    let idx_edges_source = format!("{prefix}idx_edges_source");
    let idx_edges_target = format!("{prefix}idx_edges_target");
    let idx_edges_src_tgt_rel = format!("{prefix}idx_edges_src_tgt_rel");
    let idx_edges_src_rel = format!("{prefix}idx_edges_src_rel");
    let idx_edges_tgt_rel = format!("{prefix}idx_edges_tgt_rel");
    let idx_nodes_type = format!("{prefix}idx_nodes_type");
    let idx_nodes_updated = format!("{prefix}idx_nodes_updated");
    let idx_nodes_importance = format!("{prefix}idx_nodes_importance");
    let idx_nodes_accessed = format!("{prefix}idx_nodes_accessed");
    let idx_nodes_created = format!("{prefix}idx_nodes_created");
    client
        .batch_execute(&format!(
            "CREATE TABLE IF NOT EXISTS {nodes} (
                id           TEXT PRIMARY KEY,
                node_type    TEXT NOT NULL,
                title        TEXT NOT NULL,
                tags         TEXT NOT NULL DEFAULT '',
                projects     TEXT NOT NULL DEFAULT '',
                agents       TEXT NOT NULL DEFAULT '',
                created      TEXT NOT NULL,
                updated      TEXT NOT NULL,
                body         TEXT NOT NULL DEFAULT '',
                importance   DOUBLE PRECISION NOT NULL DEFAULT 0.5,
                access_count BIGINT NOT NULL DEFAULT 0,
                accessed_at  TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS {edges} (
                id       TEXT PRIMARY KEY,
                source   TEXT NOT NULL,
                target   TEXT NOT NULL,
                relation TEXT NOT NULL DEFAULT 'related',
                weight   DOUBLE PRECISION NOT NULL DEFAULT 1.0,
                ts       TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS {idx_edges_source}  ON {edges}(source);
            CREATE INDEX IF NOT EXISTS {idx_edges_target}  ON {edges}(target);
            CREATE UNIQUE INDEX IF NOT EXISTS {idx_edges_src_tgt_rel} ON {edges}(source, target, relation);
            CREATE INDEX IF NOT EXISTS {idx_edges_src_rel} ON {edges}(source, relation);
            CREATE INDEX IF NOT EXISTS {idx_edges_tgt_rel} ON {edges}(target, relation);
            CREATE INDEX IF NOT EXISTS {idx_nodes_type}       ON {nodes}(node_type);
            CREATE INDEX IF NOT EXISTS {idx_nodes_updated}    ON {nodes}(updated DESC);
            CREATE INDEX IF NOT EXISTS {idx_nodes_importance} ON {nodes}(importance DESC);
            CREATE INDEX IF NOT EXISTS {idx_nodes_accessed}   ON {nodes}(accessed_at DESC);
            CREATE INDEX IF NOT EXISTS {idx_nodes_created}    ON {nodes}(created);
            CREATE TABLE IF NOT EXISTS {meta} (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO {meta} (key, value) VALUES ('graph_schema_version', '3')
                ON CONFLICT (key) DO NOTHING;",
        ))
        .map_err(pg_err)?;
    Ok(())
}

/// Recorded graph schema version from `{prefix}_meta`, or `0` if unset.
fn schema_version(client: &mut Client, prefix: &str) -> Result<u32> {
    let meta = format!("{prefix}_meta");
    let row = client
        .query_opt(
            &format!("SELECT value FROM {meta} WHERE key = 'graph_schema_version'"),
            &[],
        )
        .map_err(pg_err)?;
    Ok(row
        .map(|r| r.get::<_, String>(0))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
}

/// Apply pending migrations up to [`GRAPH_SCHEMA_VERSION`]. No-op when current.
///
/// Runs in a single transaction with rollback on failure — matching the SQLite
/// `migrate_graph` semantics. `prefix` namespaces every table/index name.
fn migrate(client: &mut Client, current: u32, prefix: &str) -> Result<u32> {
    if current >= GRAPH_SCHEMA_VERSION {
        return Ok(current);
    }
    let nodes = format!("{prefix}nodes");
    let edges = format!("{prefix}edges");
    let meta = format!("{prefix}_meta");
    let idx_nodes_created = format!("{prefix}idx_nodes_created");
    let idx_edges_src_rel = format!("{prefix}idx_edges_src_rel");
    let idx_edges_tgt_rel = format!("{prefix}idx_edges_tgt_rel");
    let mut tx = client.transaction().map_err(pg_err)?;
    let mut v = current;
    if v < 2 {
        tx.batch_execute(&format!(
            "CREATE INDEX IF NOT EXISTS {idx_nodes_created} ON {nodes}(created);"
        ))
        .map_err(pg_err)?;
        v = 2;
    }
    // v2 -> v3: composite indexes for relation-filtered directed edge lookups.
    if v < 3 {
        tx.batch_execute(&format!(
            "CREATE INDEX IF NOT EXISTS {idx_edges_src_rel} ON {edges}(source, relation);
             CREATE INDEX IF NOT EXISTS {idx_edges_tgt_rel} ON {edges}(target, relation);",
        ))
        .map_err(pg_err)?;
        v = 3;
    }
    tx.execute(
        &format!("UPDATE {meta} SET value = $1 WHERE key = 'graph_schema_version'"),
        &[&v.to_string()],
    )
    .map_err(pg_err)?;
    tx.commit().map_err(pg_err)?;
    Ok(v)
}

/// PostgreSQL-backed `GraphBackend` over one mutex-guarded connection.
///
/// Opening applies the schema and runs pending migrations, matching
/// `SqliteGraph::open` in the main crate. An optional `table_prefix`
/// (empty by default) namespaces every backing table and index so multiple
/// graphs can share one database — see the `*_with_prefix` constructors.
pub struct PgGraph {
    client: Mutex<Client>,
    table_prefix: String,
}

impl PgGraph {
    /// Connect to `url` (libpq connstring or `postgresql://` URL), apply schema
    /// and migrations, and return a ready backend. Uses the default (empty)
    /// table prefix.
    pub fn connect(url: &str) -> Result<Self> {
        Self::connect_config(&Self::parse_config(url)?)
    }

    /// Like [`connect`](Self::connect), but every table/index is namespaced
    /// under `prefix` (e.g. `"lk_"` → `lk_nodes` / `lk_edges` / `lk_meta`).
    /// The prefix is validated by [`is_identifier_safe`] before any SQL runs.
    pub fn connect_with_prefix(url: &str, prefix: &str) -> Result<Self> {
        Self::connect_config_with_prefix(&Self::parse_config(url)?, prefix)
    }

    /// Connect from a pre-built [`Config`] (useful for overriding `dbname`,
    /// e.g. when targeting a throwaway test database). Uses the default
    /// (empty) table prefix.
    pub fn connect_config(config: &Config) -> Result<Self> {
        let client = config.connect(NoTls).map_err(pg_err)?;
        Self::from_client(client)
    }

    /// Like [`connect_config`](Self::connect_config), but every table/index is
    /// namespaced under `prefix`.
    pub fn connect_config_with_prefix(config: &Config, prefix: &str) -> Result<Self> {
        let client = config.connect(NoTls).map_err(pg_err)?;
        Self::from_client_with_prefix(client, prefix)
    }

    /// Connect to `url` using a caller-supplied TLS connector — for servers
    /// requiring `sslmode=require` or stricter (e.g. RDS with
    /// `rds.force_ssl`). See [`Self::connect_native_tls`] for the common case
    /// of a system-trust-store `native-tls` connector. Uses the default
    /// (empty) table prefix.
    #[cfg(feature = "graph-pg-tls")]
    pub fn connect_tls<T>(url: &str, connector: T) -> Result<Self>
    where
        T: postgres::tls::MakeTlsConnect<postgres::Socket> + Send + 'static,
        T::TlsConnect: Send,
        T::Stream: Send,
        <T::TlsConnect as postgres::tls::TlsConnect<postgres::Socket>>::Future: Send,
    {
        Self::connect_config_tls(&Self::parse_config(url)?, connector)
    }

    /// TLS variant of [`connect_with_prefix`](Self::connect_with_prefix) with a
    /// caller-supplied TLS connector.
    #[cfg(feature = "graph-pg-tls")]
    pub fn connect_tls_with_prefix<T>(url: &str, prefix: &str, connector: T) -> Result<Self>
    where
        T: postgres::tls::MakeTlsConnect<postgres::Socket> + Send + 'static,
        T::TlsConnect: Send,
        T::Stream: Send,
        <T::TlsConnect as postgres::tls::TlsConnect<postgres::Socket>>::Future: Send,
    {
        Self::connect_config_tls_with_prefix(&Self::parse_config(url)?, prefix, connector)
    }

    /// Connect from a pre-built [`Config`] using a caller-supplied TLS
    /// connector. Mirrors [`Self::connect_config`] but negotiates TLS instead
    /// of `postgres::NoTls`. Uses the default (empty) table prefix.
    #[cfg(feature = "graph-pg-tls")]
    pub fn connect_config_tls<T>(config: &Config, connector: T) -> Result<Self>
    where
        T: postgres::tls::MakeTlsConnect<postgres::Socket> + Send + 'static,
        T::TlsConnect: Send,
        T::Stream: Send,
        <T::TlsConnect as postgres::tls::TlsConnect<postgres::Socket>>::Future: Send,
    {
        let client = config.connect(connector).map_err(pg_err)?;
        Self::from_client(client)
    }

    /// TLS variant of [`connect_config_with_prefix`](Self::connect_config_with_prefix)
    /// with a caller-supplied TLS connector.
    #[cfg(feature = "graph-pg-tls")]
    pub fn connect_config_tls_with_prefix<T>(
        config: &Config,
        prefix: &str,
        connector: T,
    ) -> Result<Self>
    where
        T: postgres::tls::MakeTlsConnect<postgres::Socket> + Send + 'static,
        T::TlsConnect: Send,
        T::Stream: Send,
        <T::TlsConnect as postgres::tls::TlsConnect<postgres::Socket>>::Future: Send,
    {
        let client = config.connect(connector).map_err(pg_err)?;
        Self::from_client_with_prefix(client, prefix)
    }

    /// Connect to `url` over TLS using `native-tls` with the system trust
    /// store (default settings — full certificate chain *and* hostname
    /// verification against the system trust store, not weakened) — covers
    /// the common case of a Postgres server with a publicly-trusted
    /// certificate (e.g. RDS `sslmode=require`). For custom CA bundles or
    /// client certificates, build a connector and call [`Self::connect_tls`]
    /// directly. Uses the default (empty) table prefix.
    #[cfg(feature = "graph-pg-tls")]
    pub fn connect_native_tls(url: &str) -> Result<Self> {
        let tls = native_tls::TlsConnector::new()
            .map_err(|e| KernelError::Store(format!("native-tls connector: {e}")))?;
        Self::connect_tls(url, postgres_native_tls::MakeTlsConnector::new(tls))
    }

    /// TLS variant of [`connect_with_prefix`](Self::connect_with_prefix) using
    /// `native-tls` with the system trust store.
    #[cfg(feature = "graph-pg-tls")]
    pub fn connect_native_tls_with_prefix(url: &str, prefix: &str) -> Result<Self> {
        let tls = native_tls::TlsConnector::new()
            .map_err(|e| KernelError::Store(format!("native-tls connector: {e}")))?;
        Self::connect_tls_with_prefix(url, prefix, postgres_native_tls::MakeTlsConnector::new(tls))
    }

    /// Parse a libpq connstring or `postgresql://` URL into a [`Config`],
    /// shared by every `connect*(url, ..)` constructor.
    fn parse_config(url: &str) -> Result<Config> {
        url.parse()
            .map_err(|e| KernelError::Store(format!("invalid postgres config: {e}")))
    }

    /// Shared post-connect setup (schema + migrations) for every constructor.
    ///
    /// Public so a consumer that already owns a synchronous `postgres::Client`
    /// can adopt `PgGraph` without re-opening the connection. For an *async*
    /// pool (e.g. `sqlx::PgPool`), use the planned `SqlxPgGraph` backend instead.
    /// Uses the default (empty) table prefix.
    pub fn from_client(client: Client) -> Result<Self> {
        Self::from_client_with_prefix(client, "")
    }

    /// Like [`from_client`](Self::from_client), but every table/index is
    /// namespaced under `prefix`. The prefix is validated first; an unsafe
    /// value (anything beyond ASCII alphanumeric / underscore, or a digit-led
    /// name) returns [`KernelError::Store`] before any SQL is emitted.
    pub fn from_client_with_prefix(mut client: Client, prefix: &str) -> Result<Self> {
        if !is_identifier_safe(prefix) {
            return Err(KernelError::Store(format!(
                "invalid table prefix {prefix:?}: only ASCII letters, digits, and underscore are allowed (and the first character must not be a digit)"
            )));
        }
        init_schema(&mut client, prefix)?;
        let current = schema_version(&mut client, prefix)?;
        migrate(&mut client, current, prefix)?;
        Ok(Self {
            client: Mutex::new(client),
            table_prefix: prefix.to_string(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Client> {
        self.client.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Fully-qualified `nodes` table name for this backend's prefix.
    fn nodes_tbl(&self) -> String {
        format!("{}nodes", self.table_prefix)
    }

    /// Fully-qualified `edges` table name for this backend's prefix.
    fn edges_tbl(&self) -> String {
        format!("{}edges", self.table_prefix)
    }

    /// Fully-qualified `_meta` table name for this backend's prefix.
    fn meta_tbl(&self) -> String {
        format!("{}_meta", self.table_prefix)
    }

    /// List up to `limit` nodes (uncapped — unlike `GraphBackend::query_nodes`,
    /// which is capped at 200). Used by the migration CLI to enumerate a source
    /// backend of arbitrary size.
    pub fn list_nodes(&self, limit: usize) -> Result<Vec<GraphNode>> {
        let nodes = self.nodes_tbl();
        let mut c = self.lock();
        let sql = format!("SELECT {NODE_COLUMNS} FROM {nodes} ORDER BY updated DESC LIMIT {limit}");
        let rows = c.query(&sql, &[]).map_err(pg_err)?;
        Ok(rows.iter().map(row_to_node).collect())
    }

    /// List up to `limit` edges (uncapped).
    pub fn list_edges(&self, limit: usize) -> Result<Vec<GraphEdge>> {
        let edges = self.edges_tbl();
        let mut c = self.lock();
        let rows = c
            .query(
                &format!("SELECT id, source, target, relation, weight, ts FROM {edges} LIMIT $1"),
                &[&(limit as i64)],
            )
            .map_err(pg_err)?;
        Ok(rows.iter().map(row_to_edge).collect())
    }
}

impl GraphBackend for PgGraph {
    fn upsert_node(&self, node: &GraphNode) -> Result<()> {
        let tags = join_csv(&node.tags);
        let projects = join_csv(&node.projects);
        let agents = join_csv(&node.agents);
        let params: [&(dyn ToSql + Sync); 12] = [
            &node.id,
            &node.node_type,
            &node.title,
            &tags,
            &projects,
            &agents,
            &node.created,
            &node.updated,
            &node.body,
            &node.importance,
            &node.access_count,
            &node.accessed_at,
        ];
        let nodes = self.nodes_tbl();
        let mut c = self.lock();
        c.execute(
            &format!(
                "INSERT INTO {nodes} (id, node_type, title, tags, projects, agents, created, updated, body, importance, access_count, accessed_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
             ON CONFLICT (id) DO UPDATE SET
               node_type=EXCLUDED.node_type, title=EXCLUDED.title, tags=EXCLUDED.tags,
               projects=EXCLUDED.projects, agents=EXCLUDED.agents, created=EXCLUDED.created,
               updated=EXCLUDED.updated, body=EXCLUDED.body, importance=EXCLUDED.importance,
               access_count=EXCLUDED.access_count, accessed_at=EXCLUDED.accessed_at"
            ),
            &params,
        )
        .map_err(pg_err)?;
        Ok(())
    }

    fn read_node(&self, id: &str) -> Result<Option<GraphNode>> {
        let nodes = self.nodes_tbl();
        let mut c = self.lock();
        let sql = format!("SELECT {NODE_COLUMNS} FROM {nodes} WHERE id = $1");
        let params: [&(dyn ToSql + Sync); 1] = [&id];
        let row = c.query_opt(&sql, &params).map_err(pg_err)?;
        Ok(row.as_ref().map(row_to_node))
    }

    fn delete_node(&self, id: &str) -> Result<bool> {
        let nodes = self.nodes_tbl();
        let mut c = self.lock();
        let n = c
            .execute(&format!("DELETE FROM {nodes} WHERE id = $1"), &[&id])
            .map_err(pg_err)?;
        Ok(n > 0)
    }

    fn search_nodes(&self, query: &str, limit: usize) -> Result<Vec<GraphNode>> {
        let terms = search_patterns(query);
        if terms.is_empty() {
            return Ok(vec![]);
        }
        let mut conds: Vec<String> = Vec::with_capacity(terms.len());
        let params: Vec<&(dyn ToSql + Sync)> =
            terms.iter().map(|s| -> &(dyn ToSql + Sync) { s }).collect();
        for i in 0..terms.len() {
            conds.push(format!(
                "(title || ' ' || body || ' ' || tags) ILIKE ${n} ESCAPE '\\'",
                n = i + 1
            ));
        }
        let where_clause = conds.join(" AND ");
        let nodes = self.nodes_tbl();
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM {nodes} WHERE {where_clause} ORDER BY importance DESC, updated DESC LIMIT {limit}"
        );
        let mut c = self.lock();
        let rows = c.query(&sql, &params).map_err(pg_err)?;
        Ok(rows.iter().map(row_to_node).collect())
    }

    fn query_nodes(
        &self,
        tag: Option<&str>,
        node_type: Option<&str>,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphNode>> {
        let limit = limit.min(200) as i64;
        let mut owned: Vec<String> = Vec::new();
        let mut conds: Vec<String> = Vec::new();
        if let Some(t) = tag {
            owned.push(escape_like(t));
            conds.push(format!(
                "(',' || tags || ',') ILIKE ('%,' || ${n} || ',%') ESCAPE '\\'",
                n = owned.len()
            ));
        }
        if let Some(nt) = node_type {
            owned.push(nt.to_string());
            conds.push(format!("node_type = ${n}", n = owned.len()));
        }
        if let Some(p) = project {
            owned.push(escape_like(p));
            conds.push(format!(
                "(',' || projects || ',') ILIKE ('%,' || ${n} || ',%') ESCAPE '\\'",
                n = owned.len()
            ));
        }
        let where_clause = if conds.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conds.join(" AND "))
        };
        let params: Vec<&(dyn ToSql + Sync)> =
            owned.iter().map(|s| -> &(dyn ToSql + Sync) { s }).collect();
        let nodes = self.nodes_tbl();
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM {nodes} {where_clause} ORDER BY updated DESC LIMIT {limit}"
        );
        let mut c = self.lock();
        let rows = c.query(&sql, &params).map_err(pg_err)?;
        Ok(rows.iter().map(row_to_node).collect())
    }

    fn smart_recall(
        &self,
        project: Option<&str>,
        hint: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ScoredNode>> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // FTS match set (ILIKE), used as a binary boost signal.
        let fts_ids: HashSet<String> = match hint {
            Some(h) if !h.is_empty() => self
                .search_nodes(h, limit * 4)?
                .into_iter()
                .map(|n| n.id)
                .collect(),
            _ => HashSet::new(),
        };

        // Candidate fetch (broad set), excluding stale nodes.
        let candidate_limit = (limit * 4).max(40) as i64;
        let mut owned: Vec<String> = Vec::new();
        let mut conds: Vec<String> = vec!["(',' || tags || ',') NOT ILIKE '%,stale,%'".to_string()];
        if let Some(p) = project {
            owned.push(escape_like(p));
            conds.push(format!(
                "(',' || projects || ',') ILIKE ('%,' || ${n} || ',%') ESCAPE '\\'",
                n = owned.len()
            ));
        }
        let where_clause = conds.join(" AND ");
        let params: Vec<&(dyn ToSql + Sync)> =
            owned.iter().map(|s| -> &(dyn ToSql + Sync) { s }).collect();
        let nodes = self.nodes_tbl();
        let edges = self.edges_tbl();
        let sql = format!(
            "SELECT {NODE_COLUMNS} FROM {nodes} WHERE {where_clause} ORDER BY importance DESC, updated DESC LIMIT {candidate_limit}"
        );
        let mut c = self.lock();
        let rows = c.query(&sql, &params).map_err(pg_err)?;
        let candidates: Vec<GraphNode> = rows.iter().map(row_to_node).collect();

        // Composite scoring — identical weights/recency as the SQLite backend.
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

        // Graph-boost pass: PageRank centrality over the induced subgraph of
        // the top candidates. Shares the pagerank math with the SQLite recall
        // path (zero drift) — only the edge-load SQL differs per backend.
        if scored.len() > 1 {
            const MAX_GRAPH_BOOST_PARTICIPANTS: usize = 100;
            let candidate_ids: Vec<String> = scored
                .iter()
                .take(MAX_GRAPH_BOOST_PARTICIPANTS)
                .map(|sn| sn.node.id.clone())
                .collect();
            let n = candidate_ids.len();
            let l1: String = (1..=n)
                .map(|i| format!("${i}"))
                .collect::<Vec<_>>()
                .join(",");
            let l2: String = ((n + 1)..=(2 * n))
                .map(|i| format!("${i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT id, source, target, relation, weight, ts FROM {edges} WHERE source IN ({l1}) AND target IN ({l2})"
            );
            let mut bp: Vec<&(dyn ToSql + Sync)> = Vec::with_capacity(2 * n);
            for id in &candidate_ids {
                bp.push(id);
            }
            for id in &candidate_ids {
                bp.push(id);
            }
            let sub_edges: Vec<GraphEdge> = match c.query(&sql, &bp) {
                Ok(rows) => rows
                    .iter()
                    .map(|r| GraphEdge {
                        id: r.get(0),
                        source: r.get(1),
                        target: r.get(2),
                        relation: r.get(3),
                        weight: r.get(4),
                        ts: r.get(5),
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            let csr = CsrGraph::from_edges(&candidate_ids, &sub_edges);
            let pr = pagerank_default(&csr);
            let max_pr = pr.iter().copied().fold(0.0_f64, f64::max).max(1e-12);
            let pr_map: std::collections::HashMap<String, f64> = candidate_ids
                .iter()
                .zip(pr.iter())
                .map(|(id, &s)| (id.clone(), s / max_pr))
                .collect();
            for sn in &mut scored {
                let boost = pr_map.get(&sn.node.id).copied().unwrap_or(0.0);
                sn.score += W_GRAPH * boost;
            }
            scored.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // Touch retrieved nodes in a single statement (access_count++,
        // accessed_at = now) rather than N round-trips.
        if !scored.is_empty() {
            let now = now_iso();
            let ids: Vec<&str> = scored.iter().map(|sn| sn.node.id.as_str()).collect();
            let placeholders: String = (0..ids.len())
                .map(|i| format!("${}", i + 2))
                .collect::<Vec<_>>()
                .join(",");
            let mut params: Vec<&(dyn ToSql + Sync)> = Vec::with_capacity(ids.len() + 1);
            params.push(&now);
            for id in &ids {
                params.push(id);
            }
            let sql = format!(
                "UPDATE {nodes} SET access_count = access_count + 1, accessed_at = $1 WHERE id IN ({placeholders})"
            );
            let _ = c.execute(&sql, &params);
        }

        Ok(scored)
    }

    fn related_nodes(&self, start_id: &str, depth: usize) -> Result<Vec<String>> {
        let edges = self.edges_tbl();
        let mut c = self.lock();
        let depth_v = depth as i32;
        let params: [&(dyn ToSql + Sync); 2] = [&start_id, &depth_v];
        let rows = c
            .query(
                // PostgreSQL requires a single recursive term: the bidirectional
                // seed is folded into a subquery, then one recursive step follows
                // edges in either direction (CASE picks the opposite endpoint).
                &format!(
                    "WITH RECURSIVE bfs(node_id, lvl) AS (
                    SELECT nb.node_id, 1 FROM (
                        SELECT target AS node_id FROM {edges} WHERE source = $1
                        UNION
                        SELECT source AS node_id FROM {edges} WHERE target = $1
                    ) nb
                    UNION
                    SELECT CASE WHEN e.source = bfs.node_id THEN e.target ELSE e.source END,
                           bfs.lvl + 1
                    FROM bfs
                    JOIN {edges} e ON e.source = bfs.node_id OR e.target = bfs.node_id
                    WHERE bfs.lvl < $2
                )
                SELECT DISTINCT node_id FROM bfs WHERE node_id <> $1 LIMIT 500"
                ),
                &params,
            )
            .map_err(pg_err)?;
        Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
    }

    fn append_edge(&self, edge: &GraphEdge) -> Result<()> {
        let params: [&(dyn ToSql + Sync); 6] = [
            &edge.id,
            &edge.source,
            &edge.target,
            &edge.relation,
            &edge.weight,
            &edge.ts,
        ];
        let edges = self.edges_tbl();
        let mut c = self.lock();
        c.execute(
            &format!(
                "INSERT INTO {edges} (id, source, target, relation, weight, ts)
             VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING"
            ),
            &params,
        )
        .map_err(pg_err)?;
        Ok(())
    }

    fn append_edges(&self, edges: &[GraphEdge]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let edges_tbl = self.edges_tbl();
        const CHUNK: usize = 5000;
        let mut c = self.lock();
        for chunk in edges.chunks(CHUNK) {
            // Each chunk is its own transaction — bounds WAL growth and keeps a
            // partial index build recoverable. `ON CONFLICT DO NOTHING` preserves
            // the per-row idempotency of `append_edge`.
            let mut tx = c.transaction().map_err(pg_err)?;
            {
                let stmt = tx
                    .prepare(&format!(
                        "INSERT INTO {edges_tbl} (id, source, target, relation, weight, ts)
                         VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING"
                    ))
                    .map_err(pg_err)?;
                for e in chunk {
                    let params: [&(dyn ToSql + Sync); 6] =
                        [&e.id, &e.source, &e.target, &e.relation, &e.weight, &e.ts];
                    tx.execute(&stmt, &params).map_err(pg_err)?;
                }
            }
            tx.commit().map_err(pg_err)?;
        }
        Ok(())
    }

    fn edges_for_node_dir(
        &self,
        node_id: &str,
        dir: EdgeDirection,
        relation: Option<&str>,
    ) -> Result<Vec<GraphEdge>> {
        let edges = self.edges_tbl();
        let mut c = self.lock();
        let dir_clause = match dir {
            EdgeDirection::Out => "source = $1",
            EdgeDirection::In => "target = $1",
            EdgeDirection::Both => "(source = $1 OR target = $1)",
        };
        let rows = if let Some(r) = relation {
            let sql = format!(
                "SELECT id, source, target, relation, weight, ts FROM {edges} \
                 WHERE {dir_clause} AND relation = $2 ORDER BY weight DESC"
            );
            c.query(&sql, &[&node_id, &r]).map_err(pg_err)?
        } else {
            let sql = format!(
                "SELECT id, source, target, relation, weight, ts FROM {edges} \
                 WHERE {dir_clause} ORDER BY weight DESC"
            );
            c.query(&sql, &[&node_id]).map_err(pg_err)?
        };
        Ok(rows.iter().map(row_to_edge).collect())
    }

    fn neighbors_weighted(
        &self,
        seed_ids: &[String],
        dir: EdgeDirection,
        relation: Option<&str>,
    ) -> Result<Vec<(String, f64)>> {
        if seed_ids.is_empty() {
            return Ok(vec![]);
        }
        let edges = self.edges_tbl();
        const MAX_SEEDS: usize = 100;
        let seed_ids = if seed_ids.len() > MAX_SEEDS {
            &seed_ids[..MAX_SEEDS]
        } else {
            seed_ids
        };
        let seed_arr: Vec<String> = seed_ids.to_vec();
        let seed_set: HashSet<&str> = seed_ids.iter().map(String::as_str).collect();
        let mut c = self.lock();
        let mut weights: std::collections::HashMap<String, f64> = std::collections::HashMap::new();

        // Walk each directional half (source side, target side, or both).
        let halves: &[&str] = match dir {
            EdgeDirection::Out => &["source"],
            EdgeDirection::In => &["target"],
            EdgeDirection::Both => &["source", "target"],
        };
        for &follow in halves {
            // `follow` is the column the seed matches; the neighbor is the
            // opposite endpoint.
            let select_col = if follow == "source" {
                "target"
            } else {
                "source"
            };
            let rel_clause = relation.map(|_| " AND relation = $2").unwrap_or("");
            let sql = format!(
                "SELECT {select_col} AS nb, SUM(weight) AS w FROM {edges} \
                 WHERE {follow} = ANY($1){rel_clause} GROUP BY {select_col}"
            );
            let rows = if let Some(r) = relation {
                c.query(&sql, &[&seed_arr, &r]).map_err(pg_err)?
            } else {
                c.query(&sql, &[&seed_arr]).map_err(pg_err)?
            };
            for row in &rows {
                let nb: String = row.get(0);
                let w: f64 = row.get(1);
                if !seed_set.contains(nb.as_str()) {
                    *weights.entry(nb).or_default() += w;
                }
            }
        }

        let mut result: Vec<(String, f64)> = weights.into_iter().collect();
        result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(result)
    }

    fn edges_for_node(&self, node_id: &str) -> Result<Vec<GraphEdge>> {
        let edges = self.edges_tbl();
        let mut c = self.lock();
        let params: [&(dyn ToSql + Sync); 1] = [&node_id];
        let rows = c
            .query(
                &format!(
                    "SELECT id, source, target, relation, weight, ts FROM {edges} WHERE source = $1 OR target = $1"
                ),
                &params,
            )
            .map_err(pg_err)?;
        Ok(rows.iter().map(row_to_edge).collect())
    }

    fn delete_edge(&self, id: &str) -> Result<bool> {
        let edges = self.edges_tbl();
        let mut c = self.lock();
        let params: [&(dyn ToSql + Sync); 1] = [&id];
        let n = c
            .execute(&format!("DELETE FROM {edges} WHERE id = $1"), &params)
            .map_err(pg_err)?;
        Ok(n > 0)
    }

    fn remove_edges_for_node(&self, node_id: &str) -> Result<()> {
        let edges = self.edges_tbl();
        let mut c = self.lock();
        let params: [&(dyn ToSql + Sync); 1] = [&node_id];
        c.execute(
            &format!("DELETE FROM {edges} WHERE source = $1 OR target = $1"),
            &params,
        )
        .map_err(pg_err)?;
        Ok(())
    }

    fn current_version(&self) -> Result<u32> {
        let meta = self.meta_tbl();
        let mut c = self.lock();
        let row = c
            .query_opt(
                &format!("SELECT value FROM {meta} WHERE key = 'graph_schema_version'"),
                &[],
            )
            .map_err(pg_err)?;
        Ok(row
            .map(|r| r.get::<_, String>(0))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    }

    fn migrate(&self) -> Result<u32> {
        let mut c = self.lock();
        let current = schema_version(&mut c, &self.table_prefix)?;
        migrate(&mut c, current, &self.table_prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{GraphBackend, SqliteGraph};
    use postgres::{Config, NoTls};

    const TEST_DB: &str = "llm_kernel_pg_test";

    fn sample_node(id: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            node_type: "concept".to_string(),
            title: format!("Node {id}"),
            body: "pg backend test body".to_string(),
            tags: vec!["backend".to_string()],
            projects: vec![],
            agents: vec![],
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
            importance: 0.5,
            access_count: 0,
            accessed_at: String::new(),
        }
    }

    /// Bring up a throwaway DB, run `body` against a fresh `PgGraph`, then tear it down.
    fn with_test_db<F: FnOnce(&PgGraph)>(body: F) {
        let base = std::env::var("LLMKERNEL_PG_URL").expect("LLMKERNEL_PG_URL set");
        let admin_cfg: Config = base
            .parse()
            .expect("LLMKERNEL_PG_URL is a valid libpq connstring");
        {
            let mut admin = admin_cfg.connect(NoTls).expect("admin connect");
            let _ = admin.batch_execute(&format!("DROP DATABASE IF EXISTS {TEST_DB}"));
            admin
                .batch_execute(&format!("CREATE DATABASE {TEST_DB}"))
                .expect("create test db");
        }
        let mut test_cfg = admin_cfg.clone();
        test_cfg.dbname(TEST_DB);
        let graph = PgGraph::connect_config(&test_cfg).expect("connect to test db");
        body(&graph);
        drop(graph);
        let mut admin = admin_cfg.connect(NoTls).expect("admin reconnect");
        let _ = admin.batch_execute(&format!("DROP DATABASE IF EXISTS {TEST_DB}"));
    }

    /// Offline (no server): the ILIKE pattern transform escapes LIKE wildcards
    /// and wraps each term — covers `tests-api-1` SQL-input coverage.
    #[test]
    fn search_patterns_escapes_and_wraps() {
        assert!(search_patterns("").is_empty());
        assert_eq!(search_patterns("rust"), vec!["%rust%".to_string()]);
        // LIKE wildcards (`%`, `_`, `\`) are escaped inside the term.
        assert_eq!(search_patterns("100%"), vec!["%100\\%%".to_string()]);
        assert_eq!(search_patterns("a_b"), vec!["%a\\_b%".to_string()]);
        // Multiple whitespace-separated terms → multiple patterns.
        assert_eq!(
            search_patterns("rust db"),
            vec!["%rust%".to_string(), "%db%".to_string()]
        );
    }

    /// Offline (no server): the prefix validator accepts empty / ASCII
    /// alphanumeric+underscore names (first char not a digit) and rejects
    /// everything else — the guard that keeps prefix interpolation
    /// injection-safe.
    #[test]
    fn is_identifier_safe_validation() {
        // Accepted: empty (default), and ASCII letter/underscore-led names.
        assert!(is_identifier_safe(""));
        assert!(is_identifier_safe("lk_"));
        assert!(is_identifier_safe("graph1"));
        assert!(is_identifier_safe("_x"));
        assert!(is_identifier_safe("ABC_123"));
        // Rejected: a digit-led prefix would form an invalid unquoted identifier.
        assert!(!is_identifier_safe("1lk"));
        // Rejected: whitespace, punctuation, quotes, SQL metacharacters, CJK.
        assert!(!is_identifier_safe("lk nodes"));
        assert!(!is_identifier_safe("lk;"));
        assert!(!is_identifier_safe("lk' OR 1=1--"));
        assert!(!is_identifier_safe("lk-bad"));
        assert!(!is_identifier_safe("lk.bad"));
        assert!(!is_identifier_safe("데이터"));
    }

    /// `connect_native_tls` against a live PostgreSQL configured for TLS
    /// (skips without `LLMKERNEL_PG_URL`). If the server does not offer TLS,
    /// `native-tls` negotiation fails and `connect_native_tls` surfaces that
    /// as an `Err` rather than panicking — asserting only that shape, since
    /// most local/CI Postgres instances run without TLS configured.
    #[cfg(feature = "graph-pg-tls")]
    #[test]
    fn connect_native_tls_returns_result_not_panic() {
        let base = match std::env::var("LLMKERNEL_PG_URL") {
            Ok(u) => u,
            Err(_) => {
                eprintln!("skipped: LLMKERNEL_PG_URL unset (no live PostgreSQL)");
                return;
            }
        };
        match PgGraph::connect_native_tls(&base) {
            Ok(g) => {
                assert_eq!(g.current_version().unwrap(), GRAPH_SCHEMA_VERSION);
            }
            Err(e) => {
                eprintln!("connect_native_tls returned Err as expected on a non-TLS server: {e}");
            }
        }
    }

    /// Full `GraphBackend` conformance against a live PostgreSQL (skips without
    /// `LLMKERNEL_PG_URL`).
    #[test]
    fn live_pg_graph_backend_conformance() {
        if std::env::var("LLMKERNEL_PG_URL").is_err() {
            eprintln!("skipped: LLMKERNEL_PG_URL unset (no live PostgreSQL)");
            return;
        }
        with_test_db(|g| {
            assert_eq!(g.current_version().unwrap(), GRAPH_SCHEMA_VERSION);
            assert_eq!(g.migrate().unwrap(), GRAPH_SCHEMA_VERSION);

            let dyn_g: &dyn GraphBackend = g;
            assert!(dyn_g.read_node("n1").unwrap().is_none());

            g.upsert_node(&sample_node("rust")).unwrap();
            let loaded = g.read_node("rust").unwrap().unwrap();
            assert_eq!(loaded.title, "Node rust");
            assert_eq!(loaded.tags, vec!["backend".to_string()]);

            let mut updated = sample_node("rust");
            updated.title = "Rust ownership".into();
            updated.body = "borrow checker rules".into();
            g.upsert_node(&updated).unwrap();
            assert_eq!(
                g.read_node("rust").unwrap().unwrap().title,
                "Rust ownership"
            );

            assert!(g.delete_node("rust").unwrap());
            assert!(!g.delete_node("rust").unwrap());
            assert!(g.read_node("rust").unwrap().is_none());

            let mut n = sample_node("rust");
            n.title = "Rust ownership model".into();
            n.body = "borrow checker rules".into();
            n.tags = vec!["rust".into(), "memory".into()];
            g.upsert_node(&n).unwrap();
            let mut other = sample_node("py");
            other.title = "Python GIL".into();
            g.upsert_node(&other).unwrap();

            let hits = g.search_nodes("rust", 10).unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].id, "rust");

            let tagged = g.query_nodes(Some("rust"), None, None, 10).unwrap();
            assert_eq!(tagged.len(), 1);
            assert_eq!(tagged[0].id, "rust");

            g.append_edge(&GraphEdge {
                id: "e1".into(),
                source: "rust".into(),
                target: "py".into(),
                relation: "related".into(),
                weight: 1.0,
                ts: "2026-01-01T00:00:00Z".into(),
            })
            .unwrap();
            assert_eq!(g.edges_for_node("rust").unwrap().len(), 1);
            assert!(
                g.related_nodes("rust", 2)
                    .unwrap()
                    .contains(&"py".to_string())
            );

            // correctness-1: a fresh-id edge with a duplicate (source, target,
            // relation) triple is IGNORED (ON CONFLICT DO NOTHING catches any
            // unique violation, matching SQLite's INSERT OR IGNORE) — no hard
            // unique-violation error on PostgreSQL.
            g.append_edge(&GraphEdge {
                id: "e1dup".into(),
                source: "rust".into(),
                target: "py".into(),
                relation: "related".into(),
                weight: 2.0,
                ts: "2026-01-02T00:00:00Z".into(),
            })
            .unwrap();
            assert_eq!(
                g.edges_for_node("rust").unwrap().len(),
                1,
                "duplicate (src,tgt,rel) edge ignored"
            );

            // correctness-2: a self-loop on the start node is excluded from
            // related_nodes (PostgreSQL correctly prunes the start node; the
            // SQLite backend has a pre-existing quirk that leaks it).
            g.append_edge(&GraphEdge {
                id: "eloop".into(),
                source: "rust".into(),
                target: "rust".into(),
                relation: "self".into(),
                weight: 1.0,
                ts: "2026-01-01T00:00:00Z".into(),
            })
            .unwrap();
            let related = g.related_nodes("rust", 2).unwrap();
            assert!(
                !related.contains(&"rust".to_string()),
                "start node excluded even with a self-loop"
            );

            let recalled = g.smart_recall(None, Some("ownership"), 5).unwrap();
            assert!(recalled.iter().any(|s| s.node.id == "rust"));
            let after = g.read_node("rust").unwrap().unwrap();
            assert!(after.access_count >= 1, "access_count incremented");
        });
    }

    /// Same database, two prefixes: writes under one prefix are invisible to
    /// the other because they land in separate `nodes` / `edges` / `_meta`
    /// table sets. Skips without `LLMKERNEL_PG_URL`.
    #[test]
    fn live_prefix_isolation() {
        if std::env::var("LLMKERNEL_PG_URL").is_err() {
            eprintln!("skipped: LLMKERNEL_PG_URL unset (no live PostgreSQL)");
            return;
        }
        with_test_db(|g_default| {
            // Default prefix ("") graph already has its schema; seed it.
            g_default.upsert_node(&sample_node("default_only")).unwrap();
            assert_eq!(
                g_default.read_node("default_only").unwrap().unwrap().title,
                "Node default_only"
            );

            // Open a SECOND backend on the SAME database with prefix "lk_".
            let base = std::env::var("LLMKERNEL_PG_URL").unwrap();
            let admin_cfg: Config = base.parse().expect("valid connstring");
            let mut cfg = admin_cfg.clone();
            cfg.dbname(TEST_DB);
            let g_prefixed =
                PgGraph::connect_config_with_prefix(&cfg, "lk_").expect("prefixed connect");

            // Cross-prefix isolation: neither sees the other's nodes.
            assert!(g_prefixed.read_node("default_only").unwrap().is_none());
            g_prefixed.upsert_node(&sample_node("lk_only")).unwrap();
            assert!(g_default.read_node("lk_only").unwrap().is_none());
            assert_eq!(
                g_prefixed.read_node("lk_only").unwrap().unwrap().title,
                "Node lk_only"
            );

            // Edge isolation across prefixes.
            g_prefixed.upsert_node(&sample_node("lk_peer")).unwrap();
            g_prefixed
                .append_edge(&GraphEdge {
                    id: "lk_e1".into(),
                    source: "lk_only".into(),
                    target: "lk_peer".into(),
                    relation: "related".into(),
                    weight: 1.0,
                    ts: "2026-01-01T00:00:00Z".into(),
                })
                .unwrap();
            assert_eq!(g_prefixed.edges_for_node("lk_only").unwrap().len(), 1);
            assert_eq!(g_default.edges_for_node("lk_only").unwrap().len(), 0);

            // The prefixed graph tracks its own schema version in lk_meta.
            assert!(g_prefixed.current_version().unwrap() >= 2);

            // An invalid prefix is rejected at construction (never reaches SQL).
            assert!(PgGraph::connect_config_with_prefix(&cfg, "lk; drop").is_err());
            assert!(PgGraph::connect_config_with_prefix(&cfg, "1lk").is_err());
        });
    }

    /// SQLite → PostgreSQL migration round-trip through the `GraphBackend` trait
    /// (skips without `LLMKERNEL_PG_URL`).
    #[test]
    fn live_migrate_sqlite_to_postgres_round_trip() {
        let base = match std::env::var("LLMKERNEL_PG_URL") {
            Ok(u) => u,
            Err(_) => {
                eprintln!("skipped: LLMKERNEL_PG_URL unset (no live PostgreSQL)");
                return;
            }
        };

        let src = SqliteGraph::open_in_memory().expect("sqlite source");
        let mut a = sample_node("a");
        a.body = "migrate test body".into();
        a.tags = vec!["migrate".to_string()];
        a.projects = vec!["demo".to_string()];
        a.importance = 0.6;
        src.upsert_node(&a).unwrap();
        src.upsert_node(&sample_node("b")).unwrap();
        src.append_edge(&GraphEdge {
            id: "e1".into(),
            source: "a".into(),
            target: "b".into(),
            relation: "related".into(),
            weight: 1.0,
            ts: "2026-01-01T00:00:00Z".into(),
        })
        .unwrap();
        let nodes = src.query_nodes(None, None, None, 200).unwrap();
        assert_eq!(nodes.len(), 2);

        let admin_cfg: Config = base.parse().expect("valid connstring");
        {
            let mut admin = admin_cfg.connect(NoTls).expect("admin connect");
            let _ = admin.batch_execute("DROP DATABASE IF EXISTS llm_kernel_migrate_test");
            admin
                .batch_execute("CREATE DATABASE llm_kernel_migrate_test")
                .expect("create test db");
        }
        let mut test_cfg = admin_cfg.clone();
        test_cfg.dbname("llm_kernel_migrate_test");
        let pg = PgGraph::connect_config(&test_cfg).expect("connect target");

        for n in &nodes {
            pg.upsert_node(n).unwrap();
        }
        pg.append_edge(&GraphEdge {
            id: "e1".into(),
            source: "a".into(),
            target: "b".into(),
            relation: "related".into(),
            weight: 1.0,
            ts: "2026-01-01T00:00:00Z".into(),
        })
        .unwrap();

        assert_eq!(pg.list_nodes(100).unwrap().len(), 2);
        assert_eq!(pg.list_edges(100).unwrap().len(), 1);
        let loaded = pg.read_node("a").unwrap().unwrap();
        assert_eq!(loaded.title, "Node a");
        assert_eq!(loaded.tags, vec!["migrate".to_string()]);
        assert!((loaded.importance - 0.6).abs() < 1e-9);

        drop(pg);
        let mut admin = admin_cfg.connect(NoTls).expect("admin reconnect");
        let _ = admin.batch_execute("DROP DATABASE IF EXISTS llm_kernel_migrate_test");
    }
}
