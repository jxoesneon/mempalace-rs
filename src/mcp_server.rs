use anyhow::{anyhow, Result};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::model::{RequestId, ServerJsonRpcMessage};
use rmcp::service::{RequestContext, RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::transport::Transport;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{mpsc, Mutex, Semaphore};
use tracing::{error, span, warn, Instrument, Level};

use crate::config::MempalaceConfig;
use crate::dialect::Dialect;
use crate::diary;
use crate::knowledge_graph::KnowledgeGraph;
use crate::palace_graph::PalaceGraph;
use crate::searcher::Searcher;
use crate::vector_storage::VectorStorage;

pub struct McpServer {
    config: MempalaceConfig,
    searcher: Arc<Searcher>,
    kg: Arc<Mutex<KnowledgeGraph>>,
    pg: Arc<Mutex<PalaceGraph>>,
    dialect: Dialect,
    semaphore: Arc<Semaphore>,
}

/// Applies the diary's "(via MCP)" agent-identity tag (Round 4 anti-spoofing
/// fix), used identically by both `mempalace_diary_write` and
/// `mempalace_diary_read` so a caller who only ever uses the untagged agent
/// name still reads back what it wrote.
fn tag_via_mcp(agent: &str) -> String {
    if agent.ends_with("(via MCP)") {
        agent.to_string()
    } else {
        format!("{} (via MCP)", agent)
    }
}

impl McpServer {
    pub async fn new(config: MempalaceConfig) -> Result<Self> {
        let _ = std::fs::create_dir_all(&config.config_dir);

        let _ = tokio::task::spawn_blocking(|| {
            crate::embedder_factory::EmbedderFactory::get_embedder()
        })
        .await
        .map_err(|e| anyhow!("EmbedderFactory init panicked: {e}"))?;

        let searcher = Arc::new(Searcher::new(config.clone()));

        // Round 3 Fix: Auto-repair index on startup
        let vs_res = tokio::task::spawn_blocking({
            let config = config.clone();
            move || {
                let mut vs = VectorStorage::new(
                    config.config_dir.join("vectors.db"),
                    config.config_dir.join("vectors.usearch"),
                )?;
                let repaired = vs.auto_repair()?;
                if repaired > 0 {
                    vs.save_index(config.config_dir.join("vectors.usearch"))?;
                }
                Ok::<_, anyhow::Error>(())
            }
        })
        .await?;
        if let Err(e) = vs_res {
            warn!("Startup auto-repair failed (continuing): {}", e);
        }

        let kg = KnowledgeGraph::new(
            config
                .config_dir
                .join("knowledge.db")
                .to_str()
                .unwrap_or("knowledge.db"),
        )?;
        let mut pg = PalaceGraph::new();

        // Round 2 Fix: Reload taxonomy from DB on startup
        let vectors_db_path = config.config_dir.join("vectors.db");
        if vectors_db_path.exists() {
            let kg_conn = rusqlite::Connection::open(&vectors_db_path)?;
            let _ = pg.load_from_db(&kg_conn);
        }

        let custom_emotions = config.load_emotions_map();
        let dialect = Dialect::with_custom_emotions(None, None, custom_emotions);

        // Round 3 Fix: Semaphore for backpressure (max 4 concurrent heavy tasks)
        let semaphore = Arc::new(Semaphore::new(4));

        Ok(Self {
            config,
            searcher,
            kg: Arc::new(Mutex::new(kg)),
            pg: Arc::new(Mutex::new(pg)),
            dialect,
            semaphore,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_test(config: MempalaceConfig) -> Self {
        let _ = std::fs::create_dir_all(&config.config_dir);
        let searcher = Arc::new(Searcher::new(config.clone()));
        let kg_path = config.config_dir.join("test_knowledge.db");
        let kg = KnowledgeGraph::new(kg_path.to_str().unwrap_or("test_knowledge.db")).unwrap();
        let pg = PalaceGraph::new();
        let dialect = Dialect::default();
        let semaphore = Arc::new(Semaphore::new(10));

        Self {
            config,
            searcher,
            kg: Arc::new(Mutex::new(kg)),
            pg: Arc::new(Mutex::new(pg)),
            dialect,
            semaphore,
        }
    }

    /// Serves MCP over stdio until the client disconnects.
    ///
    /// rmcp negotiates the lifecycle per client: pre-2026-07-28 clients open
    /// with the `initialize` handshake (falling back to the newest handshake
    /// revision if they ask for one we cannot serve that way), while
    /// 2026-07-28 clients skip it and carry their version in per-request
    /// `_meta`.
    pub async fn run(self) -> Result<()> {
        let (stdin, stdout) = rmcp::transport::stdio();
        let service = McpHandler(Arc::new(self))
            .serve(line_limited_transport(stdin, stdout, MAX_LINE_LENGTH))
            .await?;
        service.waiting().await?;
        Ok(())
    }

    /// The MCP tool catalogue, built once from its JSON definitions.
    fn tools() -> &'static [Tool] {
        &TOOLS
    }
}

static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(|| {
    serde_json::from_value(json!([
        {
            "name": "mempalace_status",
            "description": "Get system status overview. Returns counts of all memory drawers (stored items), wings (top-level categories), rooms (sub-categories), the AAAK compression protocol version, and storage statistics. Use this to understand the current memory palace structure and storage utilization.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mempalace_list_wings",
            "description": "List all wings (top-level memory categories) with their drawer counts. Wings are the highest level of organization in the memory palace (e.g., 'rust_patterns', 'project_ideas'). Use this to discover available memory categories before searching or storing.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mempalace_list_rooms",
            "description": "List all rooms (sub-categories) within a specific wing. Rooms organize memories within a wing (e.g., wing 'rust_patterns' might have rooms 'async', 'macros', 'lifetimes'). Use this to narrow down where to store or retrieve specific memories.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "wing": { "type": "string" }
                },
                "required": ["wing"]
            }
        },
        {
            "name": "mempalace_get_taxonomy",
            "description": "Get complete hierarchical taxonomy: wings -> rooms -> drawer counts. Returns the full tree structure showing how all memories are organized. Use this for navigation and understanding the complete memory organization structure.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mempalace_search",
            "description": "Search memories using semantic/vector similarity. Provide a natural language query to find semantically related content across all drawers. Results ranked by relevance. Optional: filter by wing and room. Use for fuzzy matching when exact keywords are unknown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "wing": { "type": "string" },
                    "room": { "type": "string" },
                    "n_results": { "type": "integer", "default": 5 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "mempalace_check_duplicate",
            "description": "Check if similar content already exists before storing. Provide text to compare against all memories. Returns similarity score (0.0-1.0) to nearest match. Use threshold parameter (default 0.9) to detect duplicates. Essential for preventing redundant storage.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": { "type": "string" },
                    "threshold": { "type": "number", "default": 0.9 }
                },
                "required": ["text"]
            }
        },
        {
            "name": "mempalace_get_aaak_spec",
            "description": "Get the AAAK (Agent-to-Agent Knowledge) protocol specification. Returns the compression format and dialect rules used for encoding/decoding agent communications. Use this to understand the compression scheme for inter-agent knowledge transfer.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mempalace_traverse_graph",
            "description": "Traverse the memory palace graph starting from a specific room. Walks through room connections following the palace architecture. Use max_hops to limit traversal depth. Useful for exploring related memory clusters and spatial relationships.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "start_room": { "type": "string" },
                    "max_hops": { "type": "integer", "default": 2 }
                },
                "required": ["start_room"]
            }
        },
        {
            "name": "mempalace_find_tunnels",
            "description": "Find connection tunnels (bridges) between otherwise disconnected memory rooms. Identifies paths that link different wings through shared concepts. Useful for discovering non-obvious relationships and navigation shortcuts across the memory palace.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mempalace_graph_stats",
            "description": "Get statistics about the palace graph structure: total nodes (rooms), edges (connections), density, and clustering metrics. Use this to understand the overall connectivity and organization health of the memory graph.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mempalace_add_drawer",
            "description": "Store verbatim (exact) content in a memory drawer. Saves the complete text as-is without compression. Optionally specify wing and room for organization. If omitted, stores in default location. Required: content (the text to store). Use for preserving exact code, documentation, or facts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": { "type": "string" },
                    "wing": { "type": "string" },
                    "room": { "type": "string" }
                },
                "required": ["content"]
            }
        },
        {
            "name": "mempalace_delete_drawer",
            "description": "Permanently delete a memory drawer by its ID. This removes the stored content from the system. Required: memory_id (integer ID of the drawer to delete). Use when content is outdated, erroneous, or needs to be purged. Irreversible operation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "memory_id": { "type": "integer" }
                },
                "required": ["memory_id"]
            }
        },
        {
            "name": "mempalace_kg_query",
            "description": "Query the Knowledge Graph for relationships involving a specific entity. Retrieves triples (subject-predicate-object) connected to the entity. Direction parameter controls: 'incoming' (what points to entity), 'outgoing' (what entity points to), or 'both' (default). Use to explore relational knowledge and dependencies.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity": { "type": "string" },
                    "direction": { "type": "string", "enum": ["incoming", "outgoing", "both"], "default": "both" }
                },
                "required": ["entity"]
            }
        },
        {
            "name": "mempalace_kg_add",
            "description": "Assert a new fact into the Knowledge Graph as a semantic triple: Subject -> Predicate -> Object. Creates a permanent relational link (e.g., 'Dependency X' 'breaks' 'Component Y'). Use to build logical chains, track dependencies, and encode structured knowledge beyond raw text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "subject": { "type": "string" },
                    "predicate": { "type": "string" },
                    "object": { "type": "string" }
                },
                "required": ["subject", "predicate", "object"]
            }
        },
        {
            "name": "mempalace_kg_invalidate",
            "description": "Mark a Knowledge Graph triple as invalid/deprecated without deleting it. Preserves the assertion for audit but flags it as no longer current. Use when facts change or assertions become outdated but historical record is needed. Soft-delete for knowledge triples.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "subject": { "type": "string" },
                    "predicate": { "type": "string" },
                    "object": { "type": "string" }
                },
                "required": ["subject", "predicate", "object"]
            }
        },
        {
            "name": "mempalace_kg_timeline",
            "description": "Get chronological history of all Knowledge Graph assertions involving an entity. Shows when triples were added and their validity status. Use to track how knowledge about an entity evolved over time and when specific relationships were established.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity": { "type": "string" }
                },
                "required": ["entity"]
            }
        },
        {
            "name": "mempalace_kg_stats",
            "description": "Get Knowledge Graph statistics: total triples, entities, predicates, and temporal distribution. Use this to understand the scale and structure of the relational knowledge base, distinct from the semantic memory drawers.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mempalace_diary_write",
            "description": "Write an entry to an agent's chronological journal (diary). Records timestamped sequential logs for a specific agent identity. Use for tracking agent actions, decisions, reflections, or session progress. Creates temporal audit trail of agent activity.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["agent", "content"]
            }
        },
        {
            "name": "mempalace_diary_read",
            "description": "Read recent entries from an agent's chronological journal. Returns last N entries (default 5) in reverse chronological order. Use to recall recent agent activities, decisions, or context from previous sessions. Essential for maintaining continuity across invocations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": { "type": "string" },
                    "last_n": { "type": "integer", "default": 5 }
                },
                "required": ["agent"]
            }
        },
        {
            "name": "mempalace_prune",
            "description": "Semantic deduplication. Finds and merges similar memories.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "threshold": { "type": "number", "default": 0.85 },
                    "dry_run": { "type": "boolean", "default": true },
                    "wing": { "type": "string" }
                }
            }
        },
        {
            "name": "mempalace_checkpoint",
            "description": "Save a whole session in one call: semantic-dedups each item, files non-duplicates as drawers, then writes one diary entry.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "wing": { "type": "string" },
                                "room": { "type": "string" },
                                "content": { "type": "string" }
                            },
                            "required": ["wing", "room", "content"]
                        }
                    },
                    "diary": { "type": "object" },
                    "dedup_threshold": { "type": "number", "default": 0.9 }
                },
                "required": ["items"]
            }
        }
    ]))
    .expect("built-in tool definitions are valid MCP tools")
});

