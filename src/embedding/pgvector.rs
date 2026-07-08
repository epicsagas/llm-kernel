//! pgvector `AsyncVectorIndex` — PostgreSQL + the `pgvector` extension.
//!
//! `PgVectorIndex` implements [`AsyncVectorIndex`] over a `sqlx::PgPool`,
//! mirroring `qdrant`/`elastic` (async remote vector backend). Vectors live in
//! a `{table}(id BIGINT PK, vec vector)` relation; search uses the cosine
//! distance operator `<=>`. Requires `CREATE EXTENSION vector` in the target DB
//! (the table + HNSW index are created automatically by [`PgVectorIndex::new`]).

use async_trait::async_trait;
use sqlx::PgPool;
use sqlx::QueryBuilder;
use sqlx::postgres::PgPoolOptions;

use crate::embedding::vector_index::SearchHit;
use crate::error::{KernelError, Result};

/// 검색 결과 행(id + cosine 유사도) — 튜플 대신 구조체로 sqlx `FromRow` 안정 매핑.
#[derive(sqlx::FromRow)]
struct ScoreRow {
    id: i64,
    score: f64,
}

/// f32 슬라이스 → pgvector 문자열 리터럴 `[1,2,3]` (text → vector 입력 캐스트).
/// `pgvector::Vector`의 sqlx `Type` 바인드가 의존 환경에 따라 충돌해 문자열로 회피.
fn vec_literal(v: &[f32]) -> String {
    let mut s = String::from("[");
    for (i, f) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&f.to_string());
    }
    s.push(']');
    s
}

/// PostgreSQL vector index backed by the `pgvector` extension.
///
/// All operations are async over a shared `PgPool` (MVCC, connection-pooled).
/// The relation named `table` holds `(id BIGINT PK, vec vector(N))` rows.
pub struct PgVectorIndex {
    pool: PgPool,
    table: String,
    dim: usize,
}

impl PgVectorIndex {
    /// Connect to `url` (libpq connstring / `postgresql://…`), create the
    /// vector table + HNSW cosine index if missing, and return a ready index.
    ///
    /// `dim` is enforced by the fixed `vector(dim)` column; vectors whose
    /// length differs from `dim` are rejected by pgvector on insert.
    pub async fn new(url: &str, table: &str, dim: usize) -> Result<Self> {
        validate_table_name(table)?;
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(url)
            .await
            .map_err(|e| KernelError::Embedding(format!("pgvector connect: {e}")))?;
        let idx = Self {
            pool,
            table: table.to_string(),
            dim,
        };
        idx.init_schema().await?;
        Ok(idx)
    }

    /// `CREATE TABLE IF NOT EXISTS {table} (id BIGINT PK, vec vector)` + HNSW
    /// cosine index. Idempotent.
    async fn init_schema(&self) -> Result<()> {
        // Identifier is caller-controlled (not user input at runtime) and
        // validated in `new`, so `format!` is acceptable here. PG cannot bind
        // identifiers. Callers pass a fixed, validated table name.
        sqlx::query(&format!(
            "CREATE TABLE IF NOT EXISTS {} (id BIGINT PRIMARY KEY, vec vector({}) NOT NULL)",
            self.table, self.dim
        ))
        .execute(&self.pool)
        .await
        .map_err(|e| KernelError::Embedding(format!("pgvector create table: {e}")))?;
        sqlx::query(&format!(
            "CREATE INDEX IF NOT EXISTS idx_{}_vec ON {} USING hnsw (vec vector_cosine_ops)",
            self.table, self.table
        ))
        .execute(&self.pool)
        .await
        .map_err(|e| KernelError::Embedding(format!("pgvector hnsw index: {e}")))?;
        Ok(())
    }

