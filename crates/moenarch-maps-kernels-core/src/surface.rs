//! Library-owned runtime surface for `maps-kernels-core`.

use runtime_core::{
    describe_surface_response, parse_surface_input, structured_operation_response,
    surface_operation, PackageSurface, RuntimeCapabilities, SurfaceError, SurfaceRequest,
    SurfaceResponse,
};
use serde::Deserialize;

use crate::{
    densify_line_flat, path_summary_flat, resample_line_flat, resample_ring_flat,
    simplify_line_flat,
};

const MAX_VALUES: usize = 100_000;

/// Returns the package surface exposed by every transport wrapper.
pub fn package_surface() -> PackageSurface {
    PackageSurface {
        library: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: RuntimeCapabilities::pure_rust(),
        operations: vec![
            surface_operation(
                "describe",
                "Describe package",
                "Numeric kernels for map and temporal GeoJSON processing.",
                serde_json::json!({"includeOperations": true}),
            ),
            surface_operation(
                "maps.kernelSummary",
                "Map kernel summary",
                "Summarizes flat 2D coordinates as an open line or closed ring.",
                serde_json::json!({"coordinates": [0.0, 0.0, 1.0, 0.0], "closed": false}),
            ),
            surface_operation(
                "maps.applyKernel",
                "Apply map kernel",
                "Resamples flat 2D coordinates as an open line or closed ring.",
                serde_json::json!({"coordinates": [0.0, 0.0, 10.0, 0.0], "coordinateCount": 3, "closed": false}),
            ),
            surface_operation(
                "maps.pathSummary",
                "Path summary",
                "Reports point count, segment count, length, and bounds for a flat 2D path.",
                serde_json::json!({"coordinates": [0.0, 0.0, 3.0, 4.0], "closed": false}),
            ),
            surface_operation(
                "maps.simplifyLine",
                "Simplify line",
                "Simplifies a flat open 2D line with deterministic Douglas-Peucker simplification.",
                serde_json::json!({"coordinates": [0.0, 0.0, 1.0, 0.01, 2.0, 0.0], "tolerance": 0.05}),
            ),
            surface_operation(
                "maps.densifyLine",
                "Densify line",
                "Inserts flat 2D line points so no segment exceeds the requested length.",
                serde_json::json!({"coordinates": [0.0, 0.0, 3.0, 0.0], "maxSegmentLength": 1.0}),
            ),
        ],
    }
}

/// Runs one library-owned operation.
pub fn run_surface_operation(request: SurfaceRequest) -> Result<SurfaceResponse, String> {
    let surface = package_surface();
    let operation = request.operation.clone();
    let value = match request.operation.as_str() {
        "describe" => return Ok(describe_surface_response(&surface, request)),
        "maps.kernelSummary" => summary_value(parse_surface_input(
            Some(operation.as_str()),
            request.input,
        )?)?,
        "maps.applyKernel" => apply_value(parse_surface_input(
            Some(operation.as_str()),
            request.input,
        )?)?,
        "maps.pathSummary" => path_summary_value(parse_surface_input(
            Some(operation.as_str()),
            request.input,
        )?)?,
        "maps.simplifyLine" => simplify_line_value(parse_surface_input(
            Some(operation.as_str()),
            request.input,
        )?)?,
        "maps.densifyLine" => densify_line_value(parse_surface_input(
            Some(operation.as_str()),
            request.input,
        )?)?,
        operation => {
            return Err(
                SurfaceError::unsupported_operation(operation, env!("CARGO_PKG_NAME"))
                    .to_error_string(),
            )
        }
    };
    Ok(structured_operation_response(&surface, operation, value))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryRequest {
    coordinates: Vec<f64>,
    #[serde(default)]
    closed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyRequest {
    coordinates: Vec<f64>,
    coordinate_count: usize,
    #[serde(default)]
    closed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimplifyRequest {
    coordinates: Vec<f64>,
    tolerance: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DensifyRequest {
    coordinates: Vec<f64>,
    max_segment_length: f64,
}

fn summary_value(request: SummaryRequest) -> Result<serde_json::Value, String> {
    validate_coordinates(&request.coordinates, request.closed)?;
    let coordinate_count = request.coordinates.len() / 2;
    let segment_count = if request.closed {
        coordinate_count
    } else {
        coordinate_count - 1
    };
    let bbox = bbox(&request.coordinates);
    Ok(serde_json::json!({
        "coordinateCount": coordinate_count,
        "closed": request.closed,
        "segmentCount": segment_count,
        "totalLength": total_length(&request.coordinates, request.closed),
        "bbox": bbox
    }))
}

fn path_summary_value(request: SummaryRequest) -> Result<serde_json::Value, String> {
    validate_coordinates(&request.coordinates, request.closed)?;
    let summary = path_summary_flat(&request.coordinates, request.closed)
        .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "pointCount": summary.point_count,
        "segmentCount": summary.segment_count,
        "closed": summary.closed,
        "length": summary.length,
        "bounds": summary.bounds
    }))
}

fn apply_value(request: ApplyRequest) -> Result<serde_json::Value, String> {
    validate_coordinates(&request.coordinates, request.closed)?;
    let coordinates = if request.closed {
        resample_ring_flat(&request.coordinates, request.coordinate_count)
    } else {
        resample_line_flat(&request.coordinates, request.coordinate_count)
    }
    .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "closed": request.closed,
        "coordinateCount": coordinates.len() / 2,
        "coordinates": coordinates
    }))
}

