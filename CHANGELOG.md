# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/jxoesneon/mempalace-rs/releases/tag/v0.1.0
