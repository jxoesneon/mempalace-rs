use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct NativeBenchResult {
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub ndcg_at_10: f64,
    pub total_time_secs: f64,
    pub avg_ms_per_query: f64,
}

type NativeReportRow = (String, String, String, String);

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

    // 2. Collect Native AAAK Evolution Benchmarks (Speed & Accuracy)
    let native_results = run_native_benchmarks()?;

    // 3. Update Documentation
    update_markdown_all("README.md", &micro_benchmarks, &native_results)?;
    update_markdown_all("benchmarks/README.md", &micro_benchmarks, &native_results)?;

    Ok(())
}

fn run_native_benchmarks() -> Result<Vec<NativeReportRow>, Box<dyn std::error::Error>> {
    let mut results = Vec::new();
    let fixture_path = "tests/fixtures/longmemeval_ci.json";

    // Check if binary and fixture exist
    if !Path::new(fixture_path).exists() {
        println!(
            "Skipping native benchmarks: fixture not found at {}",
            fixture_path
        );
        return Ok(results);
    }

    let modes = ["raw", "aaak"];
    for mode in &modes {
        println!("Running native benchmark for mode: {}...", mode);
        let output = Command::new("cargo")
            .args([
                "run",
                "--quiet",
                "--",
                "benchmark",
                "longmemeval",
                fixture_path,
                "--mode",
                mode,
                "--json",
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!(
                "Warning: Native benchmark for {} failed: {}",
                mode,
                stderr.trim()
            );
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Find the line that looks like JSON
        let json_line = stdout
            .lines()
            .find(|line| line.trim().starts_with('{') && line.trim().ends_with('}'));

        if let Some(json_str) = json_line {
            if let Ok(res) = serde_json::from_str::<NativeBenchResult>(json_str.trim()) {
                results.push((
                    mode.to_uppercase(),
                    format!("{:.1}%", res.recall_at_5 * 100.0),
                    format!("{:.1}%", res.recall_at_10 * 100.0),
                    format!("{:.1}ms", res.avg_ms_per_query),
                ));
            } else {
                println!("Warning: Failed to parse JSON for {}: {}", mode, json_str);
            }
        } else {
            println!(
                "Warning: No JSON found in output for {}. Stdout: {}",
                mode, stdout
            );
        }
    }

    Ok(results)
}

fn update_markdown_all(
    path: &str,
    micro: &[(String, String, String)],
    native: &[(String, String, String, String)],
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

    // Update Native benchmarks
    content = replace_between(
        &content,
        "<!-- ACCURACY_TABLE_START -->",
        "<!-- ACCURACY_TABLE_END -->",
        &generate_native_table(native),
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

fn generate_native_table(benches: &[(String, String, String, String)]) -> String {
    let mut table = String::from("\n| Mode | Recall@5 | Recall@10 | Latency/Query |\n|------|----------|-----------|---------------|\n");
    for (mode, r5, r10, lat) in benches {
        table.push_str(&format!(
            "| {:<4} | {:<8} | {:<9} | {:<13} |\n",
            mode, r5, r10, lat
        ));
    }
    table
}
