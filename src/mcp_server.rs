use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::io::{stdin, stdout, AsyncBufReadExt, AsyncWriteExt, BufReader};

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
    searcher: Searcher,
    kg: KnowledgeGraph,
    pg: PalaceGraph,
    dialect: Dialect,
}

impl McpServer {
    pub async fn new(config: MempalaceConfig) -> Result<Self> {
        // Ensure config directory exists
        let _ = std::fs::create_dir_all(&config.config_dir);

        let searcher = Searcher::new(config.clone());
        let kg = KnowledgeGraph::new(
            config
                .config_dir
                .join("knowledge.db")
                .to_str()
                .unwrap_or("knowledge.db"),
        )?;
        let pg = PalaceGraph::new();
        // Phase 4: load external emotion map and inject into dialect
        let custom_emotions = config.load_emotions_map();
        let dialect = Dialect::with_custom_emotions(None, None, custom_emotions);

        Ok(Self {
            config,
            searcher,
            kg,
            pg,
            dialect,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_test(config: MempalaceConfig) -> Self {
        let _ = std::fs::create_dir_all(&config.config_dir);
        let searcher = Searcher::new(config.clone());
        let kg_path = config.config_dir.join("test_knowledge.db");
        let kg = KnowledgeGraph::new(kg_path.to_str().unwrap_or("test_knowledge.db")).unwrap();
        let pg = PalaceGraph::new();
        let dialect = Dialect::default();

        Self {
            config,
            searcher,
            kg,
            pg,
            dialect,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut reader = BufReader::new(stdin());
        let mut line = String::new();

        while reader.read_line(&mut line).await? > 0 {
            let req: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(_) => {
                    line.clear();
                    continue;
                }
            };

            // JSON-RPC notifications have no id — must NOT send a response
            let is_notification = req.id.is_none() || req.method.starts_with("notifications/");
            if is_notification {
                line.clear();
                continue;
            }

            let resp = self.handle_request(req).await;
            let resp_json = serde_json::to_string(&resp)? + "\n";
            stdout().write_all(resp_json.as_bytes()).await?;
            stdout().flush().await?;
            line.clear();
        }

        Ok(())
    }

    async fn handle_request(&mut self, req: JsonRpcRequest) -> JsonRpcResponse {
        let result = match req.method.as_str() {
            "initialize" => self.handle_initialize(req.params),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(req.params).await,
            "resources/list" => Ok(json!({ "resources": [] })),
            "resources/read" => Err(anyhow!("Resource not found")),
            "prompts/list" => Ok(json!({ "prompts": [] })),
            // Silently return empty object for unknown but non-notification methods
            _ => Ok(json!({})),
        };

        match result {
            Ok(res) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(res),
                error: None,
                id: req.id,
            },
            Err(_) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: "Internal server error".to_string(),
                    data: None,
                }),
                id: req.id,
            },
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
                    "description": "Returns total drawers, wings, rooms, protocol, and AAAK spec.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "mempalace_list_wings",
                    "description": "Returns all wings with counts.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "mempalace_list_rooms",
                    "description": "Returns rooms within a wing.",
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
                    "description": "Returns full wing -> room -> count tree.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "mempalace_search",
                    "description": "Semantic search.",
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
                    "description": "Similarity check.",
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
                    "description": "Returns the AAAK spec.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "mempalace_traverse_graph",
                    "description": "Palace graph walk.",
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
                    "description": "Bridge rooms.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "mempalace_graph_stats",
                    "description": "Graph overview.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "mempalace_add_drawer",
                    "description": "File verbatim content.",
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
                    "description": "Remove drawer.",
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
                    "description": "KG access.",
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
                    "description": "Add triple to KG.",
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
                    "description": "Invalidate triple in KG.",
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
                    "description": "KG timeline.",
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
                    "description": "KG stats.",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "mempalace_diary_write",
                    "description": "Agent journal write.",
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
                    "description": "Agent journal read.",
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

    async fn handle_tools_call(&mut self, params: Option<Value>) -> Result<Value> {
        let params = params.ok_or_else(|| anyhow!("Missing params"))?;
        let name = params["name"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing tool name"))?;
        let args = &params["arguments"];

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

    pub(crate) async fn mempalace_status(&self) -> Result<Value> {
        let vs = VectorStorage::new(
            self.config.config_dir.join("vectors.db"),
            self.config.config_dir.join("vectors.usearch"),
        )?;
        let count = vs.memory_count().unwrap_or(0);

        Ok(json!({
            "total_memories": count,
            "wings": self.pg.wings.len(),
            "rooms": self.pg.rooms.len(),
            "protocol": "mempalace-mcp-v1",
            "aaak_spec": "3.1-pro",
            "storage_engine": "pure-rust-usearch"
        }))
    }

    pub(crate) async fn mempalace_list_wings(&self) -> Result<Value> {
        let mut wings = HashMap::new();
        for (wing, rooms) in &self.pg.wings {
            wings.insert(wing.clone(), rooms.len());
        }
        Ok(json!({ "wings": wings }))
    }

    pub(crate) async fn mempalace_list_rooms(&self, args: &Value) -> Result<Value> {
        let wing = args["wing"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing wing"))?;
        let rooms = self.pg.wings.get(wing).cloned().unwrap_or_default();
        Ok(json!({ "wing": wing, "rooms": rooms }))
    }

    pub(crate) async fn mempalace_get_taxonomy(&self) -> Result<Value> {
        let mut taxonomy = HashMap::new();
        let max_wings = 100; // Hard limit for safety
        for (i, (wing, rooms)) in self.pg.wings.iter().enumerate() {
            if i >= max_wings {
                break;
            }
            let mut room_counts = HashMap::new();
            for room in rooms {
                room_counts.insert(room.clone(), 0);
            }
            taxonomy.insert(wing.clone(), room_counts);
        }
        Ok(json!({ "taxonomy": taxonomy }))
    }

    pub(crate) async fn mempalace_search(&self, args: &Value) -> Result<Value> {
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing query"))?;
        let wing = args["wing"].as_str().map(|s| s.to_string());
        let room = args["room"].as_str().map(|s| s.to_string());
        let n_results = args["n_results"].as_u64().unwrap_or(5).min(1000) as usize;

        let results = self
            .searcher
            .search_memories(query, wing, room, n_results)
            .await?;
        Ok(results)
    }

    pub(crate) async fn mempalace_check_duplicate(&self, args: &Value) -> Result<Value> {
        let text = args["text"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing text"))?;
        let threshold = args["threshold"].as_f64().unwrap_or(0.9);

        let results = self.searcher.search_memories(text, None, None, 1).await?;
        let mut is_duplicate = false;
        let mut similarity = 0.0;

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

    pub(crate) async fn mempalace_get_aaak_spec(&self) -> Result<Value> {
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

    pub(crate) async fn mempalace_traverse_graph(&self, args: &Value) -> Result<Value> {
        let start_room = args["start_room"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing start_room"))?;
        let max_hops = args["max_hops"].as_u64().unwrap_or(2).min(10) as usize;

        let connected = self.pg.find_connected_rooms(start_room, max_hops);
        Ok(json!({ "start_room": start_room, "connected": connected }))
    }

    pub(crate) async fn mempalace_find_tunnels(&self) -> Result<Value> {
        let tunnels = self.pg.find_tunnels();
        Ok(json!({ "tunnels": tunnels }))
    }

    pub(crate) async fn mempalace_graph_stats(&self) -> Result<Value> {
        Ok(json!({
            "total_rooms": self.pg.rooms.len(),
            "total_wings": self.pg.wings.len(),
            "avg_rooms_per_wing": if self.pg.wings.is_empty() { 0.0 } else { self.pg.rooms.len() as f64 / self.pg.wings.len() as f64 }
        }))
    }

    pub(crate) async fn mempalace_add_drawer(&mut self, args: &Value) -> Result<Value> {
        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing content"))?;
        if content.len() > 1_000_000 {
            return Err(anyhow!("Content exceeds maximum size of 1MB"));
        }
        let wing = args["wing"].as_str().unwrap_or("general");
        let room = args["room"].as_str().unwrap_or("general");

        let memory_id = self.searcher.add_memory(content, wing, room, None, None)?;
        self.pg.add_room(room, wing);

        Ok(json!({ "status": "success", "memory_id": memory_id, "wing": wing, "room": room }))
    }

    pub(crate) async fn mempalace_delete_drawer(&self, args: &Value) -> Result<Value> {
        let memory_id = args["memory_id"]
            .as_i64()
            .ok_or_else(|| anyhow!("Missing or invalid memory_id (integer)"))?;

        self.searcher.delete_memory(memory_id)?;

        Ok(json!({ "status": "success", "memory_id": memory_id }))
    }

    pub(crate) async fn mempalace_kg_query(&self, args: &Value) -> Result<Value> {
        let entity = args["entity"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing entity"))?;
        let direction = args["direction"].as_str().unwrap_or("both");

        let results = self.kg.query_entity(entity, None, direction)?;
        Ok(json!({ "results": results }))
    }

    pub(crate) async fn mempalace_kg_add(&self, args: &Value) -> Result<Value> {
        let sub = args["subject"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing subject"))?;
        let pred = args["predicate"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing predicate"))?;
        let obj = args["object"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing object"))?;

        let id = self
            .kg
            .add_triple(sub, pred, obj, None, None, 1.0, None, None)?;
        Ok(json!({ "status": "success", "triple_id": id }))
    }

    pub(crate) async fn mempalace_kg_invalidate(&self, args: &Value) -> Result<Value> {
        let sub = args["subject"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing subject"))?;
        let pred = args["predicate"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing predicate"))?;
        let obj = args["object"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing object"))?;

        self.kg.invalidate(sub, pred, obj, None)?;
        Ok(json!({ "status": "success" }))
    }

    pub(crate) async fn mempalace_kg_timeline(&self, args: &Value) -> Result<Value> {
        let entity = args["entity"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing entity"))?;
        let results = self.kg.query_entity(entity, None, "both")?;

        // Simple timeline sort by valid_from
        let mut sorted = results;
        sorted.sort_by(|a, b| {
            let af = a["valid_from"].as_str().unwrap_or("");
            let bf = b["valid_from"].as_str().unwrap_or("");
            af.cmp(bf)
        });

        Ok(json!({ "entity": entity, "timeline": sorted }))
    }

    pub(crate) async fn mempalace_kg_stats(&self) -> Result<Value> {
        let stats = self.kg.stats()?;
        Ok(stats)
    }

    pub(crate) async fn mempalace_diary_write(&self, args: &Value) -> Result<Value> {
        let agent = args["agent"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing agent"))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing content"))?;

        diary::write_diary(agent, content)?;
        Ok(json!({ "status": "success" }))
    }

    pub(crate) async fn mempalace_diary_read(&self, args: &Value) -> Result<Value> {
        let agent = args["agent"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing agent"))?;
        let last_n = args["last_n"].as_u64().unwrap_or(5).min(1000) as usize;

        let entries = diary::read_diary(agent, last_n)?;
        Ok(json!({ "entries": entries }))
    }

    pub(crate) async fn mempalace_prune(&self, args: &Value) -> Result<Value> {
        let threshold = args["threshold"].as_f64().unwrap_or(0.85) as f32;
        let dry_run = args["dry_run"].as_bool().unwrap_or(true);
        let wing = args["wing"].as_str().map(|s| s.to_string());

        let storage_path = self.config.config_dir.join("palace.db");
        let storage = crate::storage::Storage::new(storage_path.to_str().unwrap_or("palace.db"))?;

        let report = storage
            .prune_memories(&self.config, threshold, dry_run, wing)
            .await?;

        Ok(json!({
            "status": "success",
            "dry_run": dry_run,
            "report": report
        }))
    }
}

pub async fn run_mcp_server() -> Result<()> {
    let config = MempalaceConfig::default();
    let mut server = McpServer::new(config).await?;
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

    // ── Protocol-level tests ─────────────────────────────────────────

    #[tokio::test]
    async fn test_handle_request_initialize() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request("initialize", None, Some(json!(1)));
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        assert_eq!(res["protocolVersion"], "2024-11-05");
        assert_eq!(res["serverInfo"]["name"], "mempalace-rs");
        assert!(res["capabilities"]["tools"].is_object());
        // resources and prompts should NOT be advertised
        assert!(res["capabilities"]["resources"].is_null());
        assert!(res["capabilities"]["prompts"].is_null());
    }

    #[tokio::test]
    async fn test_handle_request_tools_list() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request("tools/list", None, Some(json!(2)));
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        let tools = res["tools"].as_array().unwrap();
        assert!(
            tools.len() >= 20,
            "Expected at least 20 tools, got {}",
            tools.len()
        );
    }

    #[tokio::test]
    async fn test_handle_request_tools_call_content_wrapper() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request(
            "tools/call",
            Some(json!({ "name": "mempalace_status", "arguments": {} })),
            Some(json!(3)),
        );
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        // Must have MCP-compliant content wrapper
        let content = res["content"].as_array().expect("missing content array");
        assert!(!content.is_empty());
        assert_eq!(content[0]["type"], "text");
        // text field must be valid JSON
        let inner: Value = serde_json::from_str(content[0]["text"].as_str().unwrap())
            .expect("text not valid JSON");
        assert!(inner["total_memories"].is_number());
        assert!(inner["protocol"].is_string());
    }

    #[tokio::test]
    async fn test_handle_request_resources_list() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request("resources/list", None, Some(json!(4)));
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        assert_eq!(res["resources"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_handle_request_resources_read_error() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request("resources/read", None, Some(json!(5)));
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_some());
        assert!(resp.error.unwrap().message.contains("Resource not found"));
    }

    #[tokio::test]
    async fn test_handle_request_prompts_list() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request("prompts/list", None, Some(json!(6)));
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        assert_eq!(res["prompts"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_handle_request_unknown_method() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request("nonexistent/method", None, Some(json!(7)));
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let res = resp.result.unwrap();
        assert!(res.is_object());
    }

    #[tokio::test]
    async fn test_handle_request_preserves_id() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request("initialize", None, Some(json!("my-string-id")));
        let resp = server.handle_request(req).await;
        assert_eq!(resp.id, Some(json!("my-string-id")));
    }

    #[tokio::test]
    async fn test_handle_request_jsonrpc_version() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request("initialize", None, Some(json!(1)));
        let resp = server.handle_request(req).await;
        assert_eq!(resp.jsonrpc, "2.0");
    }

    // ── Tool schema validation ───────────────────────────────────────

    #[tokio::test]
    async fn test_tools_list_schema_completeness() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.handle_tools_list().unwrap();
        let tools = res["tools"].as_array().unwrap();

        for tool in tools {
            let name = tool["name"].as_str().expect("tool missing name");
            assert!(
                tool["description"].as_str().is_some(),
                "tool {} missing description",
                name
            );
            assert!(
                tool["inputSchema"].is_object(),
                "tool {} missing inputSchema",
                name
            );
            assert_eq!(
                tool["inputSchema"]["type"], "object",
                "tool {} inputSchema.type must be 'object'",
                name
            );
        }
    }

    #[tokio::test]
    async fn test_tools_list_expected_names() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.handle_tools_list().unwrap();
        let tools = res["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();

        let expected = [
            "mempalace_status",
            "mempalace_list_wings",
            "mempalace_list_rooms",
            "mempalace_get_taxonomy",
            "mempalace_search",
            "mempalace_check_duplicate",
            "mempalace_get_aaak_spec",
            "mempalace_traverse_graph",
            "mempalace_find_tunnels",
            "mempalace_graph_stats",
            "mempalace_add_drawer",
            "mempalace_delete_drawer",
            "mempalace_kg_query",
            "mempalace_kg_add",
            "mempalace_kg_invalidate",
            "mempalace_kg_timeline",
            "mempalace_kg_stats",
            "mempalace_diary_write",
            "mempalace_diary_read",
            "mempalace_prune",
        ];
        for name in &expected {
            assert!(names.contains(name), "missing tool: {}", name);
        }
    }

    // ── Error / edge-case tests ──────────────────────────────────────

    #[tokio::test]
    async fn test_tools_call_missing_params() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request("tools/call", None, Some(json!(10)));
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_some(), "expected error for missing params");
    }

    #[tokio::test]
    async fn test_tools_call_missing_tool_name() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request(
            "tools/call",
            Some(json!({ "arguments": {} })),
            Some(json!(11)),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_some(), "expected error for missing tool name");
    }

    #[tokio::test]
    async fn test_tools_call_unknown_tool() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let req = make_request(
            "tools/call",
            Some(json!({ "name": "nonexistent_tool", "arguments": {} })),
            Some(json!(12)),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_some());
        assert!(resp.error.unwrap().message.contains("Unknown tool"));
    }

    #[tokio::test]
    async fn test_list_rooms_missing_wing() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_list_rooms(&json!({})).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_kg_add_missing_fields() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);

        // missing subject
        assert!(server
            .mempalace_kg_add(&json!({"predicate": "is", "object": "x"}))
            .await
            .is_err());
        // missing predicate
        assert!(server
            .mempalace_kg_add(&json!({"subject": "x", "object": "y"}))
            .await
            .is_err());
        // missing object
        assert!(server
            .mempalace_kg_add(&json!({"subject": "x", "predicate": "is"}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_delete_drawer_invalid_id() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        // string instead of integer
        let res = server
            .mempalace_delete_drawer(&json!({"memory_id": "bad"}))
            .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_search_missing_query() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_search(&json!({})).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_check_duplicate_missing_text() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_check_duplicate(&json!({})).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_traverse_graph_missing_start_room() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_traverse_graph(&json!({})).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_kg_query_missing_entity() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_kg_query(&json!({})).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_kg_timeline_missing_entity() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_kg_timeline(&json!({})).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_diary_write_missing_agent() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server
            .mempalace_diary_write(&json!({"content": "hello"}))
            .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_diary_write_missing_content() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server
            .mempalace_diary_write(&json!({"agent": "test"}))
            .await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_diary_read_missing_agent() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_diary_read(&json!({})).await;
        assert!(res.is_err());
    }

    // ── Individual tool tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_mcp_initialize() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.handle_initialize(None).unwrap();
        assert_eq!(res["serverInfo"]["name"], "mempalace-rs");
    }

    #[tokio::test]
    async fn test_mcp_tools_list() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.handle_tools_list().unwrap();
        let tools = res["tools"].as_array().unwrap();
        assert!(tools.len() > 10);
    }

    #[tokio::test]
    async fn test_mempalace_status() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_status().await.unwrap();
        assert!(res["total_memories"].is_number());
        assert_eq!(res["protocol"], "mempalace-mcp-v1");
        assert_eq!(res["storage_engine"], "pure-rust-usearch");
        assert!(res["aaak_spec"].is_string());
    }

    #[tokio::test]
    async fn test_mempalace_list_wings() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        server.pg.add_room("room1", "wing1");

        let res = server.mempalace_list_wings().await.unwrap();
        assert_eq!(res["wings"]["wing1"], 1);
    }

    #[tokio::test]
    async fn test_mempalace_list_wings_empty() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_list_wings().await.unwrap();
        assert_eq!(res["wings"].as_object().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_mempalace_list_rooms() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        server.pg.add_room("room1", "wing1");
        server.pg.add_room("room2", "wing1");

        let args = json!({ "wing": "wing1" });
        let res = server.mempalace_list_rooms(&args).await.unwrap();
        let rooms = res["rooms"].as_array().unwrap();
        assert_eq!(rooms.len(), 2);
        assert_eq!(res["wing"], "wing1");
    }

    #[tokio::test]
    async fn test_mempalace_list_rooms_nonexistent_wing() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "wing": "no_such_wing" });
        let res = server.mempalace_list_rooms(&args).await.unwrap();
        assert_eq!(res["rooms"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_mempalace_get_taxonomy() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        server.pg.add_room("room1", "wing1");
        server.pg.add_room("room2", "wing2");

        let res = server.mempalace_get_taxonomy().await.unwrap();
        assert!(res["taxonomy"]["wing1"].is_object());
        assert!(res["taxonomy"]["wing2"].is_object());
    }

    #[tokio::test]
    async fn test_mempalace_get_taxonomy_empty() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_get_taxonomy().await.unwrap();
        assert_eq!(res["taxonomy"].as_object().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_mempalace_graph_stats() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        server.pg.add_room("room1", "wing1");

        let res = server.mempalace_graph_stats().await.unwrap();
        assert_eq!(res["total_rooms"], 1);
        assert_eq!(res["total_wings"], 1);
        assert!(res["avg_rooms_per_wing"].as_f64().unwrap() > 0.0);
    }

    #[tokio::test]
    async fn test_mempalace_graph_stats_empty() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_graph_stats().await.unwrap();
        assert_eq!(res["total_rooms"], 0);
        assert_eq!(res["total_wings"], 0);
        assert_eq!(res["avg_rooms_per_wing"], 0.0);
    }

    #[tokio::test]
    async fn test_mempalace_get_aaak_spec() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_get_aaak_spec().await.unwrap();
        assert!(res["spec"].as_str().unwrap().contains("AAAK Dialect"));
        assert!(res["version"].is_string());
        assert_eq!(res["compression_ratio"], "~30x");
        assert!(res["layers"].as_array().unwrap().len() == 4);
        assert!(res["features"].as_array().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn test_mempalace_search_empty_palace() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "query": "hello world" });
        let res = server.mempalace_search(&args).await.unwrap();
        assert!(res["results"].is_array());
    }

