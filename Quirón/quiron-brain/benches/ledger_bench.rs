//! Benchmarks para el ledger (lectura/escritura de eventos)
//!
//! NOTA: Setup dentro del closure para evitar problemas de lifetime con TempDir

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use quiron_brain::ledger::{LedgerReader, LedgerWriter};
use quiron_brain::storage::Storage;
use quiron_brain::types::{Event, EventKind};
use tempfile::TempDir;

fn setup() -> (TempDir, Storage, LedgerReader, LedgerWriter) {
    let dir = TempDir::new().unwrap();
    let storage = Storage::open(dir.path()).unwrap();
    let reader = LedgerReader::new(storage.clone());
    let writer = LedgerWriter::new(storage.clone());
    (dir, storage, reader, writer)
}

fn bench_event_write(c: &mut Criterion) {
    c.bench_function("event_write", |b| {
        let (_dir, _storage, _reader, writer) = setup();

        b.iter(|| {
            let event = Event::new(EventKind::FileRead, "benchmark test event");
            black_box(writer.append(black_box(event)).unwrap())
        })
    });
}

fn bench_event_read_recent(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_read_recent");

    for limit in [10, 50, 100, 500].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(limit), limit, |b, &limit| {
            // Setup dentro del closure
            let (_dir, _storage, reader, writer) = setup();

            // Preparar datos: 1000 eventos
            for i in 0..1000 {
                let event = Event::new(EventKind::FileRead, &format!("event {}", i));
                writer.append(event).unwrap();
            }

            b.iter(|| black_box(reader.recent(black_box(limit)).unwrap()))
        });
    }

    group.finish();
}

fn bench_event_count(c: &mut Criterion) {
    c.bench_function("event_count", |b| {
        let (_dir, _storage, reader, writer) = setup();

        // Preparar datos
        for i in 0..500 {
            let event = Event::new(EventKind::FileRead, &format!("event {}", i));
            writer.append(event).unwrap();
        }

        b.iter(|| black_box(reader.count().unwrap()))
    });
}

fn bench_event_by_project(c: &mut Criterion) {
    c.bench_function("event_by_project", |b| {
        let (_dir, _storage, reader, writer) = setup();

        // Preparar datos: eventos en diferentes proyectos
        for i in 0..300 {
            let mut event = Event::new(EventKind::FileRead, &format!("event {}", i));
            event.project_id = Some(format!("project-{}", i % 10));
            writer.append(event).unwrap();
        }

        // Alternar proyectos para evitar cache bias
        let projects = ["project-0", "project-3", "project-5", "project-7"];
        let mut i = 0usize;

        b.iter(|| {
            let proj = projects[i % projects.len()];
            i += 1;
            black_box(reader.by_project(black_box(proj), 50).unwrap())
        })
    });
}

criterion_group!(
    benches,
    bench_event_write,
    bench_event_read_recent,
    bench_event_count,
    bench_event_by_project,
);
criterion_main!(benches);
