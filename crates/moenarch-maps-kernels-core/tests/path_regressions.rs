//! Regression cases found by the September 2026 kernel audit.
//! Assertions are deterministic; no elapsed-time thresholds are used.

use maps_kernels_core::{
    densify_line_flat, path_summary_flat, resample_line_flat, resample_ring_flat, surface,
};
use runtime_core::{OperationId, SurfaceRequest};
use serde_json::{json, Value};

fn run(operation: &str, input: Value) -> Result<Value, String> {
    surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new(operation),
        input,
    })
    .map(|response| response.value)
}

#[test]
fn same_count_resampling_is_not_an_identity_operation() {
    let input = [0.0, 0.0, 1.0, 0.0, 10.0, 0.0];
    let expected = vec![0.0, 0.0, 5.0, 0.0, 10.0, 0.0];
    assert_eq!(resample_line_flat(&input, 3).unwrap(), expected);
    let response = run(
        "maps.applyKernel",
        json!({"coordinates": input, "coordinateCount": 3}),
    )
    .unwrap();
    assert_eq!(response["coordinates"], json!(expected));
}

#[test]
fn duplicate_vertices_do_not_consume_sampling_intervals() {
    let input = [0.0, 0.0, 0.0, 0.0, 9.0, 0.0, 9.0, 0.0];
    assert_eq!(
        resample_line_flat(&input, 4).unwrap(),
        vec![0.0, 0.0, 3.0, 0.0, 6.0, 0.0, 9.0, 0.0]
    );
}

#[test]
fn monotone_lines_match_an_independent_arc_length_oracle() {
    for source_count in 3..=32 {
        let mut input = Vec::new();
        for index in 0..source_count {
            // Uneven spacing and repeated positions, without a random seed.
            let x = (index / 2) as f64;
            input.extend_from_slice(&[x * x, 7.0]);
        }
        let end = input[input.len() - 2];
        for output_count in [2, 3, source_count, source_count + 5] {
            let output = resample_line_flat(&input, output_count).unwrap();
            assert_eq!(output.len(), output_count * 2);
            for (index, point) in output.chunks_exact(2).enumerate() {
                let expected = end * (index as f64 / (output_count - 1) as f64);
                assert!((point[0] - expected).abs() <= 1e-10);
                assert_eq!(point[1], 7.0);
            }
        }
    }
}

#[test]
fn large_finite_line_does_not_overflow_the_sample_index_product() {
    let output = resample_line_flat(&[0.0, 0.0, 1e308, 0.0], 5).unwrap();
    for (index, point) in output.chunks_exact(2).enumerate() {
        assert!(point[0].is_finite());
        assert!((point[0] / 1e308 - index as f64 / 4.0).abs() < 1e-14);
        assert_eq!(point[1], 0.0);
    }
}

#[test]
fn large_finite_ring_does_not_overflow_the_sample_index_product() {
    let output = resample_ring_flat(&[0.0, 0.0, 4e307, 0.0, 0.0, 0.0], 8).unwrap();
    let expected = [0.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0];
    for (point, expected) in output.chunks_exact(2).zip(expected) {
        assert!(point[0].is_finite());
        assert!((point[0] / 1e307 - expected).abs() < 1e-14);
        assert_eq!(point[1], 0.0);
    }
}

#[test]
fn unrepresentable_lengths_are_errors_not_infinity_or_json_null() {
    let segment_overflow = [1e308, 0.0, -1e308, 0.0];
    let sum_overflow = [0.0, 0.0, 1e308, 0.0, 0.0, 0.0];
    assert!(path_summary_flat(&segment_overflow, false).is_err());
    assert!(path_summary_flat(&sum_overflow, false).is_err());
    assert!(path_summary_flat(&sum_overflow, true).is_err());
    assert!(resample_line_flat(&segment_overflow, 2).is_err());
    assert!(resample_ring_flat(&sum_overflow, 4).is_err());
    for operation in ["maps.kernelSummary", "maps.pathSummary"] {
        assert!(run(operation, json!({"coordinates": segment_overflow})).is_err());
        assert!(run(operation, json!({"coordinates": sum_overflow})).is_err());
    }
}

