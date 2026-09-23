use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use maps_kernels_core::{
    densify_line_flat, path_summary_flat, resample_line_flat, simplify_line_flat, surface,
};
use runtime_core::{OperationId, SurfaceRequest};
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

// These new identities seed corrected behavior. The old equal-count shortcut
// returned incorrect output, so its cheaper copy is not a valid speed baseline.
#[library_benchmark]
#[bench::points_256_to_256(generated_polyline(256))]
fn bench_resample_equal_count(coordinates: Vec<f64>) -> usize {
    let resampled = resample_line_flat(black_box(&coordinates), black_box(256))
        .expect("deterministic benchmark input is valid");
    black_box(resampled.len())
}

#[library_benchmark]
#[bench::points_256(generated_polyline(256))]
fn bench_densify_line(coordinates: Vec<f64>) -> usize {
    let densified = densify_line_flat(black_box(&coordinates), black_box(0.1))
        .expect("deterministic benchmark input is valid");
    black_box(densified.len())
}

#[library_benchmark]
#[bench::points_4096(generated_polyline(4096))]
fn bench_path_summary(coordinates: Vec<f64>) -> f64 {
    let summary = path_summary_flat(black_box(&coordinates), black_box(false))
        .expect("deterministic benchmark input is valid");
    black_box(summary.length)
}

fn excessive_densification_request() -> SurfaceRequest {
    SurfaceRequest {
        operation: OperationId::new("maps.densifyLine"),
        input: serde_json::json!({
            "coordinates": [0.0, 0.0, 1.0, 0.0],
            "maxSegmentLength": 1e-12
        }),
    }
}

#[library_benchmark]
#[bench::trillion_requested_points(excessive_densification_request())]
fn bench_reject_excessive_densification(request: SurfaceRequest) -> bool {
    let rejected = surface::run_surface_operation(black_box(request)).is_err();
    assert!(rejected, "output budget must be enforced before generation");
    black_box(rejected)
}

library_benchmark_group!(
    name = map_kernels_smoke;
    benchmarks = bench_simplify_line, bench_resample_line,
        bench_resample_equal_count, bench_densify_line, bench_path_summary,
        bench_reject_excessive_densification
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
