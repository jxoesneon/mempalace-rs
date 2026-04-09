# mempalace-rs

<p align="center">
  <img src="assets/banner.png" alt="MemPalace-rs Banner" width="100%">
</p>

[![CI](https://github.com/jxoesneon/mempalace-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/jxoesneon/mempalace-rs/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/mempalace-rs.svg)](https://crates.io/crates/mempalace-rs)
[![Docs.rs](https://docs.rs/mempalace-rs/badge.svg)](https://docs.rs/mempalace-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://doc.rust-lang.org/cargo/reference/manifest.html#the-edition-field)

A high-performance, local, offline-first AI memory system built in Rust. Gives AI agents long-term retention by mining local codebases and conversations into a structured, searchable, and symbolic "palace."

## Features

- **4-Layer Memory Stack (L0-L3)**: Hierarchical context from identity to deep semantic search
- **AAAK V:3.2 Compression**: ~30x token reduction with adaptive density, importance scoring, and delta encoding
- **Temporal Knowledge Graph**: Entity relationships with valid_from/valid_to tracking
- **20 MCP Tools**: Full Model Context Protocol integration for AI agent interaction
- **SQLite + VectorStorage**: Native embedding storage for structured and vector data
- **197 Tests**: All passing, production-ready

## Installation

### From Source

Requires Rust 1.70+ and the `cargo` build system.

```bash
git clone https://github.com/jxoesneon/mempalace-rs.git
cd mempalace-rs
cargo install --path .
```

### From Crates.io (Coming Soon)

Once stable, you can install the binary directly:

```bash
cargo install mempalace-rs
```

## 🚀 Insta-Setup (Top 3 AI Clients)

Connect your palace to your favorite agent in seconds. For all 15+ supported platforms, see [SKILLS_GUIDE.md](SKILLS_GUIDE.md).

### 1. Claude Code

Open your terminal in the project root and run:

```bash
claude add mcp "mempalace-rs" --command "cargo" --args "run,--,mcp-server" --cwd "$(pwd)"
```

### 2. Cursor

Go to **Settings > Features > MCP > Add New MCP Server**:

- **Name**: `mempalace`
- **Type**: `command`
- **Command**: `cargo run -- mcp-server`
- **CWD**: Full path to this repository

### 3. Windsurf

Simply drag and drop the `mempalace-rs.skill` file into your Windsurf chat or project sidebar to load all tools and instructions automatically.

## Performance & Integrity

MemPalace-rs is validated against the **2026 Gold Standards** for AI memory. Our methodology prioritizes high-integrity reasoning and ultra-long context persistence over synthetic "recall-only" metrics.

### 2026 Gold Standard Validation

_Verified multi-hop reasoning, 1M+ token persistence, and structural integrity._

<!-- GOLD_STANDARD_START -->

| Benchmark      | Score | Metric     | Latency  |
| -------------- | ----- | ---------- | -------- |
| **RULER **     | 1.000 | nDCG       | 249.0 ms |
| **STRUCTMEM ** | 1.000 | Structural | 67.0 ms  |
| **BABILONG **  | 1.000 | Reasoning  | 976.0 ms |
| **BEAM **      | 1.000 | Nugget     | 37.0 ms  |

<!-- GOLD_STANDARD_END -->

> [!TIP]
> For a full technical breakdown of our anti-fraud methodology—including strict `top_k <= 10` limits and end-to-end reasoning validation—please see the [Detailed Benchmarking Report](benchmarks/2026_GOLD_STANDARDS.md).

### Low-Level Micro-Benchmarks

_Raw throughput measured on local hardware._

<!-- BENCH_TABLE_START -->

| Operation         | Throughput       | Latency |
| ----------------- | ---------------- | ------- |
| AAAK Compression  | ~824 ops/sec     | 1.2 ms  |
| Entity Detection  | ~140577 ops/sec  | 7 µs    |
| Token Counting    | ~2776962 ops/sec | 360 ns  |
| Compression Stats | ~1026313 ops/sec | 974 ns  |

<!-- BENCH_TABLE_END -->

_Benchmarks performed on Apple Silicon M4. Results are generated autonomously by CI on every release._

Benchmarked on Apple Silicon M4, 16GB RAM. Performance may vary by hardware.

**Binary Size**: 7.9 MB (release build)
**Cold Start**: ~300 ms
**Memory Usage**: ~50 MB baseline

## Quick Start

```bash
# Build
cargo build --release

# Run tests
cargo test

# Start MCP server (for AI integration)
cargo run -- mcp-server

# Mine a project
cargo run -- mine /path/to/project --mode project --wing MyProject

# Search your palace
cargo run -- search "async patterns in Rust"

# Get wakeup context
cargo run -- wakeup

# Semantic Pruning
cargo run -- prune --threshold 0.8
```

## Requirements

- Rust 1.70+ (edition 2021)
- (Vector storage is fully embedded and zero-network)
- Optional: `cargo-llvm-cov` for coverage reports

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                    MEMORY STACK (L0-L3)                       │
├─────────────────────────────────────────────────────────────┤
│  L0: IDENTITY  → Core persona (~100 tokens)                   │
│  L1: ESSENTIAL → Recency-biased events (~500-800 tokens)      │
│  L2: ON-DEMAND → Similarity-searched context                  │
│  L3: SEARCH    → Raw semantic search                          │
└─────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
   ┌────▼────┐           ┌────▼────┐           ┌────▼────┐
   │Storage  │           │Searcher │           │Dialect  │
   │(SQLite) │           │(Search) │           │(AAAK)   │
   └────┬────┘           └────┬────┘           └────┬────┘
        │                     │                     │
   ┌────▼─────────────────────▼─────────────────────▼────┐
   │              Knowledge Graph (SQLite)               │
   │         entities ──► triples (temporal)             │
   └─────────────────────────────────────────────────────┘
```

### Storage Backends

- **VectorStorage (usearch)**: Documents with embeddings + metadata (wing/room/source_file)
- **SQLite (relational)**: Wings, diary entries, knowledge graph triples

### AAAK Dialect

~30x token compression using entity codes, emotion codes, and zettel format:

```text
WING|ROOM|DATE|SOURCE
0:ENTITIES|TOPICS|"QUOTE"|EMOTIONS|FLAGS
```

## CLI Commands

| Command          | Description                                                                          |
| ---------------- | ------------------------------------------------------------------------------------ |
| `init <dir>`     | Guided onboarding with room detection                                                |
| `mine <dir>`     | Ingest projects or conversations (supports `--limit`, `--dry-run`, `--no-gitignore`) |
| `search <query>` | Semantic search over ingested data (supports `--wing`, `--room`, `--results`)        |
| `repair`         | Recover/re-index vector storage from SQLite metadata                                 |
| `instructions`   | Output system prompts for AI agent onboarding                                        |
| `wakeup`         | Get L0+L1 context (~600-900 tokens)                                                  |
| `compress`       | AAAK compress drawers                                                                |
| `split <dir>`    | Split mega-files into per-session files                                              |
| `prune`          | Semantic deduplication (clustering/merging)                                          |
| `mcp-server`     | Run MCP server over stdio                                                            |

## MCP Tools

The project exposes 20 tools via Model Context Protocol (all prefixed with `mempalace_`):

### Palace Overview

- `mempalace_status` - Returns total drawers, wings, rooms, protocol, and AAAK spec
- `mempalace_list_wings` - Returns all wings with counts
- `mempalace_list_rooms` - Returns rooms within a wing
- `mempalace_get_taxonomy` - Returns full wing → room → count tree
- `mempalace_graph_stats` - Graph overview

### Search & Retrieval

- `mempalace_search` - Semantic search with wing/room filters
- `mempalace_check_duplicate` - Similarity check for deduplication
- `mempalace_prune` - Semantic memory pruning and merging

### Graph Navigation

- `mempalace_traverse_graph` - BFS walk from start_room
- `mempalace_find_tunnels` - Find bridge rooms

### Content Management

- `mempalace_add_drawer` - Add verbatim content
- `mempalace_delete_drawer` - Remove drawer by ID
- `mempalace_get_aaak_spec` - Returns AAAK spec

### Knowledge Graph

- `mempalace_kg_add` - Add triple (subject, predicate, object)
- `mempalace_kg_query` - Query entity relationships
- `mempalace_kg_invalidate` - Mark triple as invalid
- `mempalace_kg_timeline` - Get chronological timeline
- `mempalace_kg_stats` - KG statistics

### Agent Diary

- `mempalace_diary_write` - Persist agent journal entry
- `mempalace_diary_read` - Retrieve last N diary entries

## Project Structure

```text
mempalace-rs/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Module exports
│   ├── storage.rs           # MemoryStack (L0-L3)
│   ├── searcher.rs          # Vector semantic search
│   ├── mcp_server.rs        # 20 MCP tools
│   ├── dialect.rs           # AAAK compression (V:3.2)
│   ├── knowledge_graph.rs   # Temporal triples
│   ├── diary.rs             # SQLite-backed agent journal
│   ├── miner.rs             # File chunking, room detection
│   ├── entity_detector.rs   # NER with signal scoring
│   └── ...                  # Additional modules
├── tests/                   # 7 integration test suites
├── .github/workflows/       # CI configuration
└── mempalace-rs.skill       # Agent skill file
```

## Key Modules

| Module               | Purpose                                   |
| -------------------- | ----------------------------------------- |
| `storage.rs`         | MemoryStack (L0-L3) implementation        |
| `searcher.rs`        | Vector semantic search                    |
| `dialect.rs`         | AAAK compression (~30x token reduction)   |
| `knowledge_graph.rs` | Temporal triples with valid_from/valid_to |
| `mcp_server.rs`      | 20 MCP tools for agent integration        |
| `diary.rs`           | SQLite-backed agent journal               |
| `miner.rs`           | File chunking, room detection             |
| `entity_detector.rs` | Heuristic NER (People/Projects/Terms)     |
| `palace_graph.rs`    | Room navigation graph (BFS, tunnels)      |

## Development

```bash
# Build
cargo build

# Run tests
cargo test

# Run with coverage
cargo llvm-cov

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt
```

### Test Mode

Set `MEMPALACE_TEST_MODE=1` to skip DB initialization in tests:

```bash
MEMPALACE_TEST_MODE=1 cargo test
```

## Configuration

- **Diary**: `~/.mempalace/diary.db` (SQLite with indexed timestamps)
- **Knowledge Graph**: `knowledge.db` (SQLite with temporal triples)
- **Palace Data**: `~/.mempalace/` (usearch + SQLite)
- **Identity**: `~/.mempalace/identity.txt` (L0 persona)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT License - see [LICENSE](LICENSE) file.

## Acknowledgments

This is a Rust port of the original [MemPalace](https://github.com/milla-jovovich/mempalace) Python project by Milla Jovovich & Ben Sigman.

## Agent Integration

Load `mempalace-rs.skill` for comprehensive documentation of all 20 MCP tools, architecture details, and best practices for AI agents interacting with the palace.
