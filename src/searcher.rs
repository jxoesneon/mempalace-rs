use crate::config::MempalaceConfig;
use crate::storage::MemoryStack;
use crate::vector_storage::VectorStorage;
use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// Note: Custom VectorStorage (fastembed + usearch + rusqlite) is used.

/// High-level search interface for retrieving context from the Palace.
///
/// The `Arc<TextEmbedding>` embedder is initialized **once** at construction
/// and shared across all `open_vector_storage` calls, eliminating the
/// 120–180 ms ONNX model re-load that previously occurred on every tool call.
pub struct Searcher {
    pub config: MempalaceConfig,
    embedder: Option<Arc<TextEmbedding>>,
}

impl Searcher {
    /// Construct a `Searcher`, eagerly loading the fastembed ONNX model.
    /// If the model fails to init (e.g. missing files), operations that
    /// require embedding will return an error gracefully.
    pub fn new(config: MempalaceConfig) -> Self {
        let embedder = Self::init_embedder(&config);
        Searcher { config, embedder }
    }

    /// Initialize the fastembed ONNX embedder, respecting MEMPALACE_MODELS_DIR
    /// env var and the binary-adjacent `models/` directory as fallback.
    fn init_embedder(_config: &MempalaceConfig) -> Option<Arc<TextEmbedding>> {
        let cache_dir = std::env::var("MEMPALACE_MODELS_DIR")
            .ok()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.join("models")))
                    .filter(|p| p.exists())
            });

        let mut init_opts =
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false);
        if let Some(cache) = cache_dir {
            init_opts = init_opts.with_cache_dir(cache);
        }

        match TextEmbedding::try_new(init_opts) {
            Ok(emb) => {
                Some(Arc::new(emb))
            }
            Err(e) => {
                eprintln!("[WARN] mempalace: failed to initialise fastembed embedder: {e}");
                None
            }
        }
    }

    /// Open a `VectorStorage` reusing the cached embedder — no ONNX reload.
    fn open_vector_storage(&self) -> Option<VectorStorage> {
        let embedder = self.embedder.clone()?;
        VectorStorage::new_with_embedder(
            self.config.config_dir.join("vectors.db"),
            self.config.config_dir.join("vectors.usearch"),
            embedder,
        )
        .ok()
    }


    pub fn add_memory(
        &self,
        text: &str,
        wing: &str,
        room: &str,
        source_file: Option<&str>,
        source_mtime: Option<f64>,
    ) -> Result<i64> {
        let mut store = self
            .open_vector_storage()
            .ok_or_else(|| anyhow::anyhow!("Vector storage unavailable"))?;
        let id = store.add_memory(text, wing, room, source_file, source_mtime)?;
        store.save_index(self.config.config_dir.join("vectors.usearch"))?;
        Ok(id)
    }

    pub fn delete_memory(&self, memory_id: i64) -> Result<()> {
        let store = self
            .open_vector_storage()
            .ok_or_else(|| anyhow::anyhow!("Vector storage unavailable"))?;
        store.delete_memory(memory_id)?;
        store.save_index(self.config.config_dir.join("vectors.usearch"))?;
        Ok(())
    }

    pub async fn wake_up(&self, wing: Option<String>) -> Result<String> {
        let mut stack = MemoryStack::new(self.config.clone());
        Ok(stack.wake_up(wing).await)
    }

    pub fn build_where_clause(
        wing: Option<&String>,
        room: Option<&String>,
    ) -> Option<serde_json::Value> {
        let mut where_clause = HashMap::<String, serde_json::Value>::new();
        if let (Some(w), Some(r)) = (wing, room) {
            let mut and_vec = Vec::new();
            let mut w_map = HashMap::<String, serde_json::Value>::new();
            w_map.insert("wing".to_string(), serde_json::Value::String(w.to_string()));
            and_vec.push(serde_json::Value::Object(w_map.into_iter().collect()));

            let mut r_map = HashMap::<String, serde_json::Value>::new();
            r_map.insert("room".to_string(), serde_json::Value::String(r.to_string()));
            and_vec.push(serde_json::Value::Object(r_map.into_iter().collect()));

            where_clause.insert("$and".to_string(), serde_json::Value::Array(and_vec));
        } else if let Some(w) = wing {
            where_clause.insert("wing".to_string(), serde_json::Value::String(w.to_string()));
        } else if let Some(r) = room {
            where_clause.insert("room".to_string(), serde_json::Value::String(r.to_string()));
        }

        if where_clause.is_empty() {
            None
        } else {
            Some(serde_json::to_value(where_clause).unwrap())
        }
    }

    pub fn format_search_results(
        query: &str,
        wing: Option<&String>,
        room: Option<&String>,
        docs: &[String],
        metas: &[Option<serde_json::Map<String, serde_json::Value>>],
        dists: &[f32],
    ) -> String {
        if docs.is_empty() || docs[0].is_empty() {
            return format!("\n  No results found for: \"{}\"", query);
        }

        let mut output = String::new();
        output.push_str(&format!("\n{}", "=".repeat(60)));
        output.push_str(&format!("\n  Results for: \"{}\"", query));
        if let Some(w) = &wing {
            output.push_str(&format!("\n  Wing: {}", w));
        }
        if let Some(r) = &room {
            output.push_str(&format!("\n  Room: {}", r));
        }
        output.push_str(&format!("\n{}\n", "=".repeat(60)));

        for i in 0..docs.len() {
            let doc = &docs[i];
            let meta = &metas[i];
            let dist = dists[i];

            let similarity = 1.0 - dist;
            let wing_name = meta
                .as_ref()
                .and_then(|m: &serde_json::Map<String, serde_json::Value>| m.get("wing"))
                .and_then(|v: &serde_json::Value| v.as_str())
                .unwrap_or("?");
            let room_name = meta
                .as_ref()
                .and_then(|m: &serde_json::Map<String, serde_json::Value>| m.get("room"))
                .and_then(|v: &serde_json::Value| v.as_str())
                .unwrap_or("?");
            let source = meta
                .as_ref()
                .and_then(|m: &serde_json::Map<String, serde_json::Value>| m.get("source_file"))
                .and_then(|v: &serde_json::Value| v.as_str())
                .unwrap_or("");
            let source_name = PathBuf::from(source)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();

            output.push_str(&format!("\n  [{}] {} / {}", i + 1, wing_name, room_name));
            output.push_str(&format!("\n      Source: {}", source_name));
            output.push_str(&format!("\n      Match:  {:.3}\n", similarity));

            let trimmed = doc.trim();
            for line in trimmed.split('\n') {
                output.push_str(&format!("\n      {}", line));
            }
            output.push('\n');
            output.push_str(&format!("\n  {}", "─".repeat(56)));
        }

        output
    }

    pub async fn search(
        &self,
        query: &str,
        wing: Option<String>,
        room: Option<String>,
        n_results: usize,
    ) -> Result<String> {
        // Use pure-Rust VectorStorage (lazy initialization)
        let Some(store) = self.open_vector_storage() else {
            return Ok(format!(
                "\n  No results found for: \"{}\" (vector storage unavailable)",
                query
            ));
        };

        // Use search_room for pre-filtered search if wing+room provided, else global search
        let records = match (&wing, &room) {
            (Some(w), Some(r)) => store.search_room(query, w, r, n_results, None)?,
            _ => store.search(query, n_results)?,
        };

        if records.is_empty() {
            return Ok(format!("\n  No results found for: \"{}\"", query));
        }

        // Convert records to legacy format for display
        let docs: Vec<String> = records.iter().map(|r| r.text_content.clone()).collect();
        let metas: Vec<Option<serde_json::Map<String, serde_json::Value>>> = records
            .iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                m.insert(
                    "wing".to_string(),
                    serde_json::Value::String(r.wing.clone()),
                );
                m.insert(
                    "room".to_string(),
                    serde_json::Value::String(r.room.clone()),
                );
                m.insert(
                    "valid_from".to_string(),
                    serde_json::Value::Number(r.valid_from.into()),
                );
                Some(m)
            })
            .collect();
        let dists: Vec<f32> = records.iter().map(|r| 1.0 - r.score).collect();

        let output =
            Self::format_search_results(query, wing.as_ref(), room.as_ref(), &docs, &metas, &dists);
        Ok(output)
    }

    pub fn format_json_results(
        query: &str,
        wing: Option<&String>,
        room: Option<&String>,
        docs: &[String],
        metas: &[Option<serde_json::Map<String, serde_json::Value>>],
        dists: &[f32],
    ) -> serde_json::Value {
        let mut hits = Vec::new();
        if !docs.is_empty() && !docs[0].is_empty() {
            for i in 0..docs.len() {
                hits.push(serde_json::json!({
                    "text": docs[i],
                    "wing": metas[i].as_ref().and_then(|m: &serde_json::Map<String, serde_json::Value>| m.get("wing")).and_then(|v: &serde_json::Value| v.as_str()).unwrap_or("unknown"),
                    "room": metas[i].as_ref().and_then(|m: &serde_json::Map<String, serde_json::Value>| m.get("room")).and_then(|v: &serde_json::Value| v.as_str()).unwrap_or("unknown"),
                    "source_file": PathBuf::from(metas[i].as_ref().and_then(|m: &serde_json::Map<String, serde_json::Value>| m.get("source_file")).and_then(|v: &serde_json::Value| v.as_str()).unwrap_or("?")).file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    "similarity": 1.0 - dists[i]
                }));
            }
        }

        serde_json::json!({
            "query": query,
            "filters": {
                "wing": wing,
                "room": room
            },
            "results": hits
        })
    }

    pub async fn search_memories(
        &self,
        query: &str,
        wing: Option<String>,
        room: Option<String>,
        n_results: usize,
    ) -> Result<serde_json::Value> {
        // Use pure-Rust VectorStorage (lazy initialization)
        let Some(store) = self.open_vector_storage() else {
            return Ok(Self::format_json_results(
                query,
                wing.as_ref(),
                room.as_ref(),
                &[],
                &[],
                &[],
            ));
        };

        let records = match (&wing, &room) {
            (Some(w), Some(r)) => store.search_room(query, w, r, n_results, None)?,
            _ => store.search(query, n_results)?,
        };

        let docs: Vec<String> = records.iter().map(|r| r.text_content.clone()).collect();
        let metas: Vec<Option<serde_json::Map<String, serde_json::Value>>> = records
            .iter()
            .map(|r| {
                let mut m = serde_json::Map::new();
                m.insert(
                    "wing".to_string(),
                    serde_json::Value::String(r.wing.clone()),
                );
                m.insert(
                    "room".to_string(),
                    serde_json::Value::String(r.room.clone()),
                );
                m.insert("id".to_string(), serde_json::Value::Number(r.id.into()));
                Some(m)
            })
            .collect();
        let dists: Vec<f32> = records.iter().map(|r| 1.0 - r.score).collect();

        Ok(Self::format_json_results(
            query,
            wing.as_ref(),
            room.as_ref(),
            &docs,
            &metas,
            &dists,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_searcher_new() {
        let config = MempalaceConfig::default();
        let searcher = Searcher::new(config);
        assert_eq!(
            searcher.config.collection_name,
            MempalaceConfig::default().collection_name
        );
    }

    #[test]
    fn test_format_search_results_empty() {
        let res = Searcher::format_search_results("hello", None, None, &[], &[], &[]);
        assert!(res.contains("No results found for: \"hello\""));

        let res2 =
            Searcher::format_search_results("world", None, None, &[String::new()], &[None], &[0.0]);
        assert!(res2.contains("No results found for: \"world\""));
    }

    #[test]
    fn test_format_search_results_with_data() {
        let docs = vec!["this is a test document".to_string()];

        let mut meta1 = serde_json::Map::new();
        meta1.insert(
            "wing".to_string(),
            serde_json::Value::String("engineering".to_string()),
        );
        meta1.insert(
            "room".to_string(),
            serde_json::Value::String("rust".to_string()),
        );
        meta1.insert(
            "source_file".to_string(),
            serde_json::Value::String("/path/to/some/file.txt".to_string()),
        );
        let metas = vec![Some(meta1)];
        let dists = vec![0.1_f32];

        let wing = Some("engineering".to_string());
        let room = Some("rust".to_string());

        let res = Searcher::format_search_results(
            "test",
            wing.as_ref(),
            room.as_ref(),
            &docs,
            &metas,
            &dists,
        );

        assert!(res.contains("Results for: \"test\""));
        assert!(res.contains("Wing: engineering"));
        assert!(res.contains("Room: rust"));
        assert!(res.contains("[1] engineering / rust"));
        assert!(res.contains("Source: file.txt"));
        assert!(res.contains("Match:  0.900"));
        assert!(res.contains("this is a test document"));
    }

    #[test]
    fn test_format_search_results_missing_metadata() {
        let docs = vec!["missing meta".to_string()];
        let metas = vec![None];
        let dists = vec![0.5_f32];

        let res = Searcher::format_search_results("meta", None, None, &docs, &metas, &dists);
        assert!(res.contains("[1] ? / ?"));
        assert!(res.contains("Source: ?"));
    }

    #[test]
    fn test_format_json_results_empty() {
        let res = Searcher::format_json_results("hello", None, None, &[], &[], &[]);
        assert_eq!(res["query"], "hello");
        assert!(res["results"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_format_json_results_with_data() {
        let docs = vec!["this is a json doc".to_string()];

        let mut meta1 = serde_json::Map::new();
        meta1.insert(
            "wing".to_string(),
            serde_json::Value::String("ops".to_string()),
        );
        meta1.insert(
            "room".to_string(),
            serde_json::Value::String("general".to_string()),
        );
        meta1.insert(
            "source_file".to_string(),
            serde_json::Value::String("/another/path/docs.md".to_string()),
        );
        let metas = vec![Some(meta1)];
        let dists = vec![0.2_f32];

        let wing = Some("ops".to_string());

        let res = Searcher::format_json_results("json", wing.as_ref(), None, &docs, &metas, &dists);

        assert_eq!(res["query"], "json");
        assert_eq!(res["filters"]["wing"], "ops");
        assert_eq!(res["filters"]["room"], serde_json::Value::Null);

        let results = res["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        let first = &results[0];
        assert_eq!(first["text"], "this is a json doc");
        assert_eq!(first["wing"], "ops");
        assert_eq!(first["room"], "general");
        assert_eq!(first["source_file"], "docs.md");

        // Due to f32 float precision 1.0 - 0.2 might be 0.800000011920929
        let sim = first["similarity"].as_f64().unwrap();
        assert!((sim - 0.8).abs() < 0.0001);
    }

    #[test]
    fn test_format_json_results_missing_metadata() {
        let docs = vec!["no meta doc".to_string()];
        let metas = vec![None];
        let dists = vec![0.0_f32];

        let res = Searcher::format_json_results("missing", None, None, &docs, &metas, &dists);
        let results = res["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        let first = &results[0];
        assert_eq!(first["wing"], "unknown");
        assert_eq!(first["room"], "unknown");
        assert_eq!(first["source_file"], "?");
    }

    #[test]
    fn test_build_where_clause_empty() {
        let res = Searcher::build_where_clause(None, None);
        assert_eq!(res, None);
    }

    #[test]
    fn test_build_where_clause_wing_only() {
        let wing = "engineering".to_string();
        let res = Searcher::build_where_clause(Some(&wing), None).unwrap();
        assert_eq!(res["wing"], "engineering");
    }

    #[test]
    fn test_build_where_clause_room_only() {
        let room = "rust".to_string();
        let res = Searcher::build_where_clause(None, Some(&room)).unwrap();
        assert_eq!(res["room"], "rust");
    }

    #[test]
    fn test_build_where_clause_wing_and_room() {
        let wing = "engineering".to_string();
        let room = "rust".to_string();
        let res = Searcher::build_where_clause(Some(&wing), Some(&room)).unwrap();

        let and_arr = res["$and"].as_array().unwrap();
        assert_eq!(and_arr.len(), 2);

        let mut has_wing = false;
        let mut has_room = false;

        for item in and_arr {
            if item.get("wing").is_some() {
                assert_eq!(item["wing"], "engineering");
                has_wing = true;
            }
            if item.get("room").is_some() {
                assert_eq!(item["room"], "rust");
                has_room = true;
            }
        }
        assert!(has_wing);
        assert!(has_room);
    }

    #[tokio::test]
    async fn test_search_graceful_when_unavailable() {
        let config = MempalaceConfig::default();
        let searcher = Searcher::new(config);

        let res = searcher.search("query", None, None, 5).await;
        assert!(res.is_ok());

        let res2 = searcher.search_memories("query", None, None, 5).await;
        assert!(res2.is_ok());
    }

    #[test]
    fn test_format_search_results_multiline_doc() {
        let docs = vec!["line 1\nline 2\nline 3".to_string()];
        let metas = vec![None];
        let dists = vec![0.1_f32];

        let res = Searcher::format_search_results("multi", None, None, &docs, &metas, &dists);
        assert!(res.contains("line 1"));
        assert!(res.contains("line 2"));
        assert!(res.contains("line 3"));
    }

    #[test]
    fn test_format_search_results_empty_pure() {
        assert!(
            Searcher::format_search_results("none", None, None, &[], &[], &[])
                .contains("No results found")
        );
    }

    #[test]
    fn test_format_json_results_empty_pure() {
        let res = Searcher::format_json_results("none", None, None, &[], &[], &[]);
        assert_eq!(res["results"].as_array().unwrap().len(), 0);
    }
}
