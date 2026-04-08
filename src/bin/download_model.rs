// Simple script to download the fastembed model
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

fn main() {
    println!("Downloading AllMiniLML6V2 model...");
    let mut opts =
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true);
    opts = opts.with_cache_dir(std::path::PathBuf::from("models"));
    let _ = TextEmbedding::try_new(opts).expect("Failed to download model");
    println!("Model downloaded successfully!");
}
