# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-04-08

## [0.2.0] - 2026-04-08

### Added

- **AAAK Dialect V:3.2 Upgrade**:
  - **Versioning**: Explicit `V:3.2` header for future-proof decoding.
  - **Adaptive Density**: Dynamic summarization control (1-10 density).
  - **Proposition Atomisation**: Fact-level extraction for hyper-atomic memory storage.
  - **Temporal Decay**: Context-aware importance scoring with `WT:N|` prioritization.
  - **Metadata Overlay**: Lossless JSON metadata injection (`JSON:` block).
  - **Delta Encoding**: Incremental context updates to save tokens during multi-turn mining.
  - **Faithfulness Scoring**: Automated quality monitoring for compression integrity.

- **Semantic Memory Pruning**:
  - New `prune` command for automated semantic deduplication.
  - Vector-based clustering and cross-memory entity/topic merging.

- **Storage Engine Enhancements**:
  - Migration to `VectorStorage` for unified SQLite + usearch persistence.
  - Unified `Importance` tracking in memory metadata.

- **MCP Tools Expansion**:
  - `tool_prune`: Semantic deduplication for agents.
  - Updated `tool_get_aaak_spec` with V:3.2 capabilities.

### Fixed

- **144 Unit Test Regressions**: Restored full test suite coverage for core modules.
- **Resource Leak**: Fixed borrow-checker violation in vector database construction.

[0.2.0]: https://github.com/jxoesneon/mempalace-rs/releases/tag/v0.2.0
[0.1.0]: https://github.com/jxoesneon/mempalace-rs/releases/tag/v0.1.0