    /// Underlying connection pool.
    ///
    /// Callers that need **cross-table transactional consistency** — e.g. pruning
    /// a law's chunks plus their vectors atomically in one transaction — can
    /// `pool().begin()` and run their own DML on the same pool the index uses.
    /// This preserves the table-name encapsulation while letting the caller
    /// coordinate a multi-statement transaction.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Remove vectors by external IDs **within a caller-provided transaction**.
    ///
    /// Enables atomic cross-table deletes: begin a tx on [`pool`](Self::pool),
    /// delete related rows in other relations (chunks, edges, …), call this with
    /// the same `&mut PgConnection`, then `commit()`. A failure anywhere rolls
    /// back the whole set — no orphaned vectors or chunks. The table name stays
    /// encapsulated (`format!` over the validated identifier).
    pub async fn remove_in_tx(&self, tx: &mut sqlx::PgConnection, ids: &[u64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let ids: Vec<i64> = ids.iter().map(|&i| to_pg_id(i)).collect::<Result<_>>()?;
        sqlx::query(&format!("DELETE FROM {} WHERE id = ANY($1)", self.table))
            .bind(&ids)
            .execute(tx)
            .await
            .map_err(|e| KernelError::Embedding(format!("pgvector remove_in_tx: {e}")))?;
        Ok(())
    }
}

#[async_trait]
impl crate::embedding::AsyncVectorIndex for PgVectorIndex {
    async fn add(&self, vectors: &[Vec<f32>], ids: &[u64]) -> Result<()> {
        if vectors.len() != ids.len() {
            return Err(KernelError::Embedding(format!(
                "vectors.len() ({}) must equal ids.len() ({})",
                vectors.len(),
                ids.len()
            )));
        }
        if vectors.is_empty() {
            return Ok(());
        }
        // Map u64 → i64 up front: pgvector stores BIGINT (i64), so values
        // above i64::MAX cannot be represented and must be rejected rather
        // than silently wrapped. Single batched INSERT → one round trip.
        let pg_ids: Vec<i64> = ids.iter().map(|&id| to_pg_id(id)).collect::<Result<_>>()?;
        let mut q = QueryBuilder::new("INSERT INTO ");
        q.push(self.table.as_str());
        q.push(" (id, vec) VALUES ");
        // pgvector `vec` 컬럼은 text 리터럴 입력 시 `::vector` 캐스트가 필수다.
        // `push_values` 는 값별 캐스트를 붙일 수 없어 수동으로 VALUES 튜플을 조립한다
        // (캐스트 누락 시 "column vec is of type vector but expression is of type text").
        for (i, (v, &id)) in vectors.iter().zip(pg_ids.iter()).enumerate() {
            if i > 0 {
                q.push(", ");
            }
            q.push("(");
            q.push_bind(id);
            q.push(", ");
            q.push_bind(vec_literal(v));
            q.push("::vector)");
        }
        q.push(" ON CONFLICT (id) DO UPDATE SET vec = EXCLUDED.vec");
        q.build()
            .execute(&self.pool)
            .await
            .map_err(|e| KernelError::Embedding(format!("pgvector add: {e}")))?;
        Ok(())
    }

