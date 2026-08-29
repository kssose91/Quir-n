//! Benchmarks para KeywordSearcher
//!
//! NOTA: Setup dentro del closure para evitar problemas de lifetime con TempDir

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use quiron_brain::keyword_search::KeywordSearcher;
use quiron_brain::ledger::{LedgerReader, LedgerWriter};
use quiron_brain::storage::Storage;
use quiron_brain::types::{Event, EventKind};
use tempfile::TempDir;

fn setup_with_data(num_events: usize) -> (TempDir, LedgerReader) {
    let dir = TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let reader = LedgerReader::new(storage.clone());
    let writer = LedgerWriter::new(storage.clone());

    let descriptions = [
        "Fixed critical authentication bypass vulnerability",
        "Refactored the user service to improve performance",
        "Added new REST API endpoints for analytics",
        "Updated Rust dependencies to latest versions",
        "Implemented Redis caching for session management",
        "Resolved memory leak in background worker",
        "Added unit tests for payment processing module",
        "Migrated database schema to PostgreSQL 15",
    ];

    for i in 0..num_events {
        let desc = descriptions[i % descriptions.len()];
        let mut event = Event::new(EventKind::FileRead, &format!("{} (iteration {})", desc, i));
        event.tags = vec![
            "code".to_string(),
            format!("module-{}", i % 5),
            if i % 3 == 0 {
                "important".to_string()
            } else {
                "routine".to_string()
            },
        ];
        writer.append(event).unwrap();
    }

    (dir, reader)
}

fn bench_keyword_search_simple(c: &mut Criterion) {
    c.bench_function("keyword_search_simple", |b| {
        let (_dir, reader) = setup_with_data(1000);
        let searcher = KeywordSearcher::new(&reader);

        b.iter(|| black_box(searcher.search(black_box("authentication")).unwrap()))
    });
}

fn bench_keyword_search_multi_word(c: &mut Criterion) {
    c.bench_function("keyword_search_multi_word", |b| {
        let (_dir, reader) = setup_with_data(1000);
        let searcher = KeywordSearcher::new(&reader);

        b.iter(|| {
            black_box(
                searcher
                    .search(black_box("user service performance"))
                    .unwrap(),
            )
        })
    });
}

fn bench_keyword_search_mixed(c: &mut Criterion) {
    c.bench_function("keyword_search_mixed", |b| {
        let (_dir, reader) = setup_with_data(1000);
        let searcher = KeywordSearcher::new(&reader);
        let queries = [
            "authentication",
            "database",
            "redis",
            "schema",
            "worker",
            "api",
            "test",
        ];
        let mut i = 0usize;

        b.iter(|| {
            let q = queries[i % queries.len()];
            i += 1;
            black_box(searcher.search(black_box(q)).unwrap())
        })
    });
}

fn bench_keyword_search_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("keyword_search_scaling");

    for size in [100, 500, 1000, 2000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let (_dir, reader) = setup_with_data(size);
            let searcher = KeywordSearcher::new(&reader);

            b.iter(|| black_box(searcher.search(black_box("database")).unwrap()))
        });
    }

    group.finish();
}

fn bench_keyword_search_no_results(c: &mut Criterion) {
    c.bench_function("keyword_search_no_results", |b| {
        let (_dir, reader) = setup_with_data(500);
        let searcher = KeywordSearcher::new(&reader);

        b.iter(|| black_box(searcher.search(black_box("nonexistent_xyz_term")).unwrap()))
    });
}

criterion_group!(
    benches,
    bench_keyword_search_simple,
    bench_keyword_search_multi_word,
    bench_keyword_search_mixed,
    bench_keyword_search_scaling,
    bench_keyword_search_no_results,
);
criterion_main!(benches);
