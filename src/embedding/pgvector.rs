//! pgvector `AsyncVectorIndex` — PostgreSQL + the `pgvector` extension.
//!
//! `PgVectorIndex` implements [`AsyncVectorIndex`] over a `sqlx::PgPool`,
//! mirroring `qdrant`/`elastic` (async remote vector backend). Vectors live in
//! a `{table}(id BIGINT PK, vec vector)` relation; search uses the cosine
//! distance operator `<=>`. Requires `CREATE EXTENSION vector` in the target DB
//! (the table + HNSW index are created automatically by [`PgVectorIndex::new`]).

use async_trait::async_trait;
use pgvector::Vector;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::embedding::vector_index::SearchHit;
use crate::error::{KernelError, Result};

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
    /// `dim` is informational (the `vector` column is unbounded; pgvector
    /// validates per-row on insert).
    pub async fn new(url: &str, table: &str, dim: usize) -> Result<Self> {
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
        // Identifier is caller-controlled (not user input at runtime) — format!
        // is acceptable here. Callers pass a fixed table name.
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
        for (v, &id) in vectors.iter().zip(ids.iter()) {
            let vec = Vector::from(v.clone());
            sqlx::query(&format!(
                "INSERT INTO {} (id, vec) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET vec = $2",
                self.table
            ))
            .bind(id as i64)
            .bind(vec)
            .execute(&self.pool)
            .await
            .map_err(|e| KernelError::Embedding(format!("pgvector add: {e}")))?;
        }
        Ok(())
    }

    async fn remove(&self, ids: &[u64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let ids: Vec<i64> = ids.iter().map(|&i| i as i64).collect();
        sqlx::query(&format!("DELETE FROM {} WHERE id = ANY($1)", self.table))
            .bind(&ids)
            .execute(&self.pool)
            .await
            .map_err(|e| KernelError::Embedding(format!("pgvector remove: {e}")))?;
        Ok(())
    }

    async fn search(&self, query: &[f32], k: usize) -> Result<Vec<SearchHit>> {
        let q = Vector::from(query.to_vec());
        // cosine distance <=> : 0 (동일) .. 2 (반대). score = 1 - distance.
        let rows: Vec<(i64, f64)> = sqlx::query_as(&format!(
            "SELECT id, 1 - (vec <=> $1::vector) FROM {} ORDER BY vec <=> $1::vector LIMIT $2",
            self.table
        ))
        .bind(q)
        .bind(k as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| KernelError::Embedding(format!("pgvector search: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(id, score)| SearchHit {
                id: id as u64,
                score: score as f32,
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
        let q = Vector::from(query.to_vec());
        let allow: Vec<i64> = allowlist.iter().map(|&i| i as i64).collect();
        let rows: Vec<(i64, f64)> = sqlx::query_as(&format!(
            "SELECT id, 1 - (vec <=> $1::vector) FROM {} WHERE id = ANY($2) \
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
            .map(|(id, score)| SearchHit {
                id: id as u64,
                score: score as f32,
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
}
