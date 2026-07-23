//! # MemPalace-rs
//!
//! A high-performance, local, offline-first AI memory system built in Rust.
//! It enables users to give their AI "memory" by mining local projects and
//! conversations into a structured palace and knowledge graph.

pub mod backups;
pub mod benchmark;
pub mod benchmarks;
pub mod closet_llm;
pub mod collision_scan;
pub mod config;
pub mod convo_miner;
pub mod convo_scanner;
pub mod corpus_origin;
pub mod daemon;
pub mod dedup;
pub mod dialect;
pub mod diary;
pub mod diary_ingest;
pub mod dynamics;
pub mod embedder_factory;
pub mod embedding;
pub mod entity_detector;
pub mod entity_registry;
pub mod exporter;
pub mod extractor;
pub mod fact_checker;
pub mod format_miner;
pub mod general_extractor;
pub mod hallways;
pub mod hooks_cli;
pub mod i18n;
pub mod instructions_cli;
pub mod knowledge_graph;
pub mod layers;
pub mod llm_client;
pub mod llm_refine;
pub mod mcp_server;
pub mod migrate;
pub mod miner;
pub mod models;
pub mod normalize;
pub mod onboarding;
pub mod palace_graph;
pub mod project_scanner;
pub mod query_sanitizer;
pub mod room_detector_local;
pub mod searcher;
pub mod service;
pub mod shared;
pub mod sources;
pub mod spellcheck;
pub use spellcheck::{should_skip, SpellChecker};
pub mod split_mega_files;
pub mod storage;
pub mod sweeper;
pub mod sync;
pub mod vector_storage;
pub use vector_storage::{MemoryRecord, TemporalRange, VectorStorage};
pub mod wal;
