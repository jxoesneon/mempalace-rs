use criterion::{criterion_group, criterion_main, Criterion};
use mempalace_rs::dialect::{AAAKContext, Dialect};
use mempalace_rs::entity_detector::extract_entities;

fn aaak_compression_benchmark(c: &mut Criterion) {
    let test_text = "Alice and Bob decided to use Rust for the new backend service. \
        They preferred it over Go because of memory safety guarantees. \
        The team was excited about the performance improvements.";

    c.bench_function("aaak_compression", |b| {
        b.iter(|| AAAKContext::compress(test_text))
    });
}

fn entity_detection_benchmark(c: &mut Criterion) {
    let test_text = "Alice and Bob worked with Dr. Chen on the Phoenix project. \
        They discussed React vs Svelte with the Engineering team.";

    c.bench_function("entity_detection", |b| {
        b.iter(|| extract_entities(test_text))
    });
}

fn token_counting_benchmark(c: &mut Criterion) {
    let test_text = "This is a sample text with approximately twenty words for testing token estimation accuracy.";

    c.bench_function("token_counting", |b| {
        b.iter(|| Dialect::count_tokens(test_text))
    });
}

fn compression_stats_benchmark(c: &mut Criterion) {
    let original = "Alice and Bob decided to use Rust for the new backend service.";
    let compressed = "ALC|BOB|DECIDE:Rust>Go[BACKEND]|PREF:safety|EXCITE:performance";
    let dialect = Dialect::default();

    c.bench_function("compression_stats", |b| {
        b.iter(|| dialect.compression_stats(original, compressed))
    });
}

criterion_group!(
    benches,
    aaak_compression_benchmark,
    entity_detection_benchmark,
    token_counting_benchmark,
    compression_stats_benchmark
);
criterion_main!(benches);