    async fn remove(&self, ids: &[u64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let ids: Vec<i64> = ids.iter().map(|&i| to_pg_id(i)).collect::<Result<_>>()?;
        sqlx::query(&format!("DELETE FROM {} WHERE id = ANY($1)", self.table))
            .bind(&ids)
            .execute(&self.pool)
            .await
            .map_err(|e| KernelError::Embedding(format!("pgvector remove: {e}")))?;
        Ok(())
    }

    async fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>> {
        let q = vec_literal(query);
        // cosine distance <=> : 0 (동일) .. 2 (반대). score = 1 - distance.
        let rows: Vec<ScoreRow> = sqlx::query_as(&format!(
            "SELECT id, 1 - (vec <=> $1::vector) AS score FROM {} ORDER BY vec <=> $1::vector LIMIT $2",
            self.table
        ))
        .bind(q)
        .bind(k as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| KernelError::Embedding(format!("pgvector search: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|r| SearchHit {
                id: r.id as u64,
                score: r.score as f32,
            })
            .collect())
    }

    async fn search_filtered(
        &self,
        query: &[f32],
        k: usize,
        allowlist: &[u64],
    ) -> Result<Vec<SearchHit>> {
        if allowlist.is_empty() {
            return Ok(Vec::new());
        }
        let q = vec_literal(query);
        let allow: Vec<i64> = allowlist
            .iter()
            .map(|&i| to_pg_id(i))
            .collect::<Result<_>>()?;
        let rows: Vec<ScoreRow> = sqlx::query_as(&format!(
            "SELECT id, 1 - (vec <=> $1::vector) AS score FROM {} WHERE id = ANY($2) \
             ORDER BY vec <=> $1::vector LIMIT $3",
            self.table
        ))
        .bind(q)
        .bind(&allow)
        .bind(k as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| KernelError::Embedding(format!("pgvector search_filtered: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|r| SearchHit {
                id: r.id as u64,
                score: r.score as f32,
            })
            .collect())
    }

    async fn len(&self) -> Result<usize> {
        let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {}", self.table))
            .fetch_one(&self.pool)
            .await
            .map_err(|e| KernelError::Embedding(format!("pgvector len: {e}")))?;
        Ok(n as usize)
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

/// `u64` external ID → PG `BIGINT` (`i64`). IDs exceeding `i64::MAX` cannot
/// be stored in a BIGINT column — reject rather than silently wrap.
fn to_pg_id(id: u64) -> Result<i64> {
    i64::try_from(id).map_err(|_| KernelError::Embedding(format!("id {id} exceeds BIGINT range")))
}

/// Validate that `table` is a plain, safe SQL identifier (ASCII alphanumeric +
/// `_`, starting with a letter or `_`). It is interpolated into DDL/DML via
/// `format!` (PG cannot bind identifiers), so reject anything that could break
/// out of the identifier context.
fn validate_table_name(table: &str) -> Result<()> {
    let mut chars = table.chars();
    let first_ok = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_');
    let valid = first_ok && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err(KernelError::Embedding(format!(
            "invalid table identifier: {table:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::AsyncVectorIndex;

    /// `LLMKERNEL_PG_URL` 미설정 시 자동 skip (graph-pg pg.rs 패턴).
    fn pg_url() -> Option<String> {
        std::env::var("LLMKERNEL_PG_URL").ok()
    }

    /// add → search → search_filtered → remove 라운드트립. HNSW는 대량 데이터에서
    /// 정확도가 보장되지만 소규모 테스트에선 정확 매칭이 간헐적일 수 있어
    /// 여기선 id/회수 위주로 검증.
    #[tokio::test]
    async fn roundtrip_add_search_remove() {
        let Some(url) = pg_url() else {
            eprintln!("skip pgvector test: LLMKERNEL_PG_URL unset");
            return;
        };
        let table = format!("lk_test_{}", line!());
        let idx = PgVectorIndex::new(&url, &table, 3).await.expect("new");

        let vecs = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let ids = vec![10u64, 20, 30];
        idx.add(&vecs, &ids).await.expect("add");
        assert_eq!(idx.len().await.unwrap(), 3);

        // nearest to [1,0,0] → id 10 우선
        let hits = idx.search(&[1.0, 0.0, 0.0], 1).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, 10);

        // filtered: allow [20,30] → 10 제외
        let hits = idx
            .search_filtered(&[1.0, 0.0, 0.0], 1, &[20, 30])
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_ne!(hits[0].id, 10);

        idx.remove(&[10]).await.unwrap();
        assert_eq!(idx.len().await.unwrap(), 2);

        // cleanup
        sqlx::query(&format!("DROP TABLE IF EXISTS {}", table))
            .execute(&idx.pool)
            .await
            .ok();
    }

    /// `remove_in_tx`: 같은 풀에서 시작한 트랜잭션 내 삭제가 원자 반영되는지 검증.
    /// klr prune(chunks + vectors 단일 tx)의 전제 — 커밋 전 롤백 시 벡터 보존 확인.
    #[tokio::test]
    async fn remove_in_tx_atomic_delete() {
        let Some(url) = pg_url() else {
            eprintln!("skip pgvector test: LLMKERNEL_PG_URL unset");
            return;
        };
        let table = format!("lk_txtx_{}", line!());
        let idx = PgVectorIndex::new(&url, &table, 3).await.expect("new");
        idx.add(&[vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]], &[11, 12])
            .await
            .expect("add");
        assert_eq!(idx.len().await.unwrap(), 2);

        // 커밋 경로: tx 내 remove → commit → 삭제 반영
        let mut tx = idx.pool().begin().await.expect("begin tx");
        idx.remove_in_tx(&mut tx, &[11])
            .await
            .expect("remove_in_tx");
        tx.commit().await.expect("commit");
        assert_eq!(idx.len().await.unwrap(), 1);

        // 롤백 경로: tx 내 remove → rollback → 벡터 보존(원자성)
        let mut tx2 = idx.pool().begin().await.expect("begin tx2");
        idx.remove_in_tx(&mut tx2, &[12])
            .await
            .expect("remove_in_tx2");
        tx2.rollback().await.expect("rollback");
        assert_eq!(idx.len().await.unwrap(), 1);

        // cleanup
        sqlx::query(&format!("DROP TABLE IF EXISTS {}", table))
            .execute(idx.pool())
            .await
            .ok();
    }

    #[test]
    fn rejects_invalid_table_name() {
        // valid identifiers accepted
        assert!(validate_table_name("lk_test_1").is_ok());
        assert!(validate_table_name("_vec").is_ok());
        // rejected — would break out of identifier context
        assert!(validate_table_name("").is_err());
        assert!(validate_table_name("1bad").is_err());
        assert!(validate_table_name("rm; DROP").is_err());
        assert!(validate_table_name("weird\"name").is_err());
        assert!(validate_table_name("sch.tbl").is_err());
    }

    #[test]
    fn rejects_overflowing_id() {
        assert_eq!(to_pg_id(0).unwrap(), 0);
        assert_eq!(to_pg_id(42).unwrap(), 42);
        assert_eq!(to_pg_id(i64::MAX as u64).unwrap(), i64::MAX);
        assert!(to_pg_id((i64::MAX as u64) + 1).is_err());
        assert!(to_pg_id(u64::MAX).is_err());
    }
}
