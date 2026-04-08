use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub name: String,
    pub score: f64,
    pub metric_name: String,
    pub latency_ms: f64,
    pub tokens_used: usize,
    pub metadata: std::collections::HashMap<String, String>,
}

type GoldStandardRow = (String, String, String, String);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Collect Criterion micro-benchmarks
    let mut micro_benchmarks = Vec::new();
    let criterion_dir = Path::new("target/criterion");
    if criterion_dir.exists() {
        let targets = [
            ("aaak_compression", "AAAK Compression"),
            ("entity_detection", "Entity Detection"),
            ("token_counting", "Token Counting"),
            ("compression_stats", "Compression Stats"),
        ];

        for (id_str, display_name) in targets.iter() {
            let estimates_path = criterion_dir
                .join(id_str)
                .join("new")
                .join("estimates.json");
            if !estimates_path.exists() {
                continue;
            }

            let content = fs::read_to_string(estimates_path)?;
            let json: Value = serde_json::from_str(&content)?;

            let mean_ns = json["mean"]["point_estimate"].as_f64().unwrap_or(0.0);

            let (latency_str, throughput_ops) = if mean_ns < 1000.0 {
                (format!("{:.0} ns", mean_ns), (1_000_000_000.0 / mean_ns))
            } else if mean_ns < 1_000_000.0 {
                (
                    format!("{:.0} µs", mean_ns / 1000.0),
                    (1_000_000.0 / (mean_ns / 1000.0)),
                )
            } else {
                (
                    format!("{:.1} ms", mean_ns / 1_000_000.0),
                    (1000.0 / (mean_ns / 1_000_000.0)),
                )
            };

            let throughput_str = format!("~{} ops/sec", throughput_ops as u64);
            micro_benchmarks.push((display_name.to_string(), throughput_str, latency_str));
        }
    }

    // 2. Collect 2026 Gold Standard Benchmarks
    let gold_results = run_gold_standard_benchmarks()?;

    // 3. Update Documentation
    update_markdown_all("README.md", &micro_benchmarks, &gold_results)?;
    update_markdown_all("benchmarks/README.md", &micro_benchmarks, &gold_results)?;

    Ok(())
}

fn run_gold_standard_benchmarks() -> Result<Vec<GoldStandardRow>, Box<dyn std::error::Error>> {
    let mut results = Vec::new();

    // We run each benchmark and capture the result
    // In CI, we expect the binaries to be built already
    let commands = [
        ("ruler", vec!["benchmark", "ruler", "--k", "10"]),
        ("structmem", vec!["benchmark", "structmem", "--hints"]),
        (
            "babilong",
            vec!["benchmark", "babilong", "--tokens", "1000000"],
        ),
        ("beam", vec!["benchmark", "beam"]),
    ];

    for (name, args) in commands {
        println!("Running 2026 Gold Standard: {}...", name);
        // Note: In this simulation we assume the binary is target/debug/mempalace-rs
        // In CI it might be target/release/mempalace-rs
        let binary = if Path::new("target/release/mempalace-rs").exists() {
            "target/release/mempalace-rs"
        } else {
            "target/debug/mempalace-rs"
        };

        let output = Command::new(binary).args(args).output()?;

        if !output.status.success() {
            println!("Warning: Benchmark {} failed", name);
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Find the score and metric from the pretty-printed output
        // (A more robust way would be adding a --json flag to all benchmarks,
        // but for now we parse the pretty output as requested)

        let mut score = "0.000".to_string();
        let mut metric = "N/A".to_string();
        let mut latency = "0.0ms".to_string();

        for line in stdout.lines() {
            if line.contains("Overall Score:") {
                score = line.split(':').nth(1).unwrap_or("0.000").trim().to_string();
            } else if line.contains("Latency:") {
                latency = line.split(':').nth(1).unwrap_or("0.0ms").trim().to_string();
            } else if name == "ruler" && line.contains("RULER Results") {
                metric = "nDCG".to_string();
            } else if name == "structmem" && line.contains("StructMemEval Results") {
                metric = "Structural".to_string();
            } else if name == "babilong" && line.contains("BABILong Results") {
                metric = "Reasoning".to_string();
            } else if name == "beam" && line.contains("BEAM Results") {
                metric = "Nugget".to_string();
            }
        }

        results.push((name.to_uppercase(), score, metric, latency));
    }

    Ok(results)
}

fn update_markdown_all(
    path: &str,
    micro: &[(String, String, String)],
    gold: &[(String, String, String, String)],
) -> Result<(), Box<dyn std::error::Error>> {
    if !Path::new(path).exists() {
        return Ok(());
    }

    let mut content = fs::read_to_string(path)?;

    // Update Micro benchmarks
    content = replace_between(
        &content,
        "<!-- BENCH_TABLE_START -->",
        "<!-- BENCH_TABLE_END -->",
        &generate_micro_table(micro),
    );

    // Update Gold Standard benchmarks
    content = replace_between(
        &content,
        "<!-- GOLD_STANDARD_START -->",
        "<!-- GOLD_STANDARD_END -->",
        &generate_gold_table(gold),
    );

    fs::write(path, content)?;
    println!("Updated {}", path);
    Ok(())
}

fn replace_between(content: &str, start_marker: &str, end_marker: &str, new_table: &str) -> String {
    let start_idx = match content.find(start_marker) {
        Some(idx) => idx + start_marker.len(),
        None => return content.to_string(),
    };
    let end_idx = match content.find(end_marker) {
        Some(idx) => idx,
        None => return content.to_string(),
    };

    let mut updated = content[..start_idx].to_string();
    updated.push_str(new_table);
    updated.push_str(&content[end_idx..]);
    updated
}

fn generate_micro_table(benches: &[(String, String, String)]) -> String {
    let mut table = String::from("\n| Operation          | Throughput        | Latency |\n|--------------------|-------------------|---------|\n");
    for (name, throughput, latency) in benches {
        table.push_str(&format!(
            "| {:<18} | {:<17} | {:<7} |\n",
            name, throughput, latency
        ));
    }
    table
}

fn generate_gold_table(benches: &[(String, String, String, String)]) -> String {
    let mut table = String::from(
        "\n| Benchmark | Score | Metric | Latency |\n|-----------|-------|--------|---------|\n",
    );
    for (name, score, metric, lat) in benches {
        table.push_str(&format!(
            "| **{:<10}** | {:<5} | {:<10} | {:<7} |\n",
            name, score, metric, lat
        ));
    }
    table
}