#[test]
fn native_output_counts_cannot_overflow_flat_capacity() {
    let byte_capacity_overflow = isize::MAX as usize / (2 * std::mem::size_of::<f64>()) + 1;
    for count in [usize::MAX, byte_capacity_overflow] {
        assert!(resample_line_flat(&[0.0, 0.0, 1.0, 0.0], count).is_err());
        assert!(resample_line_flat(&[0.0, 0.0, 0.0, 0.0], count).is_err());
        assert!(resample_ring_flat(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0], count).is_err());
        assert!(resample_ring_flat(&[0.0; 6], count).is_err());
    }
}

#[test]
fn densification_preserves_original_vertices_despite_cancellation() {
    let input = [1e16, 0.0, 1.0, 0.0, -1e16, 0.0, -1.0, 0.0];
    assert_eq!(densify_line_flat(&input, 2e16).unwrap(), input);
}

#[test]
fn densification_rejects_unrepresentable_subdivision_counts() {
    // The previous float-to-usize cast saturated and entered an enormous loop.
    assert!(densify_line_flat(&[0.0, 0.0, 1.0, 0.0], f64::from_bits(1)).is_err());
    assert!(densify_line_flat(&[1e308, 0.0, -1e308, 0.0], 1.0).is_err());
    assert!(densify_line_flat(&[0.0, 0.0, 1e20, 0.0], 1.0).is_err());
}

#[test]
fn densification_keeps_zero_length_segments_and_spacing() {
    let input = [0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 3.0, 4.0];
    let output = densify_line_flat(&input, 1.0).unwrap();
    assert_eq!(&output[..4], &[0.0, 0.0, 0.0, 0.0]);
    assert_eq!(&output[output.len() - 2..], &[3.0, 4.0]);
    assert_eq!(output.len() / 2, 9);
    for segment in output.windows(4).step_by(2) {
        assert!((segment[2] - segment[0]).hypot(segment[3] - segment[1]) <= 1.0);
    }
}

#[test]
fn surface_resampling_enforces_the_existing_point_budget() {
    for closed in [false, true] {
        for count in [100_001_usize, usize::MAX] {
            assert!(run(
                "maps.applyKernel",
                json!({"coordinates": [0.0; 6], "coordinateCount": count, "closed": closed}),
            )
            .is_err());
        }
    }
    let response = run(
        "maps.applyKernel",
        json!({"coordinates": [0.0, 0.0, 1.0, 0.0], "coordinateCount": 100_000}),
    )
    .unwrap();
    assert_eq!(response["coordinateCount"], 100_000);
    assert_eq!(response["coordinates"].as_array().unwrap().len(), 200_000);
}

#[test]
fn surface_densification_preflights_the_total_not_just_each_segment() {
    for coordinates in [
        vec![0.0, 0.0, 100_000.0, 0.0],
        vec![0.0, 0.0, 60_000.0, 0.0, 120_000.0, 0.0],
    ] {
        assert!(run(
            "maps.densifyLine",
            json!({"coordinates": coordinates, "maxSegmentLength": 1.0}),
        )
        .is_err());
    }
    assert!(run(
        "maps.densifyLine",
        json!({"coordinates": [0.0, 0.0, 1.0, 0.0], "maxSegmentLength": 1e-12}),
    )
    .is_err());
    let response = run(
        "maps.densifyLine",
        json!({"coordinates": [0.0, 0.0, 99_999.0, 0.0], "maxSegmentLength": 1.0}),
    )
    .unwrap();
    assert_eq!(response["outputPointCount"], 100_000);
}

#[test]
fn summary_aliases_share_one_numeric_authority() {
    for closed in [false, true] {
        let input = json!({"coordinates": [0.0, 0.0, 3.0, 0.0, 3.0, 4.0], "closed": closed});
        let legacy = run("maps.kernelSummary", input.clone()).unwrap();
        let summary = run("maps.pathSummary", input).unwrap();
        assert_eq!(legacy["coordinateCount"], summary["pointCount"]);
        assert_eq!(legacy["segmentCount"], summary["segmentCount"]);
        assert_eq!(legacy["totalLength"], summary["length"]);
        assert_eq!(legacy["bbox"], summary["bounds"]);
        assert_eq!(legacy["totalLength"], if closed { 12.0 } else { 7.0 });
    }
}
