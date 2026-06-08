# llm-kernel-vector-index

[TurboQuant](https://github.com/RyanCodrai/turbovec) vector index implementation for [`llm-kernel`](https://github.com/epicsagas/llm-kernel) — up to 16x memory compression with SIMD-accelerated approximate nearest neighbor search.

Part of the `llm-kernel` workspace. This crate provides `TurbovecIndex`, a concrete implementation of the `VectorIndex` trait from `llm-kernel`.

## Usage

```rust
use llm_kernel::embedding::VectorIndex;
use llm_kernel_vector_index::TurbovecIndex;

let mut idx = TurbovecIndex::new(1536, 4)?;  // 4-bit = 8x compression
idx.add(&[vec1, vec2, vec3])?;
let hits = idx.search(&query, 10)?;

// Filtered search (BM25 candidates → dense rerank)
let hits = idx.search_filtered(&query, 10, &allowed_ids)?;
```

## Why a separate crate?

TurboQuant pulls in heavy dependencies (faer, nalgebra, openblas). By keeping it in a separate workspace member, `llm-kernel` core stays lightweight while this implementation is available for projects that need large-scale vector indexing.
