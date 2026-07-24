// Simple script to download the fastembed model
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use mempalace_rs::config::home_dir;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn main() {
    let cache_dir = PathBuf::from(home_dir()).join(".fastembed_cache");
    println!(
        "Downloading AllMiniLML6V2 model to {}...",
        cache_dir.display()
    );
    let mut opts =
        InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true);
    opts = opts.with_cache_dir(cache_dir);

    let mut attempts = 0;
    let max_attempts = 5;
    let mut delay = Duration::from_secs(2);

    loop {
        attempts += 1;
        match TextEmbedding::try_new(opts.clone()) {
            Ok(_) => {
                println!("Model downloaded successfully!");
                break;
            }
            Err(e) => {
                if attempts >= max_attempts {
                    panic!(
                        "Failed to download model after {} attempts: {}",
                        max_attempts, e
                    );
                }
                println!(
                    "Attempt {} failed: {}. Retrying in {:?}...",
                    attempts, e, delay
                );
                thread::sleep(delay);
                delay *= 2;
            }
        }
    }
}
