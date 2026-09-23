use geo_clustering::{ClusterIndex, ClusterOptions, ClusterPoint};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use std::hint::black_box;

fn generated_index(side: usize) -> ClusterIndex<()> {
    let denominator = (side.saturating_sub(1)).max(1) as f64;
    let points = (0..side).flat_map(|y| {
        (0..side).map(move |x| ClusterPoint {
            id: format!("p-{x}-{y}"),
            longitude: -180.0 + (x as f64 / denominator) * 360.0,
            latitude: -90.0 + (y as f64 / denominator) * 180.0,
            properties: (),
        })
    });

    ClusterIndex::new(points, ClusterOptions::default())
        .expect("deterministic benchmark points are valid")
}

#[library_benchmark]
#[bench::viewport_65k(generated_index(256))]
fn bench_viewport_query(index: ClusterIndex<()>) -> usize {
    let items = index
        .get_clusters(black_box([7.0, 48.0, 9.0, 50.0]), black_box(8))
        .expect("deterministic viewport query is valid");
    black_box(items.len())
}

#[library_benchmark]
#[bench::antimeridian_65k(generated_index(256))]
fn bench_antimeridian_query(index: ClusterIndex<()>) -> usize {
    let items = index
        .get_clusters(black_box([175.0, -5.0, -175.0, 5.0]), black_box(8))
        .expect("deterministic antimeridian query is valid");
    black_box(items.len())
}

library_benchmark_group!(
    name = clustering_smoke;
    benchmarks = bench_viewport_query, bench_antimeridian_query
);

fn benchmark_config() -> LibraryBenchmarkConfig {
    let mut callgrind = Callgrind::default();
    callgrind
        .soft_limits([(EventKind::Ir, 5.0)])
        .fail_fast(true);
    let mut config = LibraryBenchmarkConfig::default();
    config.tool(callgrind);
    config
}

main!(
    config = benchmark_config();
    library_benchmark_groups = clustering_smoke
);
