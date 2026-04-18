use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{stdin, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex, Semaphore};
use tracing::{error, span, warn, Level};

use crate::config::MempalaceConfig;
use crate::dialect::Dialect;
use crate::diary;
use crate::knowledge_graph::KnowledgeGraph;
use crate::palace_graph::PalaceGraph;
use crate::searcher::Searcher;
use crate::vector_storage::VectorStorage;

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Option<Value>,
    id: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    id: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

pub struct McpServer {
    config: MempalaceConfig,
    searcher: Arc<Searcher>,
    kg: Arc<Mutex<KnowledgeGraph>>,
    pg: Arc<Mutex<PalaceGraph>>,
    dialect: Dialect,
    semaphore: Arc<Semaphore>,
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

    pub async fn run(self) -> Result<()> {
        let server = Arc::new(self);
        let mut reader = BufReader::new(stdin());
        let mut line = String::new();
        const MAX_LINE_LENGTH: usize = 10 * 1024 * 1024; // 10MB limit

        // Output channel to serialize writes to stdout
        let (tx, mut rx) = mpsc::channel::<String>(100);

        // Dedicated stdout writer task
        tokio::spawn(async move {
            let mut writer = tokio::io::BufWriter::new(tokio::io::stdout());
            while let Some(msg) = rx.recv().await {
                if let Err(e) = writer.write_all(msg.as_bytes()).await {
                    error!("Stdout writer failed: {}", e);
                    break;
                }
                let _ = writer.flush().await;
            }
        });

        loop {
            line.clear();
            let mut limited_reader = (&mut reader).take(MAX_LINE_LENGTH as u64);
            let bytes_read = limited_reader.read_line(&mut line).await?;

            if bytes_read == 0 {
                break;
            }

            if bytes_read >= MAX_LINE_LENGTH && !line.ends_with('\n') {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32600,
                        message: "Request too large or missing newline".to_string(),
                        data: None,
                    }),
                    id: None,
                };
                let resp_json = serde_json::to_string(&resp)? + "\n";
                let _ = tx.send(resp_json).await;

