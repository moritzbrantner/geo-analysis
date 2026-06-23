//! Library-owned runtime surface for `geo-clustering`.

use runtime_core::{
    OperationId, PackageSurface, RuntimeCapabilities, SurfaceOperation, SurfaceRequest,
    SurfaceResponse,
};
use serde::Deserialize;

use crate::{ClusterIndex, ClusterOptions, ClusterPoint};

/// Returns the package surface exposed by every transport wrapper.
pub fn package_surface() -> PackageSurface {
    PackageSurface {
        library: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: RuntimeCapabilities::pure_rust(),
        operations: vec![
            operation(
                "describe",
                "Describe package",
                "Format-agnostic geospatial point clustering.",
                serde_json::json!({"includeOperations": true}),
            ),
            operation(
                "geoCluster.viewport",
                "Cluster viewport",
                "Returns clusters or points for a bounding box and zoom level.",
                serde_json::json!({
                    "points": [{"id": "a", "longitude": 8.0, "latitude": 49.0, "properties": {}}],
                    "bounds": [7.0, 48.0, 9.0, 50.0],
                    "zoom": 8
                }),
            ),
            operation(
                "geoCluster.bounds",
                "Cluster point bounds",
                "Computes bounds for finite cluster input points.",
                serde_json::json!({
                    "points": [{"id": "a", "longitude": 8.0, "latitude": 49.0, "properties": {}}]
                }),
            ),
        ],
    }
}

fn operation(
    id: &str,
    name: &str,
    description: &str,
    example_request: serde_json::Value,
) -> SurfaceOperation {
    SurfaceOperation {
        id: OperationId::new(id),
        name: name.to_string(),
        description: Some(description.to_string()),
        curation: runtime_core::SurfaceOperationCuration::from_operation_id(id),
        input_schema: serde_json::json!({"type": "object", "additionalProperties": true, "xOperationCategory": runtime_core::operation_category(id)}),
        output_schema: serde_json::json!({"type": "object", "xOperationCategory": runtime_core::operation_category(id)}),
        example_request,
        wasm_supported: true,
        server_supported: true,
    }
}

/// Runs one library-owned operation.
pub fn run_surface_operation(request: SurfaceRequest) -> Result<SurfaceResponse, String> {
    let operation = request.operation.clone();
    let value = match request.operation.as_str() {
        "describe" | "geoCluster.describe" => describe_value(request.input),
        "geoCluster.viewport" => viewport_value(parse_input(request.input)?)?,
        "geoCluster.bounds" => bounds_value(parse_input(request.input)?)?,
        operation => {
            return Err(format!(
                "unsupported operation `{operation}` for {}",
                env!("CARGO_PKG_NAME")
            ));
        }
    };
    Ok(SurfaceResponse {
        operation,
        value,
        diagnostics: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn describe_value(input: serde_json::Value) -> serde_json::Value {
    let surface = package_surface();
    serde_json::json!({
        "library": surface.library,
        "version": surface.version,
        "operationCount": surface.operations.len(),
        "operations": surface
            .operations
            .iter()
            .map(|operation| operation.id.as_str())
            .collect::<Vec<_>>(),
        "input": input
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewportRequest {
    points: Vec<ClusterPoint<serde_json::Value>>,
    bounds: [f64; 4],
    zoom: u8,
    #[serde(default)]
    options: ClusterOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoundsRequest {
    points: Vec<ClusterPoint<serde_json::Value>>,
    #[serde(default)]
    options: ClusterOptions,
}

fn viewport_value(request: ViewportRequest) -> Result<serde_json::Value, String> {
    let index =
        ClusterIndex::new(request.points, request.options).map_err(|error| error.to_string())?;
    let items = index
        .get_clusters(request.bounds, request.zoom)
        .map_err(|error| error.to_string())?;
    serde_json::to_value(serde_json::json!({
        "bounds": request.bounds,
        "zoom": request.zoom,
        "items": items
    }))
    .map_err(|error| error.to_string())
}

fn bounds_value(request: BoundsRequest) -> Result<serde_json::Value, String> {
    let index =
        ClusterIndex::new(request.points, request.options).map_err(|error| error.to_string())?;
    Ok(serde_json::json!({ "bounds": index.get_bounds() }))
}

fn parse_input<T: for<'de> Deserialize<'de>>(input: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|error| format!("invalid request: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_clusters_viewport() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geoCluster.viewport"),
            input: serde_json::json!({
                "points": [
                    {"id": "a", "longitude": 13.0, "latitude": 52.0, "properties": {}},
                    {"id": "b", "longitude": 13.0001, "latitude": 52.0001, "properties": {}}
                ],
                "bounds": [12.0, 51.0, 14.0, 53.0],
                "zoom": 1
            }),
        })
        .expect("cluster operation");

        assert_eq!(response.value["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn surface_reports_bounds() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geoCluster.bounds"),
            input: serde_json::json!({
                "points": [
                    {"id": "a", "longitude": 13.0, "latitude": 52.0, "properties": {}},
                    {"id": "b", "longitude": 14.0, "latitude": 53.0, "properties": {}}
                ]
            }),
        })
        .expect("bounds operation");

        assert_eq!(
            response.value["bounds"],
            serde_json::json!([13.0, 52.0, 14.0, 53.0])
        );
    }
}
