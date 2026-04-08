use anyhow::Result;
use clap::{Parser, Subcommand};
use mempalace_rs::knowledge_graph::KnowledgeGraph;
use mempalace_rs::searcher::Searcher;
use mempalace_rs::storage::Storage;

use mempalace_rs::config::MempalaceConfig;

#[derive(Parser)]
#[command(
    name = "mempalace",
    about = "Give your AI a memory. No API key required."
)]
struct Cli {
    #[arg(
        short,
        long,
        help = "Where the palace lives (default: from ~/.mempalace/config.json or ~/.mempalace/palace)"
    )]
    palace: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Detect rooms from your folder structure")]
    Init { dir: String },
    #[command(about = "Mine files into the palace")]
    Mine {
        dir: String,
        #[arg(short, long, help = "Ingest mode: project or convos")]
        mode: Option<String>,
        #[arg(short, long, help = "Wing to tag with")]
        wing: Option<String>,
    },
    #[command(about = "Find anything, exact words")]
    Search { query: String },
    #[command(about = "Compress drawers using AAAK Dialect (~30x reduction)")]
    Compress {
        #[arg(short, long)]
        wing: Option<String>,
    },
    #[command(about = "Show L0 + L1 wake-up context (~600-900 tokens)")]
    Wakeup {
        #[arg(short, long)]
        wing: Option<String>,
    },
    #[command(about = "Split concatenated transcript mega-files into per-session files")]
    Split { dir: String },
    #[command(about = "Show what has been filed")]
    Status,
    #[command(about = "Semantic deduplication. Merges similar memories.")]
    Prune {
        #[arg(short, long, default_value_t = 0.85)]
        threshold: f32,
        #[arg(short, long)]
        dry_run: bool,
        #[arg(short, long)]
        wing: Option<String>,
    },
    #[command(name = "mcp-server", about = "Run the MCP server over stdio")]
    McpServer,
    #[command(about = "Run evaluations and benchmarks")]
    Benchmark {
        #[command(subcommand)]
        bench_type: BenchCommands,
    },
}

#[derive(Subcommand)]
pub enum BenchCommands {
    #[command(about = "Run LongMemEval dataset")]
    Longmemeval {
        path: String,
        #[arg(short, long, default_value = "raw")]
        mode: String,
        #[arg(short, long, help = "Output results as JSON")]
        json: bool,
    },
}

