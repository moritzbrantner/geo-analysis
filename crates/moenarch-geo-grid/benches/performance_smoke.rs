use geo_core::Geometry;
use geo_grid::{
    geometry_to_h3_cells, geometry_to_square_cells, H3Containment, H3CoverageOptions,
    SquareCoverageOptions,
};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use std::hint::black_box;

fn sample_polygon() -> Geometry {
    Geometry::Polygon {
        coordinates: vec![vec![
            [8.55, 48.72],
            [8.95, 48.72],
            [8.95, 49.05],
            [8.55, 49.05],
            [8.55, 48.72],
        ]],
    }
}

#[library_benchmark]
#[bench::h3_resolution_9(sample_polygon())]
fn bench_h3_polygon_coverage(geometry: Geometry) -> usize {
    let coverage = geometry_to_h3_cells(
        black_box(&geometry),
        black_box(H3CoverageOptions {
            resolution: 9,
            containment: H3Containment::Covers,
            max_cells: 250_000,
        }),
    )
    .expect("benchmark polygon is valid");
    black_box(coverage.cells.len())
}

#[library_benchmark]
#[bench::square_zoom_14(sample_polygon())]
fn bench_square_polygon_coverage(geometry: Geometry) -> usize {
    let coverage = geometry_to_square_cells(
        black_box(&geometry),
        black_box(SquareCoverageOptions {
            zoom: 14,
            max_cells: 250_000,
        }),
    )
    .expect("benchmark polygon is valid");
    black_box(coverage.cells.len())
}

library_benchmark_group!(
    name = geo_grid_smoke;
    benchmarks = bench_h3_polygon_coverage, bench_square_polygon_coverage
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
    library_benchmark_groups = geo_grid_smoke
);
