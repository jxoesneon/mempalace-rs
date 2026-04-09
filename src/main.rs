use anyhow::Result;
use clap::{Parser, Subcommand};
use mempalace_rs::benchmarks::babilong::Babilong;
use mempalace_rs::benchmarks::beam::BeamBenchmark;
use mempalace_rs::benchmarks::judge::MockJudge;
use mempalace_rs::benchmarks::ruler::Ruler;
use mempalace_rs::benchmarks::struct_mem::StructMemEval;
use mempalace_rs::benchmarks::Benchmark;
use mempalace_rs::config::MempalaceConfig;
use mempalace_rs::knowledge_graph::KnowledgeGraph;
use mempalace_rs::searcher::Searcher;
use mempalace_rs::storage::Storage;
use mempalace_rs::vector_storage::VectorStorage;

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
        #[arg(long, help = "Disable gitignore filtering")]
        no_gitignore: bool,
        #[arg(long, help = "Pass paths that bypass the filter")]
        include_ignored: bool,
        #[arg(short, long, help = "Override the author metadata")]
        agent: Option<String>,
        #[arg(short, long, help = "Stop mining after N files")]
        limit: Option<usize>,
        #[arg(long, help = "Log to stdout instead of writing to storage")]
        dry_run: bool,
    },
    #[command(about = "Find anything, exact words")]
    Search {
        query: String,
        #[arg(short, long, help = "Filter by wing")]
        wing: Option<String>,
        #[arg(short, long, help = "Filter by room")]
        room: Option<String>,
        #[arg(short, long, help = "Number of results", default_value_t = 5)]
        results: usize,
    },
    #[command(
        about = "Iterate over all entries in palace.db and re-index them into vector storage"
    )]
    Repair,
    #[command(about = "Output system prompts for init, search, mine, and status")]
    Instructions,
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
    #[command(about = "Run RULER benchmark (Multi-Needle & Aggregation)")]
    Ruler {
        #[arg(short, long, default_value_t = 10)]
        k: usize,
    },
    #[command(about = "Run StructMemEval benchmark")]
    Structmem {
        #[arg(long, help = "Use memory organization hints")]
        hints: bool,
    },
    #[command(about = "Run BABILong ultra-long context benchmark")]
    Babilong {
        #[arg(short, long, default_value_t = 1000000)]
        tokens: usize,
    },
    #[command(about = "Run BEAM agentic memory benchmark")]
    Beam,
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
        Commands::Mine {
            dir,
            mode,
            wing,
            no_gitignore,
            include_ignored: _,
            agent,
            limit,
            dry_run,
        } => {
            let m = mode.unwrap_or_else(|| "project".to_string());
            if m == "convos" {
                mempalace_rs::convo_miner::mine_convos(&dir, &storage, &config, wing.as_deref())
                    .await?;
            } else {
                let options = mempalace_rs::miner::MineOptions {
                    wing_override: wing,
                    no_gitignore,
                    agent,
                    limit,
                    dry_run,
                };
                mempalace_rs::miner::mine_project(&dir, &storage, &config, options).await?;
            }
            println!("  ✓ Successfully mined {} in {} mode", dir, m);
        }
        Commands::Search {
            query,
            wing,
            room,
            results,
        } => {
            let result = searcher.search(&query, wing, room, results).await?;
            println!("{}", result);
        }
        Commands::Repair => {
            storage.repair(&config).await?;
        }
        Commands::Instructions => {
            let instructions = r#"
System Prompts for AI Agents
============================

INIT:
You are an AI with a structured memory palace. Use `mempalace_init` or `mempalace_add_drawer` to store critical information you need to remember across sessions.

SEARCH:
Use `mempalace_search` to query your temporal knowledge graph and vector database. Rely on exact keywords when possible.

MINE:
To ingest large codebases or transcripts, trigger `mempalace mine <DIR>`.

STATUS:
Call `mempalace_status` periodically to understand your context window budget and the current AAAK compression ratio.
            "#;
            println!("{}", instructions.trim());
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
                    2,
                )?;
                println!("  ✓ Successfully split {}", dir);
            } else if path.is_dir() {
                let mut count = 0;
                for entry in std::fs::read_dir(path)? {
                    let entry = entry?;
                    let p = entry.path();
                    if p.is_file()
                        && mempalace_rs::split_mega_files::split_mega_file(&p, path, 2).is_ok()
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
        Commands::Benchmark { bench_type } => {
            let temp_dir = tempfile::tempdir()?;
            let db_path = temp_dir.path().join("bench.db");
            let index_path = temp_dir.path().join("bench.index");

            let mut storage = VectorStorage::new(&db_path, &index_path)?;

            let (bench, name): (Box<dyn Benchmark>, String) = match bench_type {
                BenchCommands::Ruler { k } => (Box::new(Ruler::new(k)), "RULER".into()),
                BenchCommands::Structmem { hints } => {
                    (Box::new(StructMemEval::new(hints)), "StructMemEval".into())
                }
                BenchCommands::Babilong { tokens } => {
                    (Box::new(Babilong::new(tokens)), "BABILong".into())
                }
                BenchCommands::Beam => (
                    Box::new(BeamBenchmark::new(Box::new(MockJudge))),
                    "BEAM".into(),
                ),
            };

            println!("\n  🚀 Running Benchmark: {}", name);
            let result = bench.run(&mut storage).await?;

            println!("\n  📊 {} Results", name);
            println!("  {}", "─".repeat(45));
            println!("  Overall Score: {:.3}", result.score);
            println!("  Latency:       {:.1} ms", result.latency_ms);
            if result.tokens_used > 0 {
                println!("  Tokens:        {}", result.tokens_used);
            }
            for (k, v) in &result.metadata {
                println!("  {}: {}", k, v);
            }
            println!();
        }
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
}
