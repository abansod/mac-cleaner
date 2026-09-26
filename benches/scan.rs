use std::fs;
use std::hint::black_box;
use std::path::PathBuf;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

use mac_cleaner::models::{Category, FileGroup, FileItem, ScanResult};
use mac_cleaner::safety::safe_size;
use mac_cleaner::scanners::{duplicate_scanner_in, Scanner};

/// A generated folder under the system temp dir, removed on drop.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mac-cleaner-bench-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fixture root");
        Self { root }
    }

    fn write(&self, rel: &str, contents: &[u8]) -> PathBuf {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture dir");
        }
        fs::write(&path, contents).expect("write fixture file");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// `len` bytes that start with `seed`, so files of the same size hash differently.
fn contents(seed: u64, len: usize) -> Vec<u8> {
    let mut buf = vec![(seed % 251) as u8; len];
    let tag = seed.to_le_bytes();
    let n = tag.len().min(len);
    buf[..n].copy_from_slice(&tag[..n]);
    buf
}

fn bench_safe_size(c: &mut Criterion) {
    const DIRS: usize = 10;
    const SUBDIRS: usize = 10;
    const FILES: usize = 50;
    const FILE_LEN: usize = 512;

    let fixture = Fixture::new("safe-size");
    let data = vec![7u8; FILE_LEN];
    for d in 0..DIRS {
        for s in 0..SUBDIRS {
            for f in 0..FILES {
                fixture.write(&format!("d{d}/s{s}/f{f}.bin"), &data);
            }
        }
    }
    let file_count = DIRS * SUBDIRS * FILES;
    assert_eq!(safe_size(&fixture.root), (file_count * FILE_LEN) as u64);

    let mut group = c.benchmark_group("safe_size");
    group.throughput(Throughput::Elements(file_count as u64));
    group.bench_function("5000_small_files", |b| {
        b.iter(|| safe_size(black_box(&fixture.root)))
    });
    group.finish();
}

fn bench_duplicate_scan(c: &mut Criterion) {
    const SMALL_PAIRS: u64 = 100;
    const SMALL_LEN: usize = 200 * 1024;
    const LARGE_PAIRS: u64 = 3;
    const LARGE_LEN: usize = 5 * 1024 * 1024;
    const SINGLES: u64 = 100;

    let fixture = Fixture::new("duplicates");
    for i in 0..SMALL_PAIRS {
        let data = contents(i, SMALL_LEN);
        fixture.write(&format!("a/small{i}.bin"), &data);
        fixture.write(&format!("b/small{i}.bin"), &data);
    }
    // Over 2 MB, so quick_hash reads only the first and last chunk.
    for i in 0..LARGE_PAIRS {
        let data = contents(1_000 + i, LARGE_LEN);
        fixture.write(&format!("a/large{i}.bin"), &data);
        fixture.write(&format!("b/large{i}.bin"), &data);
    }
    // Unique sizes, so these are dropped before hashing.
    for i in 0..SINGLES {
        let len = SMALL_LEN + 1 + i as usize;
        fixture.write(&format!("c/single{i}.bin"), &contents(2_000 + i, len));
    }

    let scanner = duplicate_scanner_in(vec![fixture.root.clone()]);
    let found = scanner.scan(&mut |_| {});
    assert_eq!(found.len(), (SMALL_PAIRS + LARGE_PAIRS) as usize);

    let mut group = c.benchmark_group("duplicate_scan");
    group.sample_size(20);
    group.throughput(Throughput::Elements(
        2 * (SMALL_PAIRS + LARGE_PAIRS) + SINGLES,
    ));
    group.bench_function("203_pairs_and_singles", |b| {
        b.iter(|| scanner.scan(&mut |_| {}))
    });
    group.finish();
}

fn scan_result(fixture: &Fixture) -> ScanResult {
    const CATEGORIES: [Category; 5] = [
        Category::UserCache,
        Category::Logs,
        Category::Temp,
        Category::Xcode,
        Category::Duplicates,
    ];
    const GROUPS_PER_CATEGORY: usize = 40;
    const ITEMS_PER_GROUP: usize = 50;

    let mut groups = Vec::new();
    for (c, category) in CATEGORIES.into_iter().enumerate() {
        for g in 0..GROUPS_PER_CATEGORY {
            let key = format!("c{c}g{g}");
            let items = (0..ITEMS_PER_GROUP)
                .map(|i| {
                    let path = fixture.write(&format!("{key}/{i}"), &[]);
                    let size = ((c + 1) * 1_000 + g * 10 + i) as u64;
                    FileItem::file(path, size, category, "bench", key.clone())
                })
                .collect();
            groups.push(FileGroup {
                key: key.clone(),
                category,
                title: key,
                description: String::new(),
                items,
            });
        }
    }
    ScanResult {
        groups,
        ..ScanResult::default()
    }
}

fn bench_scan_result_queries(c: &mut Criterion) {
    let fixture = Fixture::new("result");
    let result = scan_result(&fixture);
    assert_eq!(result.total_files(), 10_000);

    // Every call checks each item on disk; the TUI makes these calls on each redraw.
    let mut group = c.benchmark_group("scan_result");
    group.sample_size(50);
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(10_000));
    group.bench_function("total_size", |b| b.iter(|| black_box(&result).total_size()));
    group.bench_function("categories_sorted", |b| {
        b.iter(|| black_box(&result).categories_sorted())
    });
    group.bench_function("groups_in", |b| {
        b.iter(|| {
            black_box(&result)
                .groups_in(black_box(Category::UserCache))
                .len()
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_safe_size,
    bench_duplicate_scan,
    bench_scan_result_queries
);
criterion_main!(benches);
