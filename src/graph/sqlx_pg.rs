//! Async PostgreSQL graph backend over `sqlx::PgPool` (feature `graph-pg-sqlx`).
//!
//! `SqlxPgGraph` mirrors the SQL/DDL/semantics of the synchronous `PgGraph`
//! (`graph-pg`), but drives every operation asynchronously over a shared
//! `sqlx::PgPool` instead of one mutex-guarded `postgres::Client`. The SQL is
//! ported verbatim from `pg.rs` — only the driver changes (sqlx prepared-statement
//! cache + connection pool instead of blocking `postgres::Client`).
//!
//! # Why a second PostgreSQL backend?
//!
//! Consumers that already own a `sqlx::PgPool` (notably the `klr`
//! citation graph) cannot share a pool or transaction with `PgGraph`, which is
//! built on a synchronous `postgres::Client`. `SqlxPgGraph` lets such consumers
//! adopt the llm-kernel graph on their own pool and — critically — **expose the
//! transaction** so a multi-table prune (chunks + vectors + edges in one tx)
//! stays atomic. See [`SqlxPgGraph::pool`] and the `*_in_tx` methods.
//!
//! This backend is intentionally **non-breaking**: it is a new module that does
//! not touch `GraphBackend`, `PgGraph`, or any other file. The SQL/schema is
//! identical to `PgGraph`, so the two backends can share one database (use a
//! `table_prefix` to namespace tables when both run side by side).
//!
//! # Scope
//!
//! Inherent async methods cover klr's needs — batch edge writes, directed /
//! relation-filtered lookups, weighted neighbor aggregation, search, and
//! traversal — plus basic node/edge CRUD. `query_nodes` and `smart_recall`
//! (which would require porting the CSR PageRank pass) are out of scope: a
//! citation graph does not need memory-recall ranking. They are marked with
//! `TODO(post-1.0)` comments.

use std::collections::{HashMap, HashSet};

use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use super::schema::GRAPH_SCHEMA_VERSION;
use super::types::{EdgeDirection, GraphEdge, GraphNode, escape_like, join_csv, split_csv};
use crate::error::{KernelError, Result};

/// Standard node SELECT columns (positional order — keep in sync with [`row_to_node`]).
const NODE_COLUMNS: &str = "id, node_type, title, tags, projects, agents, \
     created, updated, body, importance, access_count, accessed_at";

/// Map a `sqlx::Error` into a [`KernelError::Store`].
fn pg_err(e: sqlx::Error) -> KernelError {
    KernelError::Store(format!("sqlx: {e:?}"))
}

/// Map a `nodes` SELECT row into a [`GraphNode`]. Column names match [`NODE_COLUMNS`].
fn row_to_node(row: &PgRow) -> GraphNode {
    let tags: String = row.get("tags");
    let projects: String = row.get("projects");
    let agents: String = row.get("agents");
    GraphNode {
        id: row.get("id"),
        node_type: row.get("node_type"),
        title: row.get("title"),
        tags: split_csv(&tags),
        projects: split_csv(&projects),
        agents: split_csv(&agents),
        created: row.get("created"),
        updated: row.get("updated"),
        body: row.get("body"),
        importance: row.get("importance"),
        access_count: row.get("access_count"),
        accessed_at: row.get("accessed_at"),
    }
}

/// Map an `edges` SELECT row into a [`GraphEdge`].
fn row_to_edge(row: &PgRow) -> GraphEdge {
    GraphEdge {
        id: row.get("id"),
        source: row.get("source"),
        target: row.get("target"),
        relation: row.get("relation"),
        weight: row.get("weight"),
        ts: row.get("ts"),
    }
}

/// Build escaped, wrapped ILIKE patterns for each whitespace-separated term in
/// `query` — e.g. `"rust db"` → `["%rust%", "%db%"]`, `"100%"` → `["%100\\%%"]`.
/// Pure (no connection); identical to `pg.rs::search_patterns` — duplicated here
/// because that helper is private and this module must not edit `pg.rs`.
fn search_patterns(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|t| format!("%{}%", escape_like(t)))
        .collect()
}

/// Validate a PostgreSQL table-name prefix: empty (default, backward-compatible)
/// or a non-empty run of ASCII alphanumeric / underscore characters. Identical
/// to `pg.rs::is_identifier_safe` — duplicated here because the `pg` module is
/// gated behind `graph-pg` (not implied by `graph-pg-sqlx`), and pulling in the
/// synchronous `postgres`+`clap` deps for one pure function would bloat klr's
/// async-only dependency tree.
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

