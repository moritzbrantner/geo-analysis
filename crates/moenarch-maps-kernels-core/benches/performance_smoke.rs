use iai_callgrind::{
    Callgrind, EventKind, LibraryBenchmarkConfig, library_benchmark, library_benchmark_group, main,
};
use maps_kernels_core::{resample_line_flat, simplify_line_flat};
use std::hint::black_box;

fn generated_polyline(point_count: usize) -> Vec<f64> {
    let mut coordinates = Vec::with_capacity(point_count * 2);
    for index in 0..point_count {
        let x = index as f64 * 0.25;
        let y = ((index % 17) as f64 - 8.0) * 0.2 + ((index * 7 % 11) as f64) * 0.03;
        coordinates.extend_from_slice(&[x, y]);
    }
    coordinates
}

#[library_benchmark]
#[bench::points_256(generated_polyline(256))]
#[bench::points_512(generated_polyline(512))]
fn bench_simplify_line(coordinates: Vec<f64>) -> usize {
    let simplified = simplify_line_flat(black_box(&coordinates), black_box(0.08))
        .expect("deterministic benchmark input is valid");
    black_box(simplified.len())
}

#[library_benchmark]
#[bench::points_256_to_512(generated_polyline(256))]
fn bench_resample_line(coordinates: Vec<f64>) -> usize {
    let resampled = resample_line_flat(black_box(&coordinates), black_box(512))
        .expect("deterministic benchmark input is valid");
    black_box(resampled.len())
}

library_benchmark_group!(
    name = map_kernels_smoke;
    benchmarks = bench_simplify_line, bench_resample_line
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
    library_benchmark_groups = map_kernels_smoke
);
