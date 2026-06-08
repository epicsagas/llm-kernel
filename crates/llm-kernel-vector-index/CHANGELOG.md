# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-06-08

### Added

- `TurbovecIndex` implementing `llm_kernel::embedding::VectorIndex` trait
- 2-bit and 4-bit quantization (up to 16x memory reduction)
- SIMD-accelerated ANN search (NEON on ARM, AVX-512BW/AVX2 on x86)
- Filtered search with allowlists for hybrid retrieval
- Persistence via `save`/`load` with sidecar metadata
- 17 inline tests including save/load roundtrip and trait object compatibility
