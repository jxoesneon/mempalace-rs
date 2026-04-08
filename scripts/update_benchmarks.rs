use serde_json::Value;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let criterion_dir = Path::new("target/criterion");
    if !criterion_dir.exists() {
        println!("target/criterion not found. Run 'cargo bench' first.");
        return Ok(());
    }

    let mut benchmarks = Vec::new();

    // Bench IDs present in benches/benchmarks.rs
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
        benchmarks.push((display_name.to_string(), throughput_str, latency_str));
    }

    update_markdown("README.md", &benchmarks)?;
    update_markdown("benchmarks/README.md", &benchmarks)?;

    Ok(())
}

fn update_markdown(
    path: &str,
    benches: &[(String, String, String)],
) -> Result<(), Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let start_marker = "<!-- BENCH_TABLE_START -->";
    let end_marker = "<!-- BENCH_TABLE_END -->";

    let start_idx = match content.find(start_marker) {
        Some(idx) => idx + start_marker.len(),
        None => return Ok(()),
    };
    let end_idx = match content.find(end_marker) {
        Some(idx) => idx,
        None => return Ok(()),
    };

    let mut new_table = String::from("\n| Operation          | Throughput        | Latency |\n|--------------------|-------------------|---------|\n");
    for (name, throughput, latency) in benches {
        new_table.push_str(&format!(
            "| {:<18} | {:<17} | {:<7} |\n",
            name, throughput, latency
        ));
    }

    let mut updated_content = content[..start_idx].to_string();
    updated_content.push_str(&new_table);
    updated_content.push_str(&content[end_idx..]);

    fs::write(path, updated_content)?;
    println!("Updated {}", path);
    Ok(())
}
