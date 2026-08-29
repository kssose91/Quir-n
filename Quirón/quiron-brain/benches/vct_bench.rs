//! Benchmarks para Virtual Context Tools (recall, timeline)
//!
//! NOTA: Setup dentro del closure para evitar problemas de lifetime con TempDir

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use quiron_brain::ledger::{LedgerReader, LedgerWriter};
use quiron_brain::storage::Storage;
use quiron_brain::types::{Event, EventKind};
use quiron_brain::vct::{RecallScope, VirtualContextTools};
use tempfile::TempDir;

fn setup_with_data(num_events: usize) -> (TempDir, Storage, LedgerReader) {
    let dir = TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let reader = LedgerReader::new(storage.clone());
    let writer = LedgerWriter::new(storage.clone());

    let descriptions = [
        "Fixed authentication bug in login module",
        "Refactored database connection pooling",
        "Added new API endpoint for users",
        "Updated dependencies in Cargo.toml",
        "Implemented caching layer for queries",
    ];

    for i in 0..num_events {
        let desc = descriptions[i % descriptions.len()];
        let mut event = Event::new(EventKind::FileRead, &format!("{} #{}", desc, i));
        event.tags = vec!["benchmark".to_string(), format!("batch-{}", i / 100)];
        event.inputs = vec![format!("/path/to/file_{}.rs", i % 20)];
        writer.append(event).unwrap();
    }

    (dir, storage, reader)
}

fn bench_recall_recent(c: &mut Criterion) {
    c.bench_function("recall_recent_scope", |b| {
        let (_dir, storage, reader) = setup_with_data(500);
        let vct = VirtualContextTools::new(&storage, &reader);

        b.iter(|| {
            black_box(
                vct.recall(black_box("authentication"), RecallScope::Recent, 10)
                    .unwrap(),
            )
        })
    });
}

fn bench_recall_all(c: &mut Criterion) {
    c.bench_function("recall_all_scope", |b| {
        let (_dir, storage, reader) = setup_with_data(500);
        let vct = VirtualContextTools::new(&storage, &reader);

        b.iter(|| {
            black_box(
                vct.recall(black_box("database"), RecallScope::All, 10)
                    .unwrap(),
            )
        })
    });
}

fn bench_recall_mixed(c: &mut Criterion) {
    // Alternar queries para evitar cache bias
    c.bench_function("recall_mixed_queries", |b| {
        let (_dir, storage, reader) = setup_with_data(500);
        let vct = VirtualContextTools::new(&storage, &reader);
        let queries = ["bug", "api", "database", "cache", "auth"];
        let mut i = 0usize;

        b.iter(|| {
            let q = queries[i % queries.len()];
            i += 1;
            black_box(vct.recall(black_box(q), RecallScope::All, 10).unwrap())
        })
    });
}

fn bench_recall_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("recall_scaling");

    for size in [100, 500, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let (_dir, storage, reader) = setup_with_data(size);
            let vct = VirtualContextTools::new(&storage, &reader);

            b.iter(|| black_box(vct.recall(black_box("bug"), RecallScope::All, 20).unwrap()))
        });
    }

    group.finish();
}

fn bench_timeline(c: &mut Criterion) {
    c.bench_function("timeline_file", |b| {
        let (_dir, storage, reader) = setup_with_data(500);
        let vct = VirtualContextTools::new(&storage, &reader);

        b.iter(|| black_box(vct.timeline(black_box("/path/to/file_5.rs"), 20).unwrap()))
    });
}

criterion_group!(
    benches,
    bench_recall_recent,
    bench_recall_all,
    bench_recall_mixed,
    bench_recall_scaling,
    bench_timeline,
);
criterion_main!(benches);
