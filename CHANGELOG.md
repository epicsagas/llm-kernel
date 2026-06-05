# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025-06-05

### Added
- LLM client trait with OpenAI and Anthropic implementations
- SQLite store helpers with WAL mode, FTS5, and schema versioning
- TOML configuration loader with auto-create
- Prompt template rendering with `{{variable}}` substitution
- Common error types via thiserror