fn simplify_line_value(request: SimplifyRequest) -> Result<serde_json::Value, String> {
    validate_coordinates(&request.coordinates, false)?;
    let coordinates = simplify_line_flat(&request.coordinates, request.tolerance)
        .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "inputPointCount": request.coordinates.len() / 2,
        "outputPointCount": coordinates.len() / 2,
        "tolerance": request.tolerance,
        "coordinates": coordinates
    }))
}

fn densify_line_value(request: DensifyRequest) -> Result<serde_json::Value, String> {
    validate_coordinates(&request.coordinates, false)?;
    let coordinates = densify_line_flat(&request.coordinates, request.max_segment_length)
        .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "inputPointCount": request.coordinates.len() / 2,
        "outputPointCount": coordinates.len() / 2,
        "maxSegmentLength": request.max_segment_length,
        "coordinates": coordinates
    }))
}

fn validate_coordinates(coordinates: &[f64], closed: bool) -> Result<(), String> {
    if coordinates.len() > MAX_VALUES * 2 {
        return Err(format!("coordinates must not exceed {}", MAX_VALUES * 2));
    }
    if coordinates.len() < if closed { 6 } else { 4 } {
        return Err(if closed {
            "closed coordinates must contain at least three points".to_string()
        } else {
            "open coordinates must contain at least two points".to_string()
        });
    }
    if !coordinates.len().is_multiple_of(2) {
        return Err("coordinates length must be even".to_string());
    }
    if coordinates.iter().any(|value| !value.is_finite()) {
        return Err("coordinates must be finite".to_string());
    }
    Ok(())
}

fn bbox(coordinates: &[f64]) -> [f64; 4] {
    let mut min_x = coordinates[0];
    let mut min_y = coordinates[1];
    let mut max_x = coordinates[0];
    let mut max_y = coordinates[1];
    for point in coordinates.chunks_exact(2).skip(1) {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    [min_x, min_y, max_x, max_y]
}

fn total_length(coordinates: &[f64], closed: bool) -> f64 {
    let point_count = coordinates.len() / 2;
    let segment_count = if closed { point_count } else { point_count - 1 };
    (0..segment_count)
        .map(|index| {
            let next = (index + 1) % point_count;
            let x0 = coordinates[index * 2];
            let y0 = coordinates[index * 2 + 1];
            let x1 = coordinates[next * 2];
            let y1 = coordinates[next * 2 + 1];
            (x1 - x0).hypot(y1 - y0)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_core::OperationId;

    #[test]
    fn summary_reports_lengths_and_bbox() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("maps.kernelSummary"),
            input: serde_json::json!({"coordinates": [0.0, 0.0, 3.0, 4.0], "closed": false}),
        })
        .expect("summary");
        assert_eq!(response.value["coordinateCount"], 2);
        assert_eq!(response.value["totalLength"], 5.0);
        assert_eq!(
            response.value["bbox"],
            serde_json::json!([0.0, 0.0, 3.0, 4.0])
        );
    }

    #[test]
    fn apply_kernel_resamples_line() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("maps.applyKernel"),
            input: serde_json::json!({"coordinates": [0.0, 0.0, 10.0, 0.0], "coordinateCount": 3}),
        })
        .expect("apply");
        assert_eq!(
            response.value["coordinates"],
            serde_json::json!([0.0, 0.0, 5.0, 0.0, 10.0, 0.0])
        );
    }

    #[test]
    fn new_path_operations_run() {
        for operation in ["maps.pathSummary", "maps.simplifyLine", "maps.densifyLine"] {
            let surface_operation = package_surface()
                .operations
                .into_iter()
                .find(|candidate| candidate.id.as_str() == operation)
                .expect("operation metadata");
            let response = run_surface_operation(SurfaceRequest {
                operation: surface_operation.id,
                input: surface_operation.example_request,
            })
            .unwrap_or_else(|error| panic!("{operation} failed: {error}"));
            assert!(response.value.is_object());
        }
    }
}
