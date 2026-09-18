use geo_viz::{GeoVizPoint, GeoVizScalarFieldIndex, GeoVizScalarFieldOptions};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use std::hint::black_box;

fn generated_index() -> GeoVizScalarFieldIndex {
    let side = 64usize;
    let denominator = (side - 1) as f64;
    let points = (0..side).flat_map(|row| {
        (0..side).map(move |column| {
            let longitude = 5.0 + (column as f64 / denominator) * 10.0;
            let latitude = 47.0 + (row as f64 / denominator) * 8.0;
            let value = (row * side + column) as f64;
            GeoVizPoint {
                id: Some(format!("p-{column}-{row}")),
                label: None,
                longitude,
                latitude,
                metrics: [("value".to_string(), value)].into_iter().collect(),
                properties: serde_json::Value::Null,
            }
        })
    });

    GeoVizScalarFieldIndex::new(
        points,
        GeoVizScalarFieldOptions {
            domain_bounds: Some([5.0, 47.0, 15.0, 55.0]),
            interpolation_k: Some(12),
            ..Default::default()
        },
    )
    .expect("deterministic scalar-field benchmark points are valid")
}

#[library_benchmark]
#[bench::sample_4k_points(generated_index())]
fn bench_repeated_samples(index: GeoVizScalarFieldIndex) -> f64 {
    let mut total = 0.0;
    for sample in 0..256usize {
        let longitude = 5.0 + (sample % 32) as f64 / 31.0 * 10.0;
        let latitude = 47.0 + (sample / 32) as f64 / 7.0 * 8.0;
        total += index
            .get_value_at_coordinate(black_box([longitude, latitude]))
            .expect("deterministic sample is valid")
            .unwrap_or_default();
    }
    black_box(total)
}

library_benchmark_group!(
    name = scalar_field_smoke;
    benchmarks = bench_repeated_samples
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
    library_benchmark_groups = scalar_field_smoke
);