    #[tokio::test]
    async fn test_mempalace_search_with_filters() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "query": "test", "wing": "tech", "room": "code", "n_results": 3 });
        let res = server.mempalace_search(&args).await.unwrap();
        assert!(res["results"].is_array());
    }

    #[tokio::test]
    async fn test_mempalace_check_duplicate_empty_palace() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "text": "something unique", "threshold": 0.95 });
        let res = server.mempalace_check_duplicate(&args).await.unwrap();
        assert_eq!(res["is_duplicate"], false);
        assert!(res["threshold"].as_f64().unwrap() > 0.0);
    }

    #[tokio::test]
    async fn test_mempalace_traverse_graph() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        server.pg.add_room("room1", "wing1");

        let args = json!({ "start_room": "room1", "max_hops": 2 });
        let res = server.mempalace_traverse_graph(&args).await.unwrap();
        assert_eq!(res["start_room"], "room1");
        assert!(res["connected"].is_array());
    }

    #[tokio::test]
    async fn test_mempalace_traverse_graph_default_hops() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "start_room": "unknown_room" });
        let res = server.mempalace_traverse_graph(&args).await.unwrap();
        assert_eq!(res["start_room"], "unknown_room");
    }

    #[tokio::test]
    async fn test_mempalace_find_tunnels() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_find_tunnels().await.unwrap();
        assert!(res["tunnels"].is_array());
    }

    #[tokio::test]
    async fn test_mempalace_add_drawer_content_too_large() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        // 1MB + 1 byte exceeds the enforced limit
        let big = "x".repeat(1_000_001);
        let args = serde_json::json!({ "content": big, "wing": "test", "room": "test" });
        let err = server.mempalace_add_drawer(&args).await;
        assert!(err.is_err(), "content over 1MB must be rejected");
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("1MB"), "error should mention the size limit");
    }

    #[tokio::test]
    async fn test_mempalace_add_drawer() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let args = json!({ "content": "test memory content", "wing": "tech", "room": "rust" });
        let res = server.mempalace_add_drawer(&args).await.unwrap();
        assert_eq!(res["status"], "success");
        assert!(res["memory_id"].is_number());
        assert_eq!(res["wing"], "tech");
        assert_eq!(res["room"], "rust");
    }

    #[tokio::test]
    async fn test_mempalace_add_drawer_defaults() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);
        let args = json!({ "content": "test memory" });
        let res = server.mempalace_add_drawer(&args).await.unwrap();
        assert_eq!(res["status"], "success");
        assert_eq!(res["wing"], "general");
        assert_eq!(res["room"], "general");
    }

    #[tokio::test]
    async fn test_mempalace_add_and_delete_drawer() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);

        // Add
        let add_args = json!({ "content": "ephemeral memory" });
        let add_res = server.mempalace_add_drawer(&add_args).await.unwrap();
        let memory_id = add_res["memory_id"].as_i64().unwrap();

        // Delete
        let del_args = json!({ "memory_id": memory_id });
        let del_res = server.mempalace_delete_drawer(&del_args).await.unwrap();
        assert_eq!(del_res["status"], "success");
        assert_eq!(del_res["memory_id"], memory_id);
    }

    // ── Knowledge Graph tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_mempalace_kg_add() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "subject": "Rust", "predicate": "is_a", "object": "language" });
        let res = server.mempalace_kg_add(&args).await.unwrap();
        assert_eq!(res["status"], "success");
        assert!(res["triple_id"].is_string());
    }

    #[tokio::test]
    async fn test_mempalace_kg_query() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        // Add then query
        server
            .mempalace_kg_add(&json!({
                "subject": "Rust", "predicate": "is_a", "object": "language"
            }))
            .await
            .unwrap();

        let res = server
            .mempalace_kg_query(&json!({ "entity": "Rust" }))
            .await
            .unwrap();
        let results = res["results"].as_array().unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0]["subject"], "Rust");
    }

    #[tokio::test]
    async fn test_mempalace_kg_query_direction_filter() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        server
            .mempalace_kg_add(&json!({
                "subject": "A", "predicate": "knows", "object": "B"
            }))
            .await
            .unwrap();

        let outgoing = server
            .mempalace_kg_query(&json!({ "entity": "A", "direction": "outgoing" }))
            .await
            .unwrap();
        assert!(!outgoing["results"].as_array().unwrap().is_empty());

        let incoming = server
            .mempalace_kg_query(&json!({ "entity": "A", "direction": "incoming" }))
            .await
            .unwrap();
        // A has no incoming edges
        assert!(incoming["results"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_mempalace_kg_invalidate() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        server
            .mempalace_kg_add(&json!({
                "subject": "X", "predicate": "is", "object": "Y"
            }))
            .await
            .unwrap();

        let res = server
            .mempalace_kg_invalidate(&json!({
                "subject": "X", "predicate": "is", "object": "Y"
            }))
            .await
            .unwrap();
        assert_eq!(res["status"], "success");
    }

    #[tokio::test]
    async fn test_mempalace_kg_invalidate_missing_fields() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        assert!(server
            .mempalace_kg_invalidate(&json!({"subject": "X"}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_mempalace_kg_timeline() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        server
            .mempalace_kg_add(&json!({
                "subject": "T", "predicate": "created_at", "object": "2024"
            }))
            .await
            .unwrap();

        let res = server
            .mempalace_kg_timeline(&json!({ "entity": "T" }))
            .await
            .unwrap();
        assert_eq!(res["entity"], "T");
        assert!(res["timeline"].is_array());
    }

    #[tokio::test]
    async fn test_mempalace_kg_stats() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_kg_stats().await.unwrap();
        assert!(res["entities"].is_number());
        assert!(res["triples"].is_number());
    }

    #[tokio::test]
    async fn test_mempalace_kg_full_lifecycle() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);

        // 1. Stats should be empty
        let stats = server.mempalace_kg_stats().await.unwrap();
        assert_eq!(stats["triples"], 0);

        // 2. Add triple
        let add = server
            .mempalace_kg_add(&json!({
                "subject": "mempalace", "predicate": "written_in", "object": "Rust"
            }))
            .await
            .unwrap();
        assert_eq!(add["status"], "success");

        // 3. Query it back
        let query = server
            .mempalace_kg_query(&json!({ "entity": "mempalace" }))
            .await
            .unwrap();
        let results = query["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["object"], "Rust");

        // 4. Stats should reflect the addition
        let stats2 = server.mempalace_kg_stats().await.unwrap();
        assert_eq!(stats2["triples"], 1);

        // 5. Invalidate
        server
            .mempalace_kg_invalidate(&json!({
                "subject": "mempalace", "predicate": "written_in", "object": "Rust"
            }))
            .await
            .unwrap();

        // 6. Timeline should still show the entry (invalidated, not deleted)
        let timeline = server
            .mempalace_kg_timeline(&json!({ "entity": "mempalace" }))
            .await
            .unwrap();
        assert!(timeline["timeline"].is_array());
    }

    // ── Diary tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_mempalace_diary_write_and_read() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);

        let write_args = json!({ "agent": "test-agent", "content": "test diary entry" });
        server.mempalace_diary_write(&write_args).await.unwrap();

        let read_args = json!({ "agent": "test-agent", "last_n": 1 });
        let res = server.mempalace_diary_read(&read_args).await.unwrap();
        let entries = res["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["content"], "test diary entry");
    }

    #[tokio::test]
    async fn test_mempalace_diary_multiple_entries() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);

        for i in 0..5 {
            server
                .mempalace_diary_write(&json!({
                    "agent": "multi-agent",
                    "content": format!("entry {}", i)
                }))
                .await
                .unwrap();
        }

        let res = server
            .mempalace_diary_read(&json!({ "agent": "multi-agent", "last_n": 3 }))
            .await
            .unwrap();
        let entries = res["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[tokio::test]
    async fn test_mempalace_diary_read_empty() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server
            .mempalace_diary_read(&json!({ "agent": "ghost-agent" }))
            .await
            .unwrap();
        assert_eq!(res["entries"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_mempalace_diary_default_last_n() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        // Should default to 5 when last_n not provided
        let res = server
            .mempalace_diary_read(&json!({ "agent": "default-agent" }))
            .await
            .unwrap();
        assert!(res["entries"].is_array());
    }

    // ── Prune test ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_mempalace_prune_dry_run() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let args = json!({ "threshold": 0.9, "dry_run": true });
        let res = server.mempalace_prune(&args).await.unwrap();
        assert_eq!(res["status"], "success");
        assert_eq!(res["dry_run"], true);
    }

    #[tokio::test]
    async fn test_mempalace_prune_defaults() {
        let (config, _td) = setup_test();
        let server = McpServer::new_test(config);
        let res = server.mempalace_prune(&json!({})).await.unwrap();
        assert_eq!(res["dry_run"], true); // default is dry_run=true
    }

    // ── Content wrapper via tools/call for each tool ─────────────────

    #[tokio::test]
    async fn test_content_wrapper_all_parameterless_tools() {
        let (config, _td) = setup_test();
        let mut server = McpServer::new_test(config);

        let parameterless_tools = [
            "mempalace_status",
            "mempalace_list_wings",
            "mempalace_get_taxonomy",
            "mempalace_find_tunnels",
            "mempalace_graph_stats",
            "mempalace_get_aaak_spec",
            "mempalace_kg_stats",
        ];

        for tool_name in &parameterless_tools {
            let req = make_request(
                "tools/call",
                Some(json!({ "name": tool_name, "arguments": {} })),
                Some(json!(tool_name.to_string())),
            );
            let resp = server.handle_request(req).await;
            assert!(
                resp.error.is_none(),
                "tool {} returned error: {:?}",
                tool_name,
                resp.error
            );
            let res = resp.result.unwrap();
            let content = res["content"]
                .as_array()
                .unwrap_or_else(|| panic!("tool {} missing content array", tool_name));
            assert_eq!(
                content[0]["type"], "text",
                "tool {} content type wrong",
                tool_name
            );
            let text = content[0]["text"].as_str().unwrap();
            let _parsed: Value = serde_json::from_str(text)
                .unwrap_or_else(|_| panic!("tool {} text not valid JSON", tool_name));
        }
    }
}
