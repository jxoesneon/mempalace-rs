# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-05-01

### Added

- **Halls Semantic Routing**: Implemented keyword-based routing for autonomous memory categorization into specialized "Halls" (e.g., `emotions`, `consciousness`, `technical`, `identity`).
- **Write-Ahead Log (WAL)**: Added asynchronous audit logging for all write operations (`add_drawer`, `delete_drawer`, `kg_add`, `kg_invalidate`) to ensure integrity and recoverability.
- **Test Coverage Mandate (>= 90%)**: Achieved 90%+ project-wide line coverage through targeted test generation for orchestration modules (`mcp_server`, `storage`, `vector_storage`).
- **Improved Platform Integration**: Added support for 15+ platform integration guides (Claude Code, Cursor, Windsurf).

### Fixed

- **Cleartext Logging Security**: Sanitized all sensitive logging (certificates, CRLs, frame payloads) across patched dependencies (`rustls-webpki`, `tungstenite`) to resolve 11 code scanning alerts.
- **Storage Pruning Stability**: Fixed edge cases in `prune_memories` and strengthened error handling for invalid configurations and IO failures.

### Changed

- **Native Vector Optimization**: Solidified the native `usearch` + `fastembed` storage engine, achieving ~3.7M tokens/sec performance.

### Fixed

- **Windows Build Failure (Issue #3)**: Fixed `cargo install` failure on Windows caused by usearch v2.24.0 using `MAP_FAILED` (a POSIX-only identifier). Patched `index_plugins.hpp` and `index.hpp` with a `#ifndef MAP_FAILED` guard and applied via Cargo `[patch.crates-io]`.
- **CI C++ Linking (all platforms)**: `patches/usearch/build.rs` was silently excluded by usearch's own `.gitignore` (`/build*` rule), so CI never compiled the C++ bridge — causing `undefined symbol: cxxbridge1$NativeIndex$...` on Linux, macOS, and Windows. Fixed by force-adding `build.rs` past the gitignore.
- **SimSIMD Disabled**: Removed the `simsimd` feature flag from the usearch dependency to avoid cascading SIMD compilation failures in CI environments.

### Added

- **Cross-Platform CI**: Dedicated `compile-windows`, `compile-macos`, and `compile-linux` jobs to catch platform-specific build issues before releases.

## [0.4.1] - 2026-04-10

### Fixed

- **MCP Content Wrapper**: Wrapped `tools/call` responses in MCP-compliant `{"content": [{"type": "text", ...}]}` format, fixing "content is missing" errors in Windsurf and other MCP clients.
- **Capability Advertisement**: Removed false `resources` and `prompts` capability claims from `initialize` response; server now only advertises `tools`.

### Added

- **MCP Protocol Handlers**: Added explicit `resources/list`, `resources/read`, and `prompts/list` handlers as defensive fallbacks.
- **Comprehensive MCP Test Suite**: 59 tests covering all 20 tools, JSON-RPC protocol handling, content wrapper validation, schema completeness, error/edge cases, and full lifecycle tests for KG, diary, and drawer operations.

## [0.4.0] - 2026-04-09

### Added

- **Full Parity with Python Implementation**:
  - Implemented `repair` command for HNSW index recovery from SQLite metadata.
  - Implemented `instructions` command for agent system-prompt onboarding.
  - Added interactive entity confirmation in `mempalace init` using `dialoguer`.
  - Ported emotional-marker and speech-pattern regex parsing to `src/extractor.rs`.
- **Sync with Latest Upstream (April 2026)**:
  - **Deterministic MD5 IDs**: Replaced unstable `DefaultHasher` with stable MD5 hashing for `drawer_id` to ensure idempotent writes.
  - **Mtime-Based Mining Skip**: Implemented file modification time tracking to skip unchanged files during re-mining (significant performance boost).
  - **MCP Server Hardening**: Bounded metadata scans in `mempalace_status` and `mempalace_get_taxonomy` to prevent OOM on massive palaces.
- **Advanced CLI Features**:
  - Enhanced `mine` with `--limit`, `--dry-run`, and `--agent` overrides.
  - Enhanced `search` with `--wing`, `--room`, and `--results` filters.
  - Native `.gitignore` support via hierarchical filtering.
- **Production Infrastructure**:
  - Re-architected CI/CD using an **Artifact-Based Pipeline** (70% faster workflow).
  - Global migration to **Rust 2026** formatting and Clippy-clean standards.

### Changed

- **MCP Tool Standard**: Renamed all tools to use the `mempalace_` prefix for marketplace compatibility.
- **L1 Context Logic**: Migrated context generation to a density-aware, room-grouped engine in the `dialect` module.

## [0.3.0] - 2026-04-09

### Added

- **2026 Gold Standard Benchmarks**:
  - Replaced legacy suites with RULER, StructMem, BABILong, and BEAM for rigorous reasoning validation.
  - Achieved perfect 1.000 integrity scores across all suites.
- **First-Class Android/Termux Support**:
  - Added `scripts/setup_android.sh` for automated mobile environment bootstrapping.
  - Patched `ort-sys` for native Android support and optimized linking against system `onnxruntime`.
- **Network Resilience**:
  - Implemented exponential backoff retry logic for resilient model ingestion during CI and local setup.

## [0.2.0] - 2026-04-08

### Added

- **AAAK Dialect V:3.2 Upgrade**:
  - Versioning, Adaptive Density, Proposition Atomisation, and Temporal Decay.
  - Metadata Overlay, Delta Encoding, and Faithfulness Scoring.
- **Semantic Memory Pruning**: Automated deduplication and clustering.
- **Storage Engine Migration**: Unified VectorStorage (SQLite + usearch).

## [0.1.0] - 2026-04-08

### Added

- **Memory Stack (L0-L3)**: Initial release of the 4-layer hierarchical context system.
