//! # llm-kernel-vector-index
//!
//! TurboQuant vector index implementation for [`llm-kernel`].
//!
//! Provides [`TurbovecIndex`] — a compressed vector index backed by
//! [turbovec](https://github.com/RyanCodrai/turbovec) (Google's TurboQuant
//! algorithm) with 2-bit/4-bit quantization (up to 16x memory reduction) and
//! SIMD-accelerated approximate nearest neighbor search.
//!
//! Implements the [`VectorIndex`](llm_kernel::embedding::VectorIndex) trait
//! defined in `llm-kernel`, so it can be used wherever a `VectorIndex` is
//! expected.
//!
//! ## Quick start
//!
//! ```no_run
//! use llm_kernel::embedding::VectorIndex;
//! use llm_kernel_vector_index::TurbovecIndex;
//!
//! let mut idx = TurbovecIndex::new(384, 4).unwrap();
//! idx.add(&[vec![0.1; 384], vec![0.2; 384]]).unwrap();
//! let hits = idx.search(&vec![0.15; 384], 5).unwrap();
//! ```
//!
//! [`llm-kernel`]: https://crates.io/crates/llm-kernel

mod turbovec_index;

pub use turbovec_index::TurbovecIndex;
