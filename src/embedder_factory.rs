use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use lazy_static::lazy_static;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

lazy_static! {
    static ref GLOBAL_EMBEDDER: Mutex<Option<Arc<TextEmbedding>>> = Mutex::new(None);
}

pub struct EmbedderFactory;

impl EmbedderFactory {
    /// Returns a cloned Arc to the global singleton TextEmbedding engine.
    /// Thread-safe and initialized exactly once per process.
    pub fn get_embedder() -> Result<Arc<TextEmbedding>> {
        let mut guard = GLOBAL_EMBEDDER.lock().unwrap();

        if let Some(emb) = &*guard {
            return Ok(emb.clone());
        }

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

        let emb = match TextEmbedding::try_new(init_opts) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[WARN] mempalace: failed to initialise fastembed embedder: {e}");
                return Err(anyhow::anyhow!("Failed to initialise fastembed: {e}"));
            }
        };

        let arc_emb = Arc::new(emb);
        *guard = Some(arc_emb.clone());
        Ok(arc_emb)
    }
}
