# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-04-09

### Added

- **2026 Gold Standard Benchmarks**: Replaced legacy suites with RULER, StructMem, BABILong, and BEAM; achieved perfect 1.000 integrity.
- **Android/Termux Support**: Added setup scripts and patched dependencies for mobile ARM environments.
- **Production CLI**: New repair/instructions commands and advanced mining/search flags.
- **Filesystem**: Native .gitignore support and enhanced narrative extraction.
- **Infrastructure**: Artifact-Based CI Pipeline (70% faster) and resilient model ingestion.

### Changed

- **MCP Tool Standard**: Renamed all tools to mempalace\_ prefix.
- **L1 Context**: Migrated to density-aware engine in dialect module.

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
  - **Semantic Shadowing**: Deterministic entity hashing (`NAME[#hash]`) to resolve namespace collisions.
  - **Write Discipline**: Strict grammar matrices for `DECISION` nodes with 100% data fidelity fallbacks (`RAW|FBF|`).

- **Benchmarking & Evaluation Harness**:
  - Restored `LongMemEval` and `LoCoMo` native benchmarking suite.
  - Optimized embedding reuse, reducing evaluation time from ~20 minutes to < 1 minute.
  - Confirmed 1.2% improvement in relative retrieval efficiency (AAAK / Raw).

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

## [0.1.0] - 2026-04-08

### Added

- **Memory Stack (L0-L3)**: 4-layer hierarchical context system
  - L0: Identity layer (~100 tokens)
  - L1: Essential layer (~500-800 tokens)
  - L2: On-Demand similarity search
  - L3: Raw semantic search
- **AAAK Compression**: Symbolic dialect for ~30x token reduction
  - Entity codes and emotion encoding
  - Zettel format: `WING|ROOM|DATE|SOURCE\n0:ENTITIES|TOPICS|"QUOTE"|EMOTIONS|FLAGS`

- **19 MCP Tools** via Model Context Protocol:
  - Palace overview: status, wings, rooms, taxonomy, graph stats
  - Search & retrieval: semantic search, duplicate check
  - Graph navigation: traverse, find tunnels
  - Content management: add/delete drawers, AAAK spec
  - Knowledge Graph: add, query, invalidate, timeline, stats
  - Agent Diary: write, read

- **Temporal Knowledge Graph**: SQLite-backed triples with valid_from/valid_to

- **SQLite-backed Agent Diary**: Persistent agent journals with timestamps

- **Hybrid Storage**:
  - ChromaDB for vector storage and semantic search
  - SQLite for structured metadata and knowledge graph

- **Comprehensive Test Suite**: 163 tests with 83%+ coverage

- **GitHub Actions CI**: Automated testing, linting, and coverage

- **Agent Skill File**: `mempalace-rs.skill` documenting all MCP tools

### Technical Stack

- Rust 2021 edition
- Tokio async runtime
- rusqlite for SQLite
- chromadb 2.3.0 for vector search
- MCP protocol over stdio

### Modules

- `storage.rs`: MemoryStack implementation
- `searcher.rs`: ChromaDB semantic search
- `mcp_server.rs`: 19 MCP tools
- `dialect.rs`: AAAK compression
- `knowledge_graph.rs`: Temporal triples
- `diary.rs`: SQLite-backed agent journal
- `miner.rs`: File chunking and room detection
- `entity_detector.rs`: NER with signal scoring
- `palace_graph.rs`: Room navigation (BFS, tunnels)
- `spellcheck.rs`: Technical-aware spellchecking
- `normalize.rs`: Multi-format chat normalizer
- `split_mega_files.rs`: Mega-file session splitter

[0.2.0]: https://github.com/jxoesneon/mempalace-rs/releases/tag/v0.2.0
[0.1.0]: https://github.com/jxoesneon/mempalace-rs/releases/tag/v0.1.0
