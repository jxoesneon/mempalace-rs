//! # MemPalace-rs
//!
//! A high-performance, local, offline-first AI memory system built in Rust.
//! It enables users to give their AI "memory" by mining local projects and
//! conversations into a structured palace and knowledge graph.

pub mod benchmark;
pub mod config;
pub mod convo_miner;
pub mod dialect;
pub mod diary;
pub mod entity_detector;
pub mod entity_registry;
pub mod extractor;
pub mod knowledge_graph;
pub mod mcp_server;
pub mod miner;
pub mod models;
pub mod normalize;
pub mod onboarding;
pub mod palace_graph;
pub mod searcher;
pub mod spellcheck;
pub use spellcheck::{should_skip, SpellChecker};
pub mod split_mega_files;
pub mod storage;
pub mod vector_storage;
pub use vector_storage::{MemoryRecord, TemporalRange, VectorStorage};
