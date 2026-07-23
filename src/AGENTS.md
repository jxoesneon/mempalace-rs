# Mempalace-rs Project Rules

This file contains project-specific rules for contributors working on `mempalace-rs`.

## Design Philosophy

- **Local-first and offline-first.** No external API calls by default. All default functionality (mining, search, embedding, storage, sync) runs on the local machine.
- **Zero external API by default.** LLM-related modules (`closet_llm`, `llm_client`, `llm_refine`) are opt-in. The CLI never sends user data to a remote service unless the user explicitly configures an LLM endpoint.
- See upstream `CLAUDE.md` for the broader project charter.

## Verification Commands

Run these commands before considering any change complete:

```powershell
cargo build --lib
cargo build --bin mempalace-rs
cargo test --lib
cargo test --bin mempalace-rs
cargo clippy --lib --bin mempalace-rs
```

For coverage:

```powershell
cargo llvm-cov --lib --summary-only
```

## Coverage Threshold

- **Line coverage must stay at or above 80%.**
- The current `cargo llvm-cov --lib --summary-only` target is 95%+ for the library. Any new module should aim for 80%+ line coverage; interactive or hard-to-test surfaces (e.g. `onboarding.rs`) may be documented exceptions.

## Security Audit Summary

The following critical safety blockers were identified and resolved:

1. `src/service.rs` — service token leaked in the `/health` JSON response.
2. `src/mcp_server.rs` — separate `wal_log` writes raw JSON params without redaction.
3. `src/mcp_server.rs` — `add_drawer` content preview logged unredacted.
4. `src/mcp_server.rs` — `mempalace_mine` could ingest arbitrary paths.
5. `src/config.rs` — config paths were not canonicalized/validated.

These issues were addressed. The remaining upstream parity work includes the CLI modules `sweep`, `migrate` / `migrate-wings`, `repair-status`, `palace set-embedder`, `daemon jobs` / `wait`, `hook run`, and localized instruction subcommands. Some of these are intentionally stubbed while their full upstream implementations are ported.

## Known Pre-existing Clippy Warnings

The following warnings are pre-existing and may be ignored unless you are actively refactoring the affected modules. Do not let them block CI; the goal is to keep the count from increasing:

- `clippy::unnecessary_sort_by` in `src/dialect.rs` (lines 347, 408, 897).
- `clippy::unnecessary_sort_by` in `src/extractor.rs` (line 554).
- `clippy::type_complexity` in `src/searcher.rs` (line 590).
- `clippy::type_complexity` in `src/vector_storage.rs` (line 808).
- `clippy::too_many_arguments` in `src/vector_storage.rs` (line 979, `add_memories_batch`).
- `clippy::collapsible_match` in `src/entity_registry.rs` (line 298).

When adding new code, prefer idiomatic Rust to avoid introducing new warnings.

## Public API Curation

`src/lib.rs` should not expose every implementation module. Stable, CLI-facing, and test-facing modules are public; internal modules are private (`mod`). If a new module is only a stub, expose a single stable function through `pub use` rather than the whole module.

To keep verification output focused while upstream modules are being ported, the crate root temporarily allows `dead_code`, `unused_imports`, `unused_variables`, and `unused_mut`.

## Contributor Notes

- Do not add new external API calls or network dependencies without explicit user approval.
- Keep the local/offline-first default in mind for all new commands and modules.
