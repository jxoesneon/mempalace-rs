// Simple script to download the fastembed model
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

fn main() {
    println!("Downloading AllMiniLML6V2 model...");
    let _ = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true),
    )
    .expect("Failed to download model");
    println!("Model downloaded successfully!");
}
