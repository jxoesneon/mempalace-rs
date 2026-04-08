# MemPalace-rs Benchmarks

Performance benchmarks for the Rust port of MemPalace.

## Micro-Benchmarks (Core Operations)

Run micro-benchmarks directly:

```bash
cargo bench
```

### Results (Apple Silicon M4, 16GB RAM)

<!-- BENCH_TABLE_START -->
| Operation              | Throughput           | Latency    | Description                            |
|------------------------|----------------------|------------|----------------------------------------|
| AAAK Compression       | ~49,000 ops/sec      | ~20 µs     | Compress 1KB meeting notes             |
| Entity Detection       | ~196,000 ops/sec     | ~5 µs      | Heuristic NER (People/Projects/Terms)  |
| Token Counting         | ~5,400,000 ops/sec   | ~186 ns    | Word-based token estimation            |
| Compression Stats      | ~1,130,000 ops/sec   | ~886 ns    | Calculate honest compression ratios    |
<!-- BENCH_TABLE_END -->

**Binary Size**: 7.9 MB (release build)  
**Cold Start**: ~300 ms  
**Memory Usage**: ~50 MB baseline

## Standard Benchmarks

### LongMemEval (500 questions)

Tests retrieval across ~53 conversation sessions per question. The standard benchmark for AI memory.

```bash
# Download data
mkdir -p /tmp/longmemeval-data
curl -fsSL -o /tmp/longmemeval-data/longmemeval_s_cleaned.json \
  https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned/resolve/main/longmemeval_s_cleaned.json

# Run (raw mode — expected 96.6% result)
./target/release/mempalace-rs benchmark longmemeval /tmp/longmemeval-data/longmemeval_s_cleaned.json

# Run with AAAK compression (expected 84.2%)
./target/release/mempalace-rs benchmark longmemeval /tmp/longmemeval-data/longmemeval_s_cleaned.json --mode aaak

# Run with room-based boosting (expected 89.4%)
./target/release/mempalace-rs benchmark longmemeval /tmp/longmemeval-data/longmemeval_s_cleaned.json --mode rooms
```

**Expected output (raw mode, full 500):**

```text
Recall@5:  0.966
Recall@10: 0.982
NDCG@10:   0.889
Time:      ~30 seconds (Rust) vs ~5 minutes (Python)
```

### LoCoMo (1,986 QA pairs)

Tests multi-hop reasoning across 10 long conversations (19-32 sessions each, 400-600 dialog turns).

```bash
./target/release/mempalace-rs benchmark locomo <dataset_path>
```

## Hybrid Mode Benchmarking

Run benchmarks with different compression modes:

| Mode      | Description                                 | Expected Recall@5 |
|-----------|---------------------------------------------|-------------------|
| `raw`     | Full verbatim retrieval                     | ~96.6%            |
| `aaak`    | AAAK compressed retrieval (~30x smaller)    | ~84.2%            |
| `rooms`   | Room-based metadata boosting                | ~89.4%            |
| `hybrid`  | Combined AAAK + room boosting               | ~91.0%            |

## Performance Notes

- Rust port is **~10x faster** than Python equivalent
- Vector search is fully local (no network I/O overhead)
- SQLite operations are negligible (<1ms)
- AAAK compression adds minimal overhead (~20µs per 1KB)

## Comparison with Python

| Benchmark                  | Python      | Rust        | Speedup |
|----------------------------|-------------|-------------|---------|
| LongMemEval (500)          | ~5 min      | ~30 sec     | **10x** |
| LoCoMo (1,986)             | ~20 min     | ~2 min      | **10x** |
| File Mining (100MB)        | ~2 min      | ~15 sec     | **8x**  |
| AAAK Compression (1KB)     | ~200 µs     | ~20 µs      | **10x** |
| Entity Detection           | ~50 µs      | ~5 µs       | **10x** |

Benchmarked on Apple Silicon M4, 16GB RAM

## Running All Tests

```bash
# Run test suite
cargo test

# Run with timing
time cargo test

# Expected output:
# running 197 tests
# test result: ok. 197 passed; 0 failed; 0 ignored
```

## Contributing Benchmarks

Add new benchmarks to `benches/` directory:

```rust
use std::time::Instant;

fn bench_my_operation() {
    let iterations = 1000;
    let start = Instant::now();
    
    for _ in 0..iterations {
        // Your operation here
    }
    
    let duration = start.elapsed();
    println!("Avg time: {:?}", duration / iterations);
}
```

## Resources

- LongMemEval paper: <https://arxiv.org/abs/2407.01437>
- LoCoMo paper: <https://arxiv.org/abs/2402.09171>