/// Async PostgreSQL graph backend over a shared `sqlx::PgPool`.
///
/// Every constructor applies the schema and runs pending migrations on connect,
/// matching `PgGraph`. An optional `table_prefix` namespaces every backing table
/// and index so several graphs can coexist in one database (identical semantics
/// to `PgGraph`'s `*_with_prefix` constructors). The prefix is validated by
/// `is_identifier_safe` before any SQL is emitted, keeping the identifier
/// interpolation injection-safe.
///
/// # Transaction exposure
///
/// [`pool`](Self::pool) plus [`append_edges_in_tx`](Self::append_edges_in_tx) and
/// [`remove_edges_for_node_in_tx`](Self::remove_edges_for_node_in_tx) let a caller
/// coordinate a multi-table transaction (e.g. klr's prune: delete chunks +
/// vectors + edges atomically). Begin a tx on `pool()`, run cross-table DML,
/// call the `*_in_tx` helpers with the same `&mut PgConnection`, then commit or
/// rollback — mirroring `PgVectorIndex::remove_in_tx`.
pub struct SqlxPgGraph {
    pool: PgPool,
    table_prefix: String,
}

impl SqlxPgGraph {
    /// Adopt a caller-owned `PgPool`, apply schema + migrations, and return a
    /// ready backend. Uses the default (empty) table prefix.
    pub async fn from_pool(pool: PgPool) -> Result<Self> {
        Self::from_pool_with_prefix(pool, "").await
    }

    /// Like [`from_pool`](Self::from_pool), but every table/index is namespaced
    /// under `prefix`. The prefix is validated by `is_identifier_safe` before
    /// any SQL runs; an unsafe value returns [`KernelError::Store`].
    pub async fn from_pool_with_prefix(pool: PgPool, prefix: &str) -> Result<Self> {
        if !is_identifier_safe(prefix) {
            return Err(KernelError::Store(format!(
                "invalid table prefix {prefix:?}: only ASCII letters, digits, and underscore are allowed (and the first character must not be a digit)"
            )));
        }
        let graph = Self {
            pool,
            table_prefix: prefix.to_string(),
        };
        graph.init_schema().await?;
        graph.migrate().await?;
        Ok(graph)
    }

    /// Connect to `url` (libpq connstring / `postgresql://…`) with an 8-connection
    /// pool, apply schema + migrations, and return a ready backend. Uses the
    /// default (empty) table prefix.
    pub async fn connect(url: &str) -> Result<Self> {
        Self::connect_with_prefix(url, "").await
    }

    /// Like [`connect`](Self::connect), but every table/index is namespaced
    /// under `prefix`.
    pub async fn connect_with_prefix(url: &str, prefix: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(url)
            .await
            .map_err(pg_err)?;
        Self::from_pool_with_prefix(pool, prefix).await
    }

    /// The underlying connection pool.
    ///
    /// Callers that need cross-table transactional consistency — e.g. pruning a
    /// law's chunks plus their graph edges atomically — can `pool().begin()` and
    /// run their own DML, then call the [`append_edges_in_tx`](Self::append_edges_in_tx)
    /// / [`remove_edges_for_node_in_tx`](Self::remove_edges_for_node_in_tx)
    /// helpers on the same `&mut PgConnection`.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ── table-name helpers (identical to PgGraph) ──────────────────────

    fn nodes_tbl(&self) -> String {
        format!("{}nodes", self.table_prefix)
    }

    fn edges_tbl(&self) -> String {
        format!("{}edges", self.table_prefix)
    }

    fn meta_tbl(&self) -> String {
        format!("{}_meta", self.table_prefix)
    }

    // ── schema init + migrate ──────────────────────────────────────────

    /// Create the graph schema if absent. Idempotent — DDL ported verbatim from
    /// `pg.rs::init_schema` (same tables, same prefixed index names). Each DDL
    /// statement is a separate round trip because sqlx's extended-protocol
    /// `query()` accepts one statement per call; init runs once per connect.
    async fn init_schema(&self) -> Result<()> {
        let p = &self.table_prefix;
        let nodes = self.nodes_tbl();
        let edges = self.edges_tbl();
        let meta = self.meta_tbl();
        let idx = |name: &str| format!("{p}{name}");

        let stmts: Vec<String> = vec![
            format!(
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
                )"
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {edges} (
                    id       TEXT PRIMARY KEY,
                    source   TEXT NOT NULL,
                    target   TEXT NOT NULL,
                    relation TEXT NOT NULL DEFAULT 'related',
                    weight   DOUBLE PRECISION NOT NULL DEFAULT 1.0,
                    ts       TEXT NOT NULL
                )"
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {edges}(source)",
                idx("idx_edges_source")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {edges}(target)",
                idx("idx_edges_target")
            ),
            format!(
                "CREATE UNIQUE INDEX IF NOT EXISTS {} ON {edges}(source, target, relation)",
                idx("idx_edges_src_tgt_rel")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {edges}(source, relation)",
                idx("idx_edges_src_rel")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {edges}(target, relation)",
                idx("idx_edges_tgt_rel")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {nodes}(node_type)",
                idx("idx_nodes_type")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {nodes}(updated DESC)",
                idx("idx_nodes_updated")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {nodes}(importance DESC)",
                idx("idx_nodes_importance")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {nodes}(accessed_at DESC)",
                idx("idx_nodes_accessed")
            ),
            format!(
                "CREATE INDEX IF NOT EXISTS {} ON {nodes}(created)",
                idx("idx_nodes_created")
            ),
            format!(
                "CREATE TABLE IF NOT EXISTS {meta} (key TEXT PRIMARY KEY, value TEXT NOT NULL)"
            ),
            format!(
                "INSERT INTO {meta} (key, value) VALUES ('graph_schema_version', '{}')
                 ON CONFLICT (key) DO NOTHING",
                GRAPH_SCHEMA_VERSION
            ),
        ];