async fn run_app(cli: Cli) -> Result<()> {
    if std::env::var("MEMPALACE_TEST_MODE").is_ok() {
        return Ok(());
    }

    let mut config = MempalaceConfig::default();
    if let Some(p) = cli.palace {
        config.palace_path = p;
    }

    // Determine storage database path
    let p_path = std::path::Path::new(&config.palace_path);
    let storage_path = if p_path.is_dir() {
        p_path.join("palace.db").to_string_lossy().to_string()
    } else {
        config.palace_path.clone()
    };

    let storage = Storage::new(&storage_path)?;
    let searcher = Searcher::new(config.clone());
    let kg_path = config.config_dir.join("knowledge.db");
    let _kg = KnowledgeGraph::new(kg_path.to_str().unwrap_or("knowledge.db"))?;

    match cli.command {
        Commands::Init { dir: _ } => {
            mempalace_rs::onboarding::run_onboarding()?;
        }
        Commands::Mine { dir, mode, wing } => {
            let m = mode.unwrap_or_else(|| "project".to_string());
            if m == "convos" {
                mempalace_rs::convo_miner::mine_convos(&dir, &storage, &config, wing.as_deref())
                    .await?;
            } else {
                mempalace_rs::miner::mine_project(&dir, &storage, &config, wing.as_deref()).await?;
            }
            println!("  ✓ Successfully mined {} in {} mode", dir, m);
        }
        Commands::Search { query } => {
            let result = searcher.search(&query, None, None, 5).await?;
            println!("{}", result);
        }
        Commands::Compress { wing } => {
            storage.compress_drawers(&config, wing).await?;
        }
        Commands::Wakeup { wing } => {
            let result = searcher.wake_up(wing).await?;
            println!("{}", result);
        }
        Commands::Split { dir } => {
            let path = std::path::Path::new(&dir);
            if path.is_file() {
                mempalace_rs::split_mega_files::split_mega_file(
                    path,
                    path.parent().unwrap_or(path),
                )?;
                println!("  ✓ Successfully split {}", dir);
            } else if path.is_dir() {
                let mut count = 0;
                for entry in std::fs::read_dir(path)? {
                    let entry = entry?;
                    let p = entry.path();
                    if p.is_file()
                        && mempalace_rs::split_mega_files::split_mega_file(&p, path).is_ok()
                    {
                        count += 1;
                    }
                }
                println!("  ✓ Successfully split {} mega-files in {}", count, dir);
            }
        }
        Commands::Status => {
            storage.status(&config).await?;
        }
        Commands::Prune {
            threshold,
            dry_run,
            wing,
        } => {
            let report = storage
                .prune_memories(&config, threshold, dry_run, wing)
                .await?;
            println!("\n  🧹 Semantic Pruning Complete");
            println!("  {}", "─".repeat(28));
            println!("  Clusters found:      {}", report.clusters_found);
            println!("  Memories merged:     {}", report.merged);
            println!("  Est. tokens saved:   {}", report.tokens_saved_est);
            if dry_run {
                println!("\n  [DRY RUN] No changes were made to the database.");
            }
            println!();
        }
        Commands::McpServer => {
            mempalace_rs::mcp_server::run_mcp_server().await?;
        }
        Commands::Benchmark { bench_type } => match bench_type {
            BenchCommands::Longmemeval { path, mode, json } => {
                let result =
                    mempalace_rs::benchmark::run_longmemeval(std::path::Path::new(&path), &mode)
                        .await?;
                if json {
                    println!("{}", serde_json::to_string(&result)?);
                } else {
                    println!("\n  📊 LongMemEval Benchmark Results ({})", mode);
                    println!("  {}", "─".repeat(45));
                    println!("  Recall@5:  {:.3}", result.recall_at_5);
                    println!("  Recall@10: {:.3}", result.recall_at_10);
                    println!("  NDCG@10:   {:.3}", result.ndcg_at_10);
                    println!(
                        "  Time:      {:.1}s ({:.1} ms/query)",
                        result.total_time_secs, result.avg_ms_per_query
                    );
                    println!();
                }
            }
        },
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    run_app(cli).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        std::env::set_var("MEMPALACE_TEST_MODE", "1");
    }

    #[tokio::test]
    async fn test_main_status() {
        setup();
        let cli = Cli::parse_from(["mempalace", "status"]);
        assert!(run_app(cli).await.is_ok());
    }

    #[tokio::test]
    async fn test_main_init() {
        setup();
        let cli = Cli::parse_from(["mempalace", "init", "/tmp"]);
        assert!(run_app(cli).await.is_ok());
    }

    #[tokio::test]
    async fn test_main_search() {
        setup();
        let cli = Cli::parse_from(["mempalace", "search", "test"]);
        assert!(run_app(cli).await.is_ok());
    }

    #[tokio::test]
    async fn test_main_compress() {
        setup();
        let cli = Cli::parse_from(["mempalace", "compress", "--wing", "test"]);
        assert!(run_app(cli).await.is_ok());
    }

    #[tokio::test]
    async fn test_main_wakeup() {
        setup();
        let cli = Cli::parse_from(["mempalace", "wakeup"]);
        assert!(run_app(cli).await.is_ok());
    }

    #[tokio::test]
    async fn test_main_split() {
        setup();
        let cli = Cli::parse_from(["mempalace", "split", "/tmp"]);
        assert!(run_app(cli).await.is_ok());
    }

    #[tokio::test]
    async fn test_main_mine() {
        setup();
        let cli = Cli::parse_from(["mempalace", "mine", "/tmp", "--mode", "project"]);
        assert!(run_app(cli).await.is_ok());
    }
}