                let mut dummy = Vec::new();
                let _ = reader.read_until(b'\n', &mut dummy).await?;
                continue;
            }

            let req: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(_) => continue,
            };

            if req.id.is_none() || req.method.starts_with("notifications/") {
                continue;
            }

            // Spawn concurrent handler with backpressure
            let server_clone = Arc::clone(&server);
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                let _permit = server_clone.semaphore.acquire().await.ok();
                let span = span!(Level::INFO, "mcp_request", method = %req.method);
                let _enter = span.enter();

                let resp = server_clone.handle_request(req).await;
                if let Ok(resp_json) = serde_json::to_string(&resp) {
                    let _ = tx_clone.send(resp_json + "\n").await;
                }
            });
        }

        Ok(())
    }

    async fn handle_request(self: &Arc<Self>, req: JsonRpcRequest) -> JsonRpcResponse {
        let method = req.method.clone();

        // Determine if this is a "heavy" tool call (embedding, etc)
        let is_heavy = match method.as_str() {
            "tools/call" => {
                if let Some(params) = &req.params {
                    let name = params["name"].as_str().unwrap_or("");
                    matches!(
                        name,
                        "mempalace_add_drawer"
                            | "mempalace_search"
                            | "mempalace_check_duplicate"
                            | "mempalace_prune"
                    )
                } else {
                    false
                }
            }
            _ => false,
        };

        let result = if is_heavy {
            // Force heavy calls to blocking threads to avoid starving the main runtime
            let self_clone = Arc::clone(self);
            let req_params = req.params.clone();
            tokio::task::spawn_blocking(move || {
                let rt = tokio::runtime::Handle::current();
                rt.block_on(self_clone.handle_tools_call(req_params))
            })
            .await
            .unwrap_or_else(|e| Err(anyhow!("Blocking task panicked: {}", e)))
        } else {
            match method.as_str() {
                "initialize" => self.handle_initialize(req.params),
                "tools/list" => self.handle_tools_list(),
                "tools/call" => self.handle_tools_call(req.params).await,
                "resources/list" => Ok(json!({ "resources": [] })),
                "prompts/list" => Ok(json!({ "prompts": [] })),
                _ => Ok(json!({})),
            }
        };

        match result {
            Ok(res) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(res),
                error: None,
                id: req.id,
            },
            Err(e) => {
                error!("Tool error ({}): {}", method, e);
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32603,
                        message: format!("Internal server error: {}", e),
                        data: None,
                    }),
                    id: req.id,
                }
            }
        }
    }

    fn handle_initialize(&self, _params: Option<Value>) -> Result<Value> {
        Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {
                    "listChanged": true
                }
            },
            "serverInfo": {
                "name": "mempalace-rs",
                "version": env!("CARGO_PKG_VERSION")
            }
        }))
    }

    fn handle_tools_list(&self) -> Result<Value> {
        Ok(json!({
            "tools": [
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
                }
            ]
        }))
    }

    pub async fn handle_tools_call(&self, params: Option<Value>) -> Result<Value> {
        let params = params.ok_or_else(|| anyhow!("Missing params"))?;
        let name = params["name"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing tool name"))?;
        let args = params.get("arguments").unwrap_or(&Value::Null);

        let tool_result = match name {
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
            _ => Err(anyhow!("Unknown tool: {}", name)),
        }?;

        // Wrap in MCP-compliant content format
        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&tool_result)?
            }]
        }))
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
        let room = args["room"].as_str().unwrap_or("general");

        let memory_id = self.searcher.add_memory(content, wing, room, None, None)?;
        let mut pg = self.pg.lock().await;
        pg.add_room(room, wing);

        Ok(json!({ "status": "success", "memory_id": memory_id, "wing": wing, "room": room }))
    }

    pub async fn mempalace_delete_drawer(&self, args: &Value) -> Result<Value> {
        let memory_id = args["memory_id"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid memory_id (integer)"))?;

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
        let agent = if agent_input.ends_with("(via MCP)") {
            agent_input.to_string()
        } else {
            format!("{} (via MCP)", agent_input)
        };

        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing content"))?;

        diary::write_diary(&agent, content)?;
        Ok(json!({ "status": "success", "agent": agent }))
    }

    pub async fn mempalace_diary_read(&self, args: &Value) -> Result<Value> {
        let agent = args["agent"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing agent"))?;
        let last_n = args["last_n"].as_u64().unwrap_or(5).min(1000) as usize;

        let entries = diary::read_diary(agent, last_n)?;
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
}

pub async fn run_mcp_server() -> Result<()> {
    let config = MempalaceConfig::default();
    let server = McpServer::new(config).await?;
    server.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test() -> (MempalaceConfig, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = MempalaceConfig::new(Some(temp_dir.path().to_path_buf()));
        (config, temp_dir)
    }

    fn make_request(method: &str, params: Option<Value>, id: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id,
        }
    }

    #[tokio::test]
    async fn test_handle_request_initialize() {
        let (config, _td) = setup_test();
        let server = Arc::new(McpServer::new_test(config));
        let req = make_request("initialize", None, Some(json!(1)));
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        assert_eq!(res["protocolVersion"], "2024-11-05");
        assert_eq!(res["serverInfo"]["name"], "mempalace-rs");
    }

    #[tokio::test]
    async fn test_handle_request_tools_list() {
        let (config, _td) = setup_test();
        let server = Arc::new(McpServer::new_test(config));
        let req = make_request("tools/list", None, Some(json!(2)));
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        let tools = res["tools"].as_array().unwrap();
        assert!(tools.len() >= 20);
    }

    #[tokio::test]
    async fn test_handle_request_tools_call_content_wrapper() {
        let (config, _td) = setup_test();
        let server = Arc::new(McpServer::new_test(config));
        let req = make_request(
            "tools/call",
            Some(json!({ "name": "mempalace_status", "arguments": {} })),
            Some(json!(3)),
        );
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        let content = res["content"].as_array().expect("missing content array");
        assert!(!content.is_empty());
        assert_eq!(content[0]["type"], "text");
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
}