impl McpServer {
    pub async fn handle_tools_call(&self, params: Option<Value>) -> Result<Value> {
        let params = params.ok_or_else(|| anyhow!("Missing params"))?;
        let name = params["name"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing tool name"))?;
        let args = params.get("arguments").unwrap_or(&Value::Null);
        let tool_result = self.dispatch_tool(name, args).await?;

        // Wrap in MCP-compliant content format
        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&tool_result)?
            }]
        }))
    }

    /// Runs the named tool and returns its raw (unwrapped) JSON result.
    async fn dispatch_tool(&self, name: &str, args: &Value) -> Result<Value> {
        match name {
            "mempalace_status" => self.mempalace_status().await,
            "mempalace_list_wings" => self.mempalace_list_wings().await,
            "mempalace_list_rooms" => self.mempalace_list_rooms(args).await,
            "mempalace_get_taxonomy" => self.mempalace_get_taxonomy().await,
            "mempalace_search" => self.mempalace_search(args).await,
            "mempalace_check_duplicate" => self.mempalace_check_duplicate(args).await,
            "mempalace_get_aaak_spec" => self.mempalace_get_aaak_spec().await,
            "mempalace_traverse_graph" => self.mempalace_traverse_graph(args).await,
            "mempalace_find_tunnels" => self.mempalace_find_tunnels().await,
            "mempalace_graph_stats" => self.mempalace_graph_stats().await,
            "mempalace_add_drawer" => self.mempalace_add_drawer(args).await,
            "mempalace_delete_drawer" => self.mempalace_delete_drawer(args).await,
            "mempalace_kg_query" => self.mempalace_kg_query(args).await,
            "mempalace_kg_add" => self.mempalace_kg_add(args).await,
            "mempalace_kg_invalidate" => self.mempalace_kg_invalidate(args).await,
            "mempalace_kg_timeline" => self.mempalace_kg_timeline(args).await,
            "mempalace_kg_stats" => self.mempalace_kg_stats().await,
            "mempalace_diary_write" => self.mempalace_diary_write(args).await,
            "mempalace_diary_read" => self.mempalace_diary_read(args).await,
            "mempalace_prune" => self.mempalace_prune(args).await,
            "mempalace_checkpoint" => self.mempalace_checkpoint(args).await,
            _ => Err(anyhow!("Unknown tool: {}", name)),
        }
    }

    pub async fn mempalace_status(&self) -> Result<Value> {
        let vs = VectorStorage::new(
            self.config.config_dir.join("vectors.db"),
            self.config.config_dir.join("vectors.usearch"),
        )?;
        let count = vs.memory_count().unwrap_or(0);
        let pg = self.pg.lock().await;

        Ok(json!({
            "total_memories": count,
            "wings": pg.wings.len(),
            "rooms": pg.rooms.len(),
            "protocol": "mempalace-mcp-v1",
            "aaak_spec": "3.1-pro",
            "storage_engine": "pure-rust-usearch"
        }))
    }

    pub async fn mempalace_list_wings(&self) -> Result<Value> {
        let mut wings = HashMap::new();
        let max_wings = 100; // Round 4 Fix: Metadata Capping
        let pg = self.pg.lock().await;
        for (i, (wing, rooms)) in pg.wings.iter().enumerate() {
            if i >= max_wings {
                break;
            }
            wings.insert(wing.clone(), rooms.len());
        }
        Ok(json!({ "wings": wings, "capped": pg.wings.len() > max_wings }))
    }

    pub async fn mempalace_list_rooms(&self, args: &Value) -> Result<Value> {
        let wing = args["wing"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing wing"))?;
        let pg = self.pg.lock().await;
        let rooms = pg.wings.get(wing).cloned().unwrap_or_default();

        let max_rooms = 500;
        let capped = rooms.len() > max_rooms;
        let result_rooms: Vec<_> = rooms.into_iter().take(max_rooms).collect();

        Ok(json!({ "wing": wing, "rooms": result_rooms, "capped": capped }))
    }

    pub async fn mempalace_get_taxonomy(&self) -> Result<Value> {
        let mut taxonomy = HashMap::new();
        let max_wings = 100; // Hard limit for safety
        let pg = self.pg.lock().await;
        for (i, (wing, rooms)) in pg.wings.iter().enumerate() {
            if i >= max_wings {
                break;
            }
            let mut room_counts = HashMap::new();
            // Capped rooms within taxonomy as well
            for room in rooms.iter().take(100) {
                room_counts.insert(room.clone(), 0);
            }
            taxonomy.insert(wing.clone(), room_counts);
        }
        Ok(json!({ "taxonomy": taxonomy, "capped": pg.wings.len() > max_wings }))
    }

    pub async fn mempalace_search(&self, args: &Value) -> Result<Value> {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing query"))?;
        let wing = args["wing"].as_str().map(|s| s.to_string());
        let room = args["room"].as_str().map(|s| s.to_string());
        let n_results = args["n_results"].as_u64().unwrap_or(5).min(100) as usize; // Round 4 Fix: Limit search results

        let results = self
            .searcher
            .search_memories(query, wing, room, n_results)
            .await?;
        Ok(results)
    }

    pub async fn mempalace_check_duplicate(&self, args: &Value) -> Result<Value> {
        let text = args["text"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing text"))?;
        let threshold = args["threshold"].as_f64().unwrap_or(0.9);

        let results = self.searcher.search_memories(text, None, None, 1).await?;
        let mut similarity = 0.0;
        let mut is_duplicate = false;

        if let Some(hits) = results["results"].as_array() {
            if let Some(first) = hits.first() {
                similarity = first["similarity"].as_f64().unwrap_or(0.0);
                if similarity >= threshold {
                    is_duplicate = true;
                }
            }
        }

        Ok(json!({
            "is_duplicate": is_duplicate,
            "similarity": similarity,
            "threshold": threshold
        }))
    }

    pub async fn mempalace_get_aaak_spec(&self) -> Result<Value> {
        Ok(json!({
            "spec": "AAAK Dialect V:3.2",
            "version": crate::dialect::AAAK_VERSION,
            "compression_ratio": "~30x",
            "layers": ["L0: IDENTITY", "L1: ESSENTIAL", "L2: ON-DEMAND", "L3: SEARCH"],
            "format": "V:3.2\nWING|ROOM|DATE|SOURCE\n0:ENTITIES|TOPICS|\"QUOTE\"|EMOTIONS|FLAGS\nJSON:{overlay}",
            "proposition_format": "V:3.2\nWING|ROOM|DATE|SOURCE\nP0:ENTITIES|TOPICS|EMOTIONS|FLAGS\nP1:ENTITIES|TOPICS",
            "density_range": "1 (compact) – 10 (verbose), default 5",
            "features": [
                "versioning (V:3.2)",
                "adaptive density",
                "metadata overlay (JSON:)",
                "external emotion dictionary (emotions.json)",
                "proposition atomisation (compress_propositions)",
                "faithfulness scoring",
                "delta encoding"
            ],
            "entity_codes": self.dialect.entity_codes.len(),
            "custom_emotions": self.dialect.custom_emotions.len()
        }))
    }

    pub async fn mempalace_traverse_graph(&self, args: &Value) -> Result<Value> {
        let start_room = args["start_room"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing start_room"))?;
        let max_hops = args["max_hops"].as_u64().unwrap_or(2).min(5) as usize; // Capped

        let pg = self.pg.lock().await;
        let connected = pg.find_connected_rooms(start_room, max_hops);
        Ok(json!({ "start_room": start_room, "connected": connected }))
    }

    pub async fn mempalace_find_tunnels(&self) -> Result<Value> {
        let pg = self.pg.lock().await;
        let tunnels = pg.find_tunnels();
        Ok(json!({ "tunnels": tunnels }))
    }

    pub async fn mempalace_graph_stats(&self) -> Result<Value> {
        let pg = self.pg.lock().await;
        Ok(json!({
            "total_rooms": pg.rooms.len(),
            "total_wings": pg.wings.len(),
            "avg_rooms_per_wing": if pg.wings.is_empty() { 0.0 } else { pg.rooms.len() as f64 / pg.wings.len() as f64 }
        }))
    }

    pub async fn mempalace_add_drawer(&self, args: &Value) -> Result<Value> {
        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing content"))?;
        if content.len() > 1_000_000 {
            return Err(anyhow!("Content exceeds maximum size of 1MB"));
        }
        let wing = args["wing"].as_str().unwrap_or("general");
        let room = match args["room"].as_str() {
            Some(r) => r.to_string(),
            None => crate::miner::detect_hall(content, &self.config),
        };

        self.wal_log(
            "add_drawer",
            json!({
                "wing": wing,
                "room": room,
                "content_len": content.len(),
                "content_preview": &content[..content.len().min(100)]
            }),
        )
        .await;

        let memory_id = self.searcher.add_memory(content, wing, &room, None, None)?;
        let mut pg = self.pg.lock().await;
        pg.add_room(&room, wing);

        Ok(json!({ "status": "success", "memory_id": memory_id, "wing": wing, "room": room }))
    }

    pub async fn mempalace_delete_drawer(&self, args: &Value) -> Result<Value> {
        let memory_id = args["memory_id"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid memory_id (integer)"))?;

        self.wal_log("delete_drawer", json!({ "memory_id": memory_id }))
            .await;

        // Round 4 Fix: Protected Wings
        let record = self.searcher.get_memory_by_id(memory_id)?;
        if matches!(record.wing.as_str(), "audit" | "diary" | "system") {
            return Err(anyhow!(
                "Cannot delete protected memory in wing '{}'",
                record.wing
            ));
        }

        self.searcher.delete_memory(memory_id)?;

        Ok(json!({ "status": "success", "memory_id": memory_id }))
    }

    pub async fn mempalace_kg_query(&self, args: &Value) -> Result<Value> {
        let entity = args["entity"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing entity"))?;
        let direction = args["direction"].as_str().unwrap_or("both");

        let kg = self.kg.lock().await;
        let mut results = kg.query_entity(entity, None, direction)?;

        let max_results = 200;
        let capped = results.len() > max_results;
        results.truncate(max_results);

        Ok(json!({ "results": results, "capped": capped }))
    }

    pub async fn mempalace_kg_add(&self, args: &Value) -> Result<Value> {
        let sub = args["subject"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing subject"))?;
        let pred = args["predicate"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing predicate"))?;
        let obj = args["object"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing object"))?;

        self.wal_log(
            "kg_add",
            json!({ "subject": sub, "predicate": pred, "object": obj }),
        )
        .await;

        let kg = self.kg.lock().await;
        let id = kg.add_triple(sub, pred, obj, None, None, 1.0, None, None)?;
        Ok(json!({ "status": "success", "triple_id": id }))
    }

    pub async fn mempalace_kg_invalidate(&self, args: &Value) -> Result<Value> {
        let sub = args["subject"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing subject"))?;
        let pred = args["predicate"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing predicate"))?;
        let obj = args["object"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing object"))?;

        self.wal_log(
            "kg_invalidate",
            json!({ "subject": sub, "predicate": pred, "object": obj }),
        )
        .await;

        let kg = self.kg.lock().await;
        kg.invalidate(sub, pred, obj, None)?;
        Ok(json!({ "status": "success" }))
    }

    pub async fn mempalace_kg_timeline(&self, args: &Value) -> Result<Value> {
        let entity = args["entity"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing entity"))?;
        let kg = self.kg.lock().await;
        let results = kg.query_entity(entity, None, "both")?;

        // Simple timeline sort by valid_from
        let mut sorted = results;
        sorted.sort_by(|a, b| {
            let af = a["valid_from"].as_str().unwrap_or("");
            let bf = b["valid_from"].as_str().unwrap_or("");
            af.cmp(bf)
        });

        Ok(json!({ "entity": entity, "timeline": sorted }))
    }

    pub async fn mempalace_kg_stats(&self) -> Result<Value> {
        let kg = self.kg.lock().await;
        let stats = kg.stats()?;
        Ok(stats)
    }

    pub async fn mempalace_diary_write(&self, args: &Value) -> Result<Value> {
        let agent_input = args["agent"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing agent"))?;

        // Round 4 Fix: Sanitize and Tag Agent Identity to prevent spoofing
        // We enforce a "(via MCP)" suffix for all entries written through this interface
        let agent = tag_via_mcp(agent_input);

        let content = args["content"]
            .as_str()
            .or_else(|| args["summary"].as_str())
            .ok_or_else(|| anyhow!("Missing content or summary"))?;

        diary::write_diary(&agent, content)?;
        Ok(json!({ "status": "success", "agent": agent }))
    }

    pub async fn mempalace_diary_read(&self, args: &Value) -> Result<Value> {
        let agent_input = args["agent"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing agent"))?;
        let last_n = args["last_n"].as_u64().unwrap_or(5).min(1000) as usize;

        // Reads must apply the same "(via MCP)" tagging mempalace_diary_write
        // enforces, or a caller who never learns the tagged form (for example
        // one following an agent-identity convention documented elsewhere)
        // gets an empty result for entries that do exist.
        let agent = tag_via_mcp(agent_input);
        let entries = diary::read_diary(&agent, last_n)?;
        Ok(json!({ "entries": entries }))
    }

    pub async fn mempalace_prune(&self, args: &Value) -> Result<Value> {
        let threshold = args["threshold"].as_f64().unwrap_or(0.85) as f32;
        let dry_run = args["dry_run"].as_bool().unwrap_or(true);
        let wing = args["wing"].as_str().map(|s| s.to_string());

        let storage_path = self.config.config_dir.join("palace.db");
        let config = self.config.clone();

        let report = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;

            rt.block_on(async {
                let storage =
                    crate::storage::Storage::new(storage_path.to_str().unwrap_or("palace.db"))?;
                storage
                    .prune_memories(&config, threshold, dry_run, wing)
                    .await
            })
        })
        .await??;

        Ok(json!({
            "status": "success",
            "dry_run": dry_run,
            "report": report
        }))
    }

    pub async fn mempalace_checkpoint(&self, args: &Value) -> Result<Value> {
        let items = args["items"]
            .as_array()
            .ok_or_else(|| anyhow!("Missing or invalid items array"))?;
        let dedup_threshold = args["dedup_threshold"].as_f64().unwrap_or(0.9);

        let mut filed_count = 0;
        let mut skipped_count = 0;
        let mut results = Vec::new();

        for item in items {
            let wing = item["wing"].as_str().unwrap_or("general");
            let room = item["room"].as_str().unwrap_or("general");
            let content = match item["content"].as_str() {
                Some(c) => c,
                None => continue,
            };

            let dup_args = json!({
                "content": content,
                "threshold": dedup_threshold
            });
            let dup_res = self.mempalace_check_duplicate(&dup_args).await?;
            let is_duplicate = dup_res["is_duplicate"].as_bool().unwrap_or(false);

            if is_duplicate {
                skipped_count += 1;
                results.push(json!({
                    "wing": wing,
                    "room": room,
                    "status": "skipped_duplicate",
                    "similarity": dup_res["similarity"]
                }));
            } else {
                let add_args = json!({
                    "wing": wing,
                    "room": room,
                    "content": content
                });
                let add_res = self.mempalace_add_drawer(&add_args).await?;
                filed_count += 1;
                results.push(json!({
                    "wing": wing,
                    "room": room,
                    "status": "filed",
                    "memory_id": add_res["memory_id"]
                }));
            }
        }

        let mut diary_written = false;
        if let Some(diary) = args.get("diary").filter(|v| !v.is_null()) {
            let mut diary_payload = diary.clone();
            if diary_payload.get("agent").is_none() && diary_payload.get("agent_name").is_some() {
                diary_payload["agent"] = diary_payload["agent_name"].clone();
            }
            if diary_payload.get("content").is_none() && diary_payload.get("entry").is_some() {
                diary_payload["content"] = diary_payload["entry"].clone();
            }
            let diary_res = self.mempalace_diary_write(&diary_payload).await?;
            if diary_res["status"] == "success" {
                diary_written = true;
            }
        }

        Ok(json!({
            "status": "success",
            "filed_count": filed_count,
            "skipped_count": skipped_count,
            "diary_written": diary_written,
            "items": results
        }))
    }

    async fn wal_log(&self, operation: &str, params: Value) {
        let wal_dir = self.config.config_dir.join("wal");
        let _ = fs::create_dir_all(&wal_dir);
        let wal_file = wal_dir.join("write_log.jsonl");

        let entry = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "operation": operation,
            "params": params
        });

        if let Ok(line) = serde_json::to_string(&entry) {
            use tokio::io::AsyncWriteExt;
            if let Ok(mut file) = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(wal_file)
                .await
            {
                let _ = file.write_all(line.as_bytes()).await;
                let _ = file.write_all(b"\n").await;
            }
        }
    }
}

pub async fn run_mcp_server() -> Result<()> {
    let config = MempalaceConfig::default();
    let server = McpServer::new(config).await?;
    server.run().await
}

/// Upper bound on a single newline-delimited message from the client.
const MAX_LINE_LENGTH: usize = 10 * 1024 * 1024; // 10MB limit

/// Tools that embed or scan the vector store ("heavy" calls).
fn is_heavy(name: &str) -> bool {
    matches!(
        name,
        "mempalace_add_drawer"
            | "mempalace_search"
            | "mempalace_check_duplicate"
            | "mempalace_prune"
    )
}

/// rmcp adapter for [`McpServer`]. Holding it in an `Arc` lets heavy tool
/// calls move onto blocking threads.
#[derive(Clone)]
struct McpHandler(Arc<McpServer>);

impl ServerHandler for McpHandler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("mempalace-rs", env!("CARGO_PKG_VERSION")),
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(McpServer::tools().to_vec()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        McpServer::tools()
            .iter()
            .find(|tool| tool.name == name)
            .cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        if self.get_tool(&name).is_none() {
            return Err(McpError::invalid_params(
                format!("Unknown tool: {}", name),
                None,
            ));
        }
        let args = request.arguments.map(Value::Object).unwrap_or(Value::Null);
        let server = Arc::clone(&self.0);
        let span = span!(Level::INFO, "mcp_request", tool = %name);

        let result = async {
            // Round 3 Fix: backpressure on concurrent tool calls
            let _permit = server.semaphore.acquire().await.ok();
            if is_heavy(&name) {
                // Force heavy calls to blocking threads to avoid starving the main runtime
                let server = Arc::clone(&server);
                let name = name.clone();
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(server.dispatch_tool(&name, &args))
                })
                .await
                .unwrap_or_else(|e| Err(anyhow!("Blocking task panicked: {}", e)))
            } else {
                server.dispatch_tool(&name, &args).await
            }
        }
        .instrument(span)
        .await;

        // Tool failures go back as `isError` results so the model can see
        // them; only protocol problems (e.g. an unknown tool) are JSON-RPC errors.
        let result = match result {
            Ok(value) => {
                let text = serde_json::to_string(&value)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                CallToolResult::success(vec![ContentBlock::text(text)])
            }
            Err(e) => {
                error!("Tool error ({}): {}", name, e);
                CallToolResult::error(vec![ContentBlock::text(e.to_string())])
            }
        };
        Ok(result.into())
    }
}

/// How much of each line is kept for recovering the `id` of an oversized request.
const ID_SALVAGE_PREFIX: usize = 64 * 1024;

/// Builds the server's transport over `read`/`write`, answering any message
/// longer than `max_line` bytes with a JSON-RPC Invalid Request error.
fn line_limited_transport<R, W>(read: R, write: W, max_line: usize) -> LineLimitedTransport<R, W>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let (oversized_tx, oversized) = mpsc::unbounded_channel();
    let reader = LineLimitedReader::new(read, max_line, oversized_tx);
    LineLimitedTransport {
        inner: AsyncRwTransport::new_server(reader, write),
        oversized,
        max_line,
    }
}

/// rmcp's stdio transport plus replies to the oversized lines that
/// [`LineLimitedReader`] drops, so the session survives them and the client's
/// pending request fails promptly instead of hanging.
struct LineLimitedTransport<R: AsyncRead + Unpin, W: AsyncWrite> {
    inner: AsyncRwTransport<RoleServer, LineLimitedReader<R>, W>,
    /// The salvaged `id` (if any) of each dropped line.
    oversized: mpsc::UnboundedReceiver<Option<RequestId>>,
    max_line: usize,
}

impl<R, W> Transport<RoleServer> for LineLimitedTransport<R, W>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(item)
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            // `AsyncRwTransport::receive` is cancellation-safe: a partly read
            // line is kept for the next call.
            tokio::select! {
                Some(id) = self.oversized.recv() => {
                    warn!("Rejected an MCP message over {} bytes", self.max_line);
                    let error = McpError::invalid_request(
                        format!("Request too large: messages are limited to {} bytes", self.max_line),
                        None,
                    );
                    if self.inner.send(ServerJsonRpcMessage::error(error, id)).await.is_err() {
                        return None;
                    }
                }
                message = self.inner.receive() => return message,
            }
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.inner.close().await
    }
}

/// Drops any line longer than `max` bytes before it reaches rmcp.
///
/// rmcp's stdio transport buffers whole lines with no size cap. Past the cap,
/// this stops passing bytes through and replaces the first excess byte with a
/// NUL, so rmcp sees a capped, unparsable line and ignores it. When the line's
/// newline arrives, the request `id` salvaged from its start is sent on
/// `oversized` for [`LineLimitedTransport`] to answer.
struct LineLimitedReader<R> {
    inner: R,
    max: usize,
    since_newline: usize,
    /// The start of the current line, for [`salvage_request_id`].
    prefix: Vec<u8>,
    discarding: bool,
    oversized: mpsc::UnboundedSender<Option<RequestId>>,
}

impl<R> LineLimitedReader<R> {
    fn new(inner: R, max: usize, oversized: mpsc::UnboundedSender<Option<RequestId>>) -> Self {
        Self {
            inner,
            max,
            since_newline: 0,
            prefix: Vec::new(),
            discarding: false,
            oversized,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for LineLimitedReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let start = buf.filled().len();
        loop {
            std::task::ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
            let end = buf.filled().len();
            if end == start {
                return Poll::Ready(Ok(())); // EOF
            }

            // Compact the bytes just read in place, dropping discarded ones.
            let filled = buf.filled_mut();
            let mut kept = start;
            for i in start..end {
                let byte = filled[i];
                if byte == b'\n' {
                    if this.discarding {
                        this.discarding = false;
                        let _ = this.oversized.send(salvage_request_id(&this.prefix));
                    }
                    this.since_newline = 0;
                    this.prefix.clear();
                    filled[kept] = byte;
                    kept += 1;
                } else if !this.discarding {
                    this.since_newline += 1;
                    if this.since_newline > this.max {
                        this.discarding = true;
                        filled[kept] = 0;
                    } else {
                        if this.prefix.len() < ID_SALVAGE_PREFIX {
                            this.prefix.push(byte);
                        }
                        filled[kept] = byte;
                    }
                    kept += 1;
                }
            }
            buf.set_filled(kept);
            // Reporting zero bytes would read as EOF, so keep going until
            // something survives.
            if kept > start {
                return Poll::Ready(Ok(()));
            }
        }
    }
}

/// Recovers the top-level `"id"` from the start of a JSON-RPC message that may
/// be cut off, so an error about it can still be matched to the request.
fn salvage_request_id(prefix: &[u8]) -> Option<RequestId> {
    fn skip_ws(rest: &[u8]) -> &[u8] {
        let n = rest
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        &rest[n..]
    }
    fn next_value<T: serde::de::DeserializeOwned>(rest: &mut &[u8]) -> Option<T> {
        let input: &[u8] = rest;
        let mut values = serde_json::Deserializer::from_slice(input).into_iter::<T>();
        let value = values.next()?.ok()?;
        *rest = &input[values.byte_offset()..];
        Some(value)
    }

    let mut rest = skip_ws(prefix).strip_prefix(b"{")?;
    loop {
        let key: String = next_value(&mut rest)?;
        rest = skip_ws(rest).strip_prefix(b":")?;
        if key == "id" {
            return next_value(&mut rest);
        }
        next_value::<serde::de::IgnoredAny>(&mut rest)?;
        rest = skip_ws(rest).strip_prefix(b",")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test() -> (MempalaceConfig, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = MempalaceConfig::new(Some(temp_dir.path().to_path_buf()));
        (config, temp_dir)
    }

    /// A raw newline-delimited JSON-RPC client wired to a served [`McpHandler`].
    struct WireClient {
        reader: tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
        writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
        _palace: tempfile::TempDir,
    }

    impl WireClient {
        fn start() -> Self {
            Self::start_with_line_limit(MAX_LINE_LENGTH)
        }

        fn start_with_line_limit(max_line: usize) -> Self {
            let (config, palace) = setup_test();
            let handler = McpHandler(Arc::new(McpServer::new_test(config)));
            let (server_io, client_io) = tokio::io::duplex(64 * 1024);
            let (server_read, server_write) = tokio::io::split(server_io);
            let transport = line_limited_transport(server_read, server_write, max_line);
            tokio::spawn(async move {
                if let Ok(service) = handler.serve(transport).await {
                    let _ = service.waiting().await;
                }
            });
            let (reader, writer) = tokio::io::split(client_io);
            Self {
                reader: tokio::io::BufReader::new(reader),
                writer,
                _palace: palace,
            }
        }

        async fn send(&mut self, message: Value) {
            use tokio::io::AsyncWriteExt;
            let line = serde_json::to_string(&message).unwrap() + "\n";
            self.writer.write_all(line.as_bytes()).await.unwrap();
        }

        async fn request(&mut self, message: Value) -> Value {
            use tokio::io::AsyncBufReadExt;
            self.send(message).await;
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();
            serde_json::from_str(&line).unwrap()
        }

        /// Runs the pre-2026-07-28 `initialize` handshake and returns its result.
        async fn initialize(&mut self, protocol_version: &str) -> Value {
            let resp = self
                .request(json!({
                    "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": {
                        "protocolVersion": protocol_version,
                        "capabilities": {},
                        "clientInfo": { "name": "legacy-client", "version": "0.0.1" }
                    }
                }))
                .await;
            self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
                .await;
            resp["result"].clone()
        }
    }

    /// Per-request lifecycle metadata a 2026-07-28 client sends instead of `initialize`.
    fn inline_meta() -> Value {
        json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": { "name": "inline-client", "version": "0.0.1" },
            "io.modelcontextprotocol/clientCapabilities": {}
        })
    }

    #[tokio::test]
    async fn legacy_client_handshake_echoes_its_version() {
        let mut client = WireClient::start();
        let res = client.initialize("2024-11-05").await;

        assert_eq!(res["protocolVersion"], "2024-11-05");
        assert_eq!(res["serverInfo"]["name"], "mempalace-rs");
        assert!(res["capabilities"]["tools"].is_object());

        let resp = client
            .request(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
            .await;
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 20);
        // `resultType` belongs to 2026-07-28 and must not leak to legacy peers.
        assert!(resp["result"].get("resultType").is_none());
    }

    #[tokio::test]
    async fn handshake_naming_2026_07_28_falls_back_to_newest_handshake_version() {
        let mut client = WireClient::start();
        let res = client.initialize("2026-07-28").await;
        assert_eq!(res["protocolVersion"], "2025-11-25");
    }

    #[tokio::test]
    async fn legacy_tools_call_wraps_result_in_text_content() {
        let mut client = WireClient::start();
        client.initialize("2025-06-18").await;

        let resp = client
            .request(json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": "mempalace_status", "arguments": {} }
            }))
            .await;
        let content = resp["result"]["content"]
            .as_array()
            .expect("missing content array");
        assert_eq!(content[0]["type"], "text");
        assert_ne!(resp["result"]["isError"], true);
        let status: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
        assert!(status["total_memories"].is_number());
    }

    #[tokio::test]
    async fn inline_client_needs_no_handshake() {
        let mut client = WireClient::start();

        let resp = client
            .request(json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/list",
                "params": { "_meta": inline_meta() }
            }))
            .await;
        assert_eq!(resp["result"]["resultType"], "complete");
        assert!(resp["result"]["tools"].as_array().unwrap().len() >= 20);

        let resp = client
            .request(json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "_meta": inline_meta(), "name": "mempalace_status", "arguments": {} }
            }))
            .await;
        assert_eq!(resp["result"]["content"][0]["type"], "text");
    }

    #[tokio::test]
    async fn discover_advertises_legacy_and_current_versions() {
        let mut client = WireClient::start();
        let resp = client
            .request(json!({
                "jsonrpc": "2.0", "id": 1, "method": "server/discover",
                "params": { "_meta": inline_meta() }
            }))
            .await;
        let versions = resp["result"]["supportedVersions"].to_string();
        assert!(versions.contains("2024-11-05"), "{resp}");
        assert!(versions.contains("2026-07-28"), "{resp}");
    }

    #[tokio::test]
    async fn unknown_tool_is_a_protocol_error_and_failing_tool_is_an_error_result() {
        let mut client = WireClient::start();
        client.initialize("2025-11-25").await;

        let resp = client
            .request(json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": { "name": "unknown_tool", "arguments": {} }
            }))
            .await;
        assert_eq!(resp["error"]["code"], -32602);

        let resp = client
            .request(json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": "mempalace_list_rooms" }
            }))
            .await;
        assert_eq!(resp["result"]["isError"], true);
    }

    #[tokio::test]
    async fn oversized_request_gets_matching_error_and_session_survives() {
        let mut client = WireClient::start_with_line_limit(4096);
        client.initialize("2025-11-25").await;

        let resp = client
            .request(json!({
                "jsonrpc": "2.0", "id": 7, "method": "tools/call",
                "params": {
                    "name": "mempalace_add_drawer",
                    "arguments": { "content": "x".repeat(10_000) }
                }
            }))
            .await;
        assert_eq!(resp["id"], 7, "{resp}");
        assert_eq!(resp["error"]["code"], -32600);
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("too large"));

        let resp = client
            .request(json!({
                "jsonrpc": "2.0", "id": 8, "method": "tools/call",
                "params": { "name": "mempalace_status", "arguments": {} }
            }))
            .await;
        assert_eq!(resp["id"], 8);
        assert_eq!(resp["result"]["content"][0]["type"], "text");
    }

    #[tokio::test]
    async fn line_limited_reader_drops_only_overlong_lines() {
        use tokio::io::AsyncReadExt;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let input = &b"abc\n{\"id\":5,\"x\":\"long\"}\ndef\n"[..];
        let mut reader = LineLimitedReader::new(input, 8, tx);
        let mut out = Vec::new();
        reader.read_to_end(&mut out).await.unwrap();

        // The long line is cut at the limit and poisoned with a NUL.
        assert_eq!(out, b"abc\n{\"id\":5,\0\ndef\n");
        assert_eq!(rx.try_recv().unwrap(), Some(RequestId::Number(5)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn salvage_request_id_reads_the_top_level_id_from_a_truncated_message() {
        let salvage = |s: &str| salvage_request_id(s.as_bytes());

        assert_eq!(
            salvage(r#"{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"argu"#),
            Some(RequestId::Number(42))
        );
        assert_eq!(
            salvage(r#" { "id" : "req-1" , "method""#),
            Some(RequestId::String("req-1".into()))
        );
        // A nested "id" is not the request's.
        assert_eq!(
            salvage(r#"{"params":{"id":1,"s":"}\"id\":9"},"id":2,"#),
            Some(RequestId::Number(2))
        );
        // Cut off before the id, or not JSON at all.
        assert_eq!(
            salvage(r#"{"jsonrpc":"2.0","method":"tools/call","params":{"#),
            None
        );
        assert_eq!(salvage("garbage"), None);
    }

    #[tokio::test]
    async fn test_mempalace_status() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_status().await.unwrap();
        assert!(res["total_memories"].is_number());
    }

    #[tokio::test]
    async fn test_mempalace_add_drawer() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "content": "test memory content", "wing": "tech", "room": "rust" });
        let res = server.mempalace_add_drawer(&args).await.unwrap();
        assert_eq!(res["status"], "success");
    }

    #[tokio::test]
    async fn test_mempalace_add_drawer_too_large() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let large_content = "a".repeat(1_000_001);
        let args = json!({ "content": large_content });
        let res = server.mempalace_add_drawer(&args).await;
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .to_string()
            .contains("exceeds maximum size"));
    }

    #[tokio::test]
    async fn test_mempalace_delete_nonexistent() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "memory_id": 999999 });
        let res = server.mempalace_delete_drawer(&args).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_mempalace_list_rooms_invalid_wing() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({});
        let res = server.mempalace_list_rooms(&args).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_handle_tools_call_missing_name() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "not_name": "mempalace_status" });
        let res = server.handle_tools_call(Some(args)).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Missing tool name"));
    }

    #[tokio::test]
    async fn test_handle_tools_call_unknown_tool() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "name": "unknown_tool" });
        let res = server.handle_tools_call(Some(args)).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Unknown tool"));
    }

    #[test]
    fn tag_via_mcp_appends_the_suffix_once() {
        assert_eq!(
            tag_via_mcp("claude-contextos"),
            "claude-contextos (via MCP)"
        );
        assert_eq!(
            tag_via_mcp("claude-contextos (via MCP)"),
            "claude-contextos (via MCP)"
        );
    }

    #[tokio::test]
    async fn test_diary_write_and_read_round_trip_with_untagged_agent_name() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);

        // diary::{write,read}_diary resolve $HOME independently of
        // MempalaceConfig's own config_dir, so isolate them here the same
        // way diary.rs's own test does.
        let diary_home = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", diary_home.path());

        let write_args = json!({ "agent": "round-trip-agent", "content": "hello" });
        server.mempalace_diary_write(&write_args).await.unwrap();

        // A caller reading back with the same, untagged agent name it wrote
        // with must find the entry -- this is the read/write symmetry bug
        // being fixed here.
        let read_args = json!({ "agent": "round-trip-agent", "last_n": 5 });
        let result = server.mempalace_diary_read(&read_args).await.unwrap();
        let entries = result["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["content"], "hello");
    }
}