        for ddl in &stmts {
            sqlx::query(ddl).execute(&self.pool).await.map_err(pg_err)?;
        }
        Ok(())
    }

    /// Recorded graph schema version from `{prefix}_meta`, or `0` if unset.
    pub async fn current_version(&self) -> Result<u32> {
        let meta = self.meta_tbl();
        let row = sqlx::query(&format!(
            "SELECT value FROM {meta} WHERE key = 'graph_schema_version'"
        ))
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;
        match row {
            Some(r) => {
                let s: String = r.get("value");
                Ok(s.parse().unwrap_or(0))
            }
            None => Ok(0),
        }
    }

    /// Apply pending migrations up to [`GRAPH_SCHEMA_VERSION`]. No-op when current.
    ///
    /// Runs in a single transaction with rollback on failure — matching
    /// `pg.rs::migrate`. Returns the new version.
    pub async fn migrate(&self) -> Result<u32> {
        let current = self.current_version().await?;
        if current >= GRAPH_SCHEMA_VERSION {
            return Ok(current);
        }
        let p = &self.table_prefix;
        let nodes = self.nodes_tbl();
        let edges = self.edges_tbl();
        let meta = self.meta_tbl();
        let idx = |name: &str| format!("{p}{name}");

        let mut tx = self.pool.begin().await.map_err(pg_err)?;
        let mut v = current;
        // v1 -> v2: index nodes by creation timestamp.
        if v < 2 {
            sqlx::query(&format!(
                "CREATE INDEX IF NOT EXISTS {} ON {nodes}(created)",
                idx("idx_nodes_created")
            ))
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
            v = 2;
        }
        // v2 -> v3: composite indexes for relation-filtered directed edge lookups.
        if v < 3 {
            sqlx::query(&format!(
                "CREATE INDEX IF NOT EXISTS {} ON {edges}(source, relation)",
                idx("idx_edges_src_rel")
            ))
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
            sqlx::query(&format!(
                "CREATE INDEX IF NOT EXISTS {} ON {edges}(target, relation)",
                idx("idx_edges_tgt_rel")
            ))
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
            v = 3;
        }
        sqlx::query(&format!(
            "UPDATE {meta} SET value = $1 WHERE key = 'graph_schema_version'"
        ))
        .bind(v.to_string())
        .execute(&mut *tx)
        .await
        .map_err(pg_err)?;
        tx.commit().await.map_err(pg_err)?;
        Ok(v)
    }

    // ── basic CRUD ─────────────────────────────────────────────────────

    /// Insert or update a node (upsert by `id`). Mirrors `PgGraph::upsert_node`.
    pub async fn upsert_node(&self, node: &GraphNode) -> Result<()> {
        let tags = join_csv(&node.tags);
        let projects = join_csv(&node.projects);
        let agents = join_csv(&node.agents);
        let nodes = self.nodes_tbl();
        sqlx::query(&format!(
            "INSERT INTO {nodes} (id, node_type, title, tags, projects, agents, created, updated, body, importance, access_count, accessed_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
             ON CONFLICT (id) DO UPDATE SET
               node_type=EXCLUDED.node_type, title=EXCLUDED.title, tags=EXCLUDED.tags,
               projects=EXCLUDED.projects, agents=EXCLUDED.agents, created=EXCLUDED.created,
               updated=EXCLUDED.updated, body=EXCLUDED.body, importance=EXCLUDED.importance,
               access_count=EXCLUDED.access_count, accessed_at=EXCLUDED.accessed_at"
        ))
        .bind(node.id.as_str())
        .bind(node.node_type.as_str())
        .bind(node.title.as_str())
        .bind(tags.as_str())
        .bind(projects.as_str())
        .bind(agents.as_str())
        .bind(node.created.as_str())
        .bind(node.updated.as_str())
        .bind(node.body.as_str())
        .bind(node.importance)
        .bind(node.access_count)
        .bind(node.accessed_at.as_str())
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Read a node by `id`, or `None` if absent.
    pub async fn read_node(&self, id: &str) -> Result<Option<GraphNode>> {
        let nodes = self.nodes_tbl();
        let row = sqlx::query(&format!("SELECT {NODE_COLUMNS} FROM {nodes} WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(row.as_ref().map(row_to_node))
    }

    /// Delete a node by `id`. Returns `true` if a row was removed.
    pub async fn delete_node(&self, id: &str) -> Result<bool> {
        let nodes = self.nodes_tbl();
        let res = sqlx::query(&format!("DELETE FROM {nodes} WHERE id = $1"))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(res.rows_affected() > 0)
    }

    /// Append a single edge (`ON CONFLICT DO NOTHING`). Mirrors `PgGraph::append_edge`.
    pub async fn append_edge(&self, edge: &GraphEdge) -> Result<()> {
        let edges = self.edges_tbl();
        sqlx::query(&format!(
            "INSERT INTO {edges} (id, source, target, relation, weight, ts)
             VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING"
        ))
        .bind(edge.id.as_str())
        .bind(edge.source.as_str())
        .bind(edge.target.as_str())
        .bind(edge.relation.as_str())
        .bind(edge.weight)
        .bind(edge.ts.as_str())
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// All edges touching `node_id` (either endpoint). Mirrors `PgGraph::edges_for_node`.
    pub async fn edges_for_node(&self, node_id: &str) -> Result<Vec<GraphEdge>> {
        let edges = self.edges_tbl();
        let rows = sqlx::query(&format!(
            "SELECT id, source, target, relation, weight, ts FROM {edges} \
             WHERE source = $1 OR target = $1"
        ))
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows.iter().map(row_to_edge).collect())
    }

    /// Delete a single edge by `id`. Returns `true` if a row was removed.
    pub async fn delete_edge(&self, id: &str) -> Result<bool> {
        let edges = self.edges_tbl();
        let res = sqlx::query(&format!("DELETE FROM {edges} WHERE id = $1"))
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(res.rows_affected() > 0)
    }

    // ── klr core: batch edges + directed traversal ─────────────────────

    /// Batch-insert edges in chunks of 5000, each chunk its own transaction
    /// (`ON CONFLICT DO NOTHING` per row). Mirrors `PgGraph::append_edges`.
    /// sqlx caches the prepared statement per connection, so the repeated
    /// `INSERT` reuses one parse/plan per chunk.
    pub async fn append_edges(&self, edges: &[GraphEdge]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let edges_tbl = self.edges_tbl();
        let sql = format!(
            "INSERT INTO {edges_tbl} (id, source, target, relation, weight, ts)
             VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING"
        );
        const CHUNK: usize = 5000;
        for chunk in edges.chunks(CHUNK) {
            let mut tx = self.pool.begin().await.map_err(pg_err)?;
            for e in chunk {
                sqlx::query(&sql)
                    .bind(e.id.as_str())
                    .bind(e.source.as_str())
                    .bind(e.target.as_str())
                    .bind(e.relation.as_str())
                    .bind(e.weight)
                    .bind(e.ts.as_str())
                    .execute(&mut *tx)
                    .await
                    .map_err(pg_err)?;
            }
            tx.commit().await.map_err(pg_err)?;
        }
        Ok(())
    }

    /// Append edges **within a caller-provided transaction** (commit is the
    /// caller's responsibility — this method does not commit). Enables atomic
    /// cross-table writes: begin a tx on [`pool`](Self::pool), write chunks /
    /// vectors / other relations, call this with the same `&mut PgConnection`,
    /// then `commit()`. A failure anywhere rolls back the whole set.
    pub async fn append_edges_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        edges: &[GraphEdge],
    ) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let edges_tbl = self.edges_tbl();
        let sql = format!(
            "INSERT INTO {edges_tbl} (id, source, target, relation, weight, ts)
             VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING"
        );
        for e in edges {
            sqlx::query(&sql)
                .bind(e.id.as_str())
                .bind(e.source.as_str())
                .bind(e.target.as_str())
                .bind(e.relation.as_str())
                .bind(e.weight)
                .bind(e.ts.as_str())
                .execute(&mut *tx)
                .await
                .map_err(pg_err)?;
        }
        Ok(())
    }

    /// Directed, optionally relation-filtered edge lookup. Mirrors
    /// `PgGraph::edges_for_node_dir`. `dir` selects out / in / both; `relation`
    /// further restricts to a single relationship type when `Some`.
    pub async fn edges_for_node_dir(
        &self,
        node_id: &str,
        dir: EdgeDirection,
        relation: Option<&str>,
    ) -> Result<Vec<GraphEdge>> {
        let edges = self.edges_tbl();
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
            sqlx::query(&sql)
                .bind(node_id)
                .bind(r)
                .fetch_all(&self.pool)
                .await
                .map_err(pg_err)?
        } else {
            let sql = format!(
                "SELECT id, source, target, relation, weight, ts FROM {edges} \
                 WHERE {dir_clause} ORDER BY weight DESC"
            );
            sqlx::query(&sql)
                .bind(node_id)
                .fetch_all(&self.pool)
                .await
                .map_err(pg_err)?
        };
        Ok(rows.iter().map(row_to_edge).collect())
    }

    /// Aggregate neighbor weights from seed nodes, optionally direction- and
    /// relation-filtered. Mirrors `PgGraph::neighbors_weighted`: walks the
    /// source/target halves, uses `= ANY($1)` for the seed set, `GROUP BY` the
    /// opposite endpoint, and `SUM(weight)`. Seeds themselves are excluded.
    /// Capped at 100 seeds.
    pub async fn neighbors_weighted(
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

        let halves: &[&str] = match dir {
            EdgeDirection::Out => &["source"],
            EdgeDirection::In => &["target"],
            EdgeDirection::Both => &["source", "target"],
        };
        let mut weights: HashMap<String, f64> = HashMap::new();

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
                sqlx::query(&sql)
                    .bind(&seed_arr)
                    .bind(r)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(pg_err)?
            } else {
                sqlx::query(&sql)
                    .bind(&seed_arr)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(pg_err)?
            };
            for row in &rows {
                let nb: String = row.get("nb");
                let w: f64 = row.get("w");
                if !seed_set.contains(nb.as_str()) {
                    *weights.entry(nb).or_default() += w;
                }
            }
        }

        let mut result: Vec<(String, f64)> = weights.into_iter().collect();
        result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(result)
    }

    /// Remove every edge touching `node_id` (either endpoint).
    /// Mirrors `PgGraph::remove_edges_for_node`.
    pub async fn remove_edges_for_node(&self, node_id: &str) -> Result<()> {
        let edges = self.edges_tbl();
        sqlx::query(&format!(
            "DELETE FROM {edges} WHERE source = $1 OR target = $1"
        ))
        .bind(node_id)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    /// Remove edges for `node_id` **within a caller-provided transaction** (no
    /// commit here). Same atomic-prune pattern as [`Self::append_edges_in_tx`].
    pub async fn remove_edges_for_node_in_tx(
        &self,
        tx: &mut sqlx::PgConnection,
        node_id: &str,
    ) -> Result<()> {
        let edges = self.edges_tbl();
        sqlx::query(&format!(
            "DELETE FROM {edges} WHERE source = $1 OR target = $1"
        ))
        .bind(node_id)
        .execute(tx)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    // ── search / traversal ─────────────────────────────────────────────

    /// ILIKE substring search over `(title, body, tags)` for each
    /// whitespace-separated term (AND of terms). Mirrors `PgGraph::search_nodes`
    /// — extension-free vanilla PostgreSQL ILIKE with escaped, wrapped patterns.
    pub async fn search_nodes(&self, query: &str, limit: usize) -> Result<Vec<GraphNode>> {
        let terms = search_patterns(query);
        if terms.is_empty() {
            return Ok(vec![]);
        }
        let nodes = self.nodes_tbl();
        let mut qb = QueryBuilder::<Postgres>::new("SELECT ");
        qb.push(NODE_COLUMNS);
        qb.push(" FROM ");
        qb.push(nodes.as_str());
        qb.push(" WHERE ");
        for (i, term) in terms.iter().enumerate() {
            if i > 0 {
                qb.push(" AND ");
            }
            qb.push("(title || ' ' || body || ' ' || tags) ILIKE ");
            qb.push_bind(term.clone());
            qb.push(" ESCAPE '\\'");
        }
        qb.push(" ORDER BY importance DESC, updated DESC LIMIT ");
        qb.push_bind(limit as i64);
        let rows = qb.build().fetch_all(&self.pool).await.map_err(pg_err)?;
        Ok(rows.iter().map(row_to_node).collect())
    }

    /// Bidirectional BFS up to `depth` hops from `start_id` via a recursive CTE.
    /// Mirrors `PgGraph::related_nodes`. Returns distinct neighbor IDs (the start
    /// node is excluded), capped at 500.
    pub async fn related_nodes(&self, start_id: &str, depth: usize) -> Result<Vec<String>> {
        let edges = self.edges_tbl();
        // PostgreSQL requires a single recursive term: the bidirectional seed
        // is folded into a subquery, then one recursive step follows edges in
        // either direction (CASE picks the opposite endpoint).
        let sql = format!(
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
        );
        let rows = sqlx::query(&sql)
            .bind(start_id)
            .bind(depth as i32)
            .fetch_all(&self.pool)
            .await
            .map_err(pg_err)?;
        Ok(rows
            .iter()
            .map(|r| {
                let id: String = r.get("node_id");
                id
            })
            .collect())
    }

    // TODO(post-1.0): query_nodes / smart_recall — klr citation-graph path does
    // not need them. Porting smart_recall requires the CSR PageRank boost pass
    // (super::algo::{CsrGraph, pagerank_default}) which is a larger surface.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LLMKERNEL_PG_URL` unset → self-skip (pgvector / pg.rs pattern).
    fn pg_url() -> Option<String> {
        std::env::var("LLMKERNEL_PG_URL").ok()
    }

    fn sample_node(id: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            node_type: "concept".to_string(),
            title: format!("Node {id}"),
            body: "sqlx pg test body".to_string(),
            tags: vec!["sqlx".to_string()],
            projects: vec![],
            agents: vec![],
            created: "2026-01-01T00:00:00Z".to_string(),
            updated: "2026-01-01T00:00:00Z".to_string(),
            importance: 0.5,
            access_count: 0,
            accessed_at: String::new(),
        }
    }

    fn sample_edge(id: &str, src: &str, tgt: &str, rel: &str, w: f64) -> GraphEdge {
        GraphEdge {
            id: id.to_string(),
            source: src.to_string(),
            target: tgt.to_string(),
            relation: rel.to_string(),
            weight: w,
            ts: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    /// Drop the prefixed tables so parallel tests with distinct prefixes don't
    /// accumulate state.
    async fn cleanup(pool: &PgPool, prefix: &str) {
        let _ = sqlx::query(&format!("DROP TABLE IF EXISTS {}nodes", prefix))
            .execute(pool)
            .await;
        let _ = sqlx::query(&format!("DROP TABLE IF EXISTS {}edges", prefix))
            .execute(pool)
            .await;
        let _ = sqlx::query(&format!("DROP TABLE IF EXISTS {}_meta", prefix))
            .execute(pool)
            .await;
    }

    // ── offline (no server) ────────────────────────────────────────────

    /// The ILIKE pattern transform is identical to pg.rs (escapes LIKE
    /// wildcards, wraps each term).
    #[test]
    fn search_patterns_escapes_and_wraps() {
        assert!(search_patterns("").is_empty());
        assert_eq!(search_patterns("rust"), vec!["%rust%".to_string()]);
        assert_eq!(search_patterns("100%"), vec!["%100\\%%".to_string()]);
        assert_eq!(search_patterns("a_b"), vec!["%a\\_b%".to_string()]);
        assert_eq!(
            search_patterns("rust db"),
            vec!["%rust%".to_string(), "%db%".to_string()]
        );
    }

    // ── live (LLMKERNEL_PG_URL gated, throwaway prefixed tables) ───────

    /// Basic CRUD round-trip: upsert/read/update/delete nodes + edges.
    #[tokio::test]
    async fn basic_crud_roundtrip() {
        let Some(url) = pg_url() else {
            eprintln!("skip: LLMKERNEL_PG_URL unset");
            return;
        };
        let prefix = "lk_sg1_";
        let g = SqlxPgGraph::connect_with_prefix(&url, prefix)
            .await
            .expect("connect");

        assert!(g.read_node("n1").await.unwrap().is_none());
        g.upsert_node(&sample_node("n1")).await.unwrap();
        let loaded = g.read_node("n1").await.unwrap().unwrap();
        assert_eq!(loaded.title, "Node n1");
        assert_eq!(loaded.tags, vec!["sqlx".to_string()]);

        // upsert = update on conflict
        let mut updated = sample_node("n1");
        updated.title = "Updated".into();
        g.upsert_node(&updated).await.unwrap();
        assert_eq!(g.read_node("n1").await.unwrap().unwrap().title, "Updated");

        // single edge append + edges_for_node + delete_edge
        g.upsert_node(&sample_node("n2")).await.unwrap();
        g.append_edge(&sample_edge("e1", "n1", "n2", "related", 1.0))
            .await
            .unwrap();
        assert_eq!(g.edges_for_node("n1").await.unwrap().len(), 1);
        assert!(g.delete_edge("e1").await.unwrap());
        assert!(!g.delete_edge("e1").await.unwrap());
        assert_eq!(g.edges_for_node("n1").await.unwrap().len(), 0);

        // delete node
        assert!(g.delete_node("n1").await.unwrap());
        assert!(!g.delete_node("n1").await.unwrap());
        assert!(g.read_node("n1").await.unwrap().is_none());

        cleanup(g.pool(), prefix).await;
    }

    /// Batch edges with `ON CONFLICT DO NOTHING` dedup: a fresh-id edge sharing
    /// a duplicate (source, target, relation) triple is silently ignored.
    #[tokio::test]
    async fn batch_edges_dedup() {
        let Some(url) = pg_url() else {
            eprintln!("skip: LLMKERNEL_PG_URL unset");
            return;
        };
        let prefix = "lk_sg2_";
        let g = SqlxPgGraph::connect_with_prefix(&url, prefix)
            .await
            .expect("connect");

        for n in &["a", "b", "c"] {
            g.upsert_node(&sample_node(n)).await.unwrap();
        }

        let edges = vec![
            sample_edge("e1", "a", "b", "cites", 1.0),
            sample_edge("e2", "a", "c", "cites", 0.8),
            // duplicate (src,tgt,rel) of e1 — ON CONFLICT DO NOTHING
            sample_edge("e1dup", "a", "b", "cites", 2.0),
        ];
        g.append_edges(&edges).await.unwrap();

        // Only e1 and e2 survived (e1dup deduped by the unique index).
        let out = g
            .edges_for_node_dir("a", EdgeDirection::Out, Some("cites"))
            .await
            .unwrap();
        assert_eq!(out.len(), 2, "duplicate (src,tgt,rel) edge ignored");

        // empty slice is a no-op
        g.append_edges(&[]).await.unwrap();

        cleanup(g.pool(), prefix).await;
    }

    /// Directed, relation-filtered edge lookup + weighted neighbor aggregation.
    #[tokio::test]
    async fn edges_dir_and_neighbors() {
        let Some(url) = pg_url() else {
            eprintln!("skip: LLMKERNEL_PG_URL unset");
            return;
        };
        let prefix = "lk_sg3_";
        let g = SqlxPgGraph::connect_with_prefix(&url, prefix)
            .await
            .expect("connect");

        for n in &["seed", "t1", "t2", "t3"] {
            g.upsert_node(&sample_node(n)).await.unwrap();
        }

        // seed cites t1/t2/t3 (out), t1 cites seed (in), seed related t1 (other rel)
        g.append_edges(&[
            sample_edge("e1", "seed", "t1", "cites", 1.0),
            sample_edge("e2", "seed", "t2", "cites", 0.5),
            sample_edge("e3", "seed", "t3", "cites", 0.3),
        ])
        .await
        .unwrap();
        g.append_edge(&sample_edge("e4", "t1", "seed", "cites", 2.0))
            .await
            .unwrap();
        g.append_edge(&sample_edge("e5", "seed", "t1", "related", 1.0))
            .await
            .unwrap();

        // Out + "cites": 3 (t1, t2, t3)
        let out_cites = g
            .edges_for_node_dir("seed", EdgeDirection::Out, Some("cites"))
            .await
            .unwrap();
        assert_eq!(out_cites.len(), 3);

        // In + "cites": 1 (from t1)
        let in_cites = g
            .edges_for_node_dir("seed", EdgeDirection::In, Some("cites"))
            .await
            .unwrap();
        assert_eq!(in_cites.len(), 1);
        assert_eq!(in_cites[0].source, "t1");

        // neighbors_weighted Out "cites" → t1(1.0), t2(0.5), t3(0.3), desc by weight
        let neighbors = g
            .neighbors_weighted(&["seed".to_string()], EdgeDirection::Out, Some("cites"))
            .await
            .unwrap();
        assert_eq!(neighbors.len(), 3);
        assert_eq!(neighbors[0].0, "t1");
        assert!((neighbors[0].1 - 1.0).abs() < 1e-9);
        assert_eq!(neighbors[2].0, "t3");

        cleanup(g.pool(), prefix).await;
    }

    /// `remove_edges_for_node` drops only the edges touching the target node.
    #[tokio::test]
    async fn remove_edges_for_node_drops_only_touching() {
        let Some(url) = pg_url() else {
            eprintln!("skip: LLMKERNEL_PG_URL unset");
            return;
        };
        let prefix = "lk_sg4_";
        let g = SqlxPgGraph::connect_with_prefix(&url, prefix)
            .await
            .expect("connect");

        for n in &["x", "y", "z"] {
            g.upsert_node(&sample_node(n)).await.unwrap();
        }
        g.append_edges(&[
            sample_edge("e1", "x", "y", "cites", 1.0),
            sample_edge("e2", "z", "x", "cites", 1.0),
            sample_edge("e3", "y", "z", "cites", 1.0),
        ])
        .await
        .unwrap();
        assert_eq!(g.edges_for_node("x").await.unwrap().len(), 2);

        g.remove_edges_for_node("x").await.unwrap();
        assert_eq!(g.edges_for_node("x").await.unwrap().len(), 0);
        // y-z edge survives (neither endpoint is x)
        assert_eq!(g.edges_for_node("y").await.unwrap().len(), 1);

        cleanup(g.pool(), prefix).await;
    }

    /// Transaction atomicity: `*_in_tx` commit and rollback paths.
    /// This is the core guarantee klr relies on for atomic prune.
    #[tokio::test]
    async fn tx_atomicity_commit_and_rollback() {
        let Some(url) = pg_url() else {
            eprintln!("skip: LLMKERNEL_PG_URL unset");
            return;
        };
        let prefix = "lk_sg5_";
        let g = SqlxPgGraph::connect_with_prefix(&url, prefix)
            .await
            .expect("connect");

        for n in &["p", "q"] {
            g.upsert_node(&sample_node(n)).await.unwrap();
        }

        // Commit path: append_edges_in_tx → commit → edge visible
        let mut tx = g.pool().begin().await.expect("begin tx");
        g.append_edges_in_tx(&mut tx, &[sample_edge("e1", "p", "q", "cites", 1.0)])
            .await
            .unwrap();
        tx.commit().await.expect("commit");
        assert_eq!(g.edges_for_node("p").await.unwrap().len(), 1);

        // Rollback path: append_edges_in_tx → rollback → edge NOT visible
        let mut tx2 = g.pool().begin().await.expect("begin tx2");
        g.append_edges_in_tx(&mut tx2, &[sample_edge("e2", "p", "q", "cites", 1.0)])
            .await
            .unwrap();
        tx2.rollback().await.expect("rollback");
        assert_eq!(
            g.edges_for_node("p").await.unwrap().len(),
            1,
            "rolled-back edge not visible"
        );

        // remove_edges_for_node_in_tx rollback → edge survives
        let mut tx3 = g.pool().begin().await.expect("begin tx3");
        g.remove_edges_for_node_in_tx(&mut tx3, "p").await.unwrap();
        tx3.rollback().await.expect("rollback tx3");
        assert_eq!(g.edges_for_node("p").await.unwrap().len(), 1);

        // remove_edges_for_node_in_tx commit → edge removed
        let mut tx4 = g.pool().begin().await.expect("begin tx4");
        g.remove_edges_for_node_in_tx(&mut tx4, "p").await.unwrap();
        tx4.commit().await.expect("commit tx4");
        assert_eq!(g.edges_for_node("p").await.unwrap().len(), 0);

        cleanup(g.pool(), prefix).await;
    }

    /// ILIKE search + recursive BFS traversal + version helpers.
    #[tokio::test]
    async fn search_related_and_version() {
        let Some(url) = pg_url() else {
            eprintln!("skip: LLMKERNEL_PG_URL unset");
            return;
        };
        let prefix = "lk_sg6_";
        let g = SqlxPgGraph::connect_with_prefix(&url, prefix)
            .await
            .expect("connect");

        let mut rust = sample_node("rust");
        rust.title = "Rust ownership".into();
        rust.body = "borrow checker".into();
        g.upsert_node(&rust).await.unwrap();
        g.upsert_node(&sample_node("py")).await.unwrap();

        // search_nodes
        let hits = g.search_nodes("rust", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "rust");
        assert!(g.search_nodes("", 10).await.unwrap().is_empty());

        // related_nodes via BFS
        g.append_edge(&sample_edge("e1", "rust", "py", "related", 1.0))
            .await
            .unwrap();
        let related = g.related_nodes("rust", 2).await.unwrap();
        assert!(related.contains(&"py".to_string()));

        // version helpers (schema applied on connect)
        assert_eq!(g.current_version().await.unwrap(), GRAPH_SCHEMA_VERSION);
        assert_eq!(g.migrate().await.unwrap(), GRAPH_SCHEMA_VERSION);

        cleanup(g.pool(), prefix).await;
    }

    /// An invalid table prefix is rejected at construction (before any SQL).
    #[tokio::test]
    async fn invalid_prefix_rejected() {
        let Some(url) = pg_url() else {
            eprintln!("skip: LLMKERNEL_PG_URL unset");
            return;
        };
        assert!(
            SqlxPgGraph::connect_with_prefix(&url, "lk; drop")
                .await
                .is_err()
        );
        assert!(SqlxPgGraph::connect_with_prefix(&url, "1lk").await.is_err());
    }
}
