//! Library-owned runtime surface for `geo-viz`.

use maps_kernels_core::{resample_line_flat, resample_ring_flat};
use runtime_core::{
    OperationId, PackageSurface, RuntimeCapabilities, SurfaceOperation, SurfaceRequest,
    SurfaceResponse,
};
use serde::Deserialize;

use crate::{
    create_scalar_field_grid, GeoFlowIndex, GeoJsonIndex, GeoPointIndex, GeoVizAggregationOptions,
    GeoVizFlow, GeoVizFlowOptions, GeoVizGeoJsonOptions, GeoVizHeatOptions, GeoVizPoint,
    GeoVizScalarFieldOptions, GeoVizViewportQuery,
};

/// Returns the package surface exposed by every transport wrapper.
pub fn package_surface() -> PackageSurface {
    PackageSurface {
        library: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: RuntimeCapabilities::pure_rust(),
        operations: vec![
            operation(
                "describe",
                "Describe geo visualization package",
                "Describes geo-viz operations.",
                serde_json::json!({"includeOperations": true}),
            ),
            operation(
                "geoViz.bounds",
                "Geo visualization bounds",
                "Computes geographic bounds for finite lon/lat points.",
                serde_json::json!({"points": [{"longitude": 8.0, "latitude": 49.0}]}),
            ),
            operation(
                "geoViz.aggregateViewport",
                "Aggregate geo viewport",
                "Clusters points and returns renderer-agnostic viewport features.",
                serde_json::json!({
                    "points": [{"id": "a", "longitude": 8.0, "latitude": 49.0}],
                    "query": {"bounds": [7.0, 48.0, 9.0, 50.0], "zoom": 8}
                }),
            ),
            operation(
                "geoViz.heatViewport",
                "Geo heat viewport",
                "Returns weighted heat features for visible geographic points.",
                serde_json::json!({
                    "points": [{"id": "a", "longitude": 8.0, "latitude": 49.0, "metrics": {"weight": 2}}],
                    "query": {"bounds": [7.0, 48.0, 9.0, 50.0], "zoom": 8}
                }),
            ),
            operation(
                "geoViz.geoJsonViewport",
                "GeoJSON viewport",
                "Filters GeoJSON features for a viewport and returns a FeatureCollection.",
                serde_json::json!({
                    "geoJson": {"type": "FeatureCollection", "features": []},
                    "query": {"bounds": [7.0, 48.0, 9.0, 50.0], "zoom": 8}
                }),
            ),
            operation(
                "geoViz.flowViewport",
                "Geo flow viewport",
                "Filters and optionally aggregates geographic flows for a viewport.",
                serde_json::json!({
                    "flows": [{"from": [8.0, 49.0], "to": [9.0, 50.0], "metrics": {"weight": 2}}],
                    "query": {"bounds": [7.0, 48.0, 10.0, 51.0], "zoom": 8}
                }),
            ),
            operation(
                "geoViz.resampleGeometry",
                "Resample map geometry",
                "Resamples flat 2D coordinates as an open line or closed ring.",
                serde_json::json!({
                    "coordinates": [0.0, 0.0, 10.0, 0.0],
                    "coordinateCount": 3,
                    "closed": false
                }),
            ),
            operation(
                "geoViz.scalarFieldGrid",
                "Scalar field grid",
                "Creates an IDW scalar field grid for geographic value points.",
                serde_json::json!({
                    "points": [{"id": "a", "longitude": 8.0, "latitude": 49.0, "metrics": {"value": 2}}],
                    "options": {"domainBounds": [7.0, 48.0, 9.0, 50.0], "fieldColumns": 8, "fieldRows": 4}
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
        "geoViz.describe" | "describe" => describe_value(request.input),
        "geoViz.bounds" => bounds_value(parse_input(request.input)?)?,
        "geoViz.aggregateViewport" => aggregate_value(parse_input(request.input)?)?,
        "geoViz.heatViewport" => heat_value(parse_input(request.input)?)?,
        "geoViz.geoJsonViewport" => geojson_value(parse_input(request.input)?)?,
        "geoViz.flowViewport" => flow_value(parse_input(request.input)?)?,
        "geoViz.resampleGeometry" => resample_value(parse_input(request.input)?)?,
        "geoViz.scalarFieldGrid" => scalar_field_grid_value(parse_input(request.input)?)?,
        operation => {
            return Err(format!(
                "unsupported operation `{operation}` for {}",
                env!("CARGO_PKG_NAME")
            ));
        }
    };
    Ok(response(operation, value))
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

fn response(operation: OperationId, value: serde_json::Value) -> SurfaceResponse {
    SurfaceResponse {
        operation,
        value,
        diagnostics: Vec::new(),
        artifacts: Vec::new(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoundsRequest {
    points: Vec<GeoVizPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AggregateRequest {
    points: Vec<GeoVizPoint>,
    query: GeoVizViewportQuery,
    #[serde(default)]
    options: GeoVizAggregationOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HeatRequest {
    points: Vec<GeoVizPoint>,
    query: GeoVizViewportQuery,
    #[serde(default)]
    options: Option<GeoVizHeatOptions>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeoJsonRequest {
    geo_json: serde_json::Value,
    query: GeoVizViewportQuery,
    #[serde(default)]
    options: GeoVizGeoJsonOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlowRequest {
    flows: Vec<GeoVizFlow>,
    query: GeoVizViewportQuery,
    #[serde(default)]
    options: GeoVizFlowOptions,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResampleRequest {
    coordinates: Vec<f64>,
    coordinate_count: usize,
    #[serde(default)]
    closed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScalarFieldGridRequest {
    points: Vec<GeoVizPoint>,
    #[serde(default)]
    options: GeoVizScalarFieldOptions,
}

fn bounds_value(request: BoundsRequest) -> Result<serde_json::Value, String> {
    let index = GeoPointIndex::new(request.points, GeoVizAggregationOptions::default())
        .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({ "bounds": index.get_bounds() }))
}

fn aggregate_value(request: AggregateRequest) -> Result<serde_json::Value, String> {
    let index =
        GeoPointIndex::new(request.points, request.options).map_err(|error| error.to_string())?;
    serde_json::to_value(
        index
            .get_viewport_aggregation(request.query)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn heat_value(request: HeatRequest) -> Result<serde_json::Value, String> {
    let index = GeoPointIndex::new(request.points, GeoVizAggregationOptions::default())
        .map_err(|error| error.to_string())?;
    serde_json::to_value(
        index
            .get_heat_features(
                request.query,
                request.options.unwrap_or(GeoVizHeatOptions {
                    radius_meters: None,
                    weight_metric: None,
                }),
            )
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn geojson_value(request: GeoJsonRequest) -> Result<serde_json::Value, String> {
    let index = GeoJsonIndex::new(request.geo_json).map_err(|error| error.to_string())?;
    serde_json::to_value(
        index
            .get_viewport_features(request.query, request.options)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn flow_value(request: FlowRequest) -> Result<serde_json::Value, String> {
    let index = GeoFlowIndex::new(request.flows).map_err(|error| error.to_string())?;
    serde_json::to_value(
        index
            .get_viewport_flows(request.query, request.options)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn resample_value(request: ResampleRequest) -> Result<serde_json::Value, String> {
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

fn scalar_field_grid_value(request: ScalarFieldGridRequest) -> Result<serde_json::Value, String> {
    serde_json::to_value(
        create_scalar_field_grid(request.points, request.options)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn parse_input<T: for<'de> Deserialize<'de>>(input: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|error| format!("invalid request: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_exposes_geo_viz_operations() {
        let surface = package_surface();
        assert_eq!(surface.library, env!("CARGO_PKG_NAME"));
        assert!(surface
            .operations
            .iter()
            .any(|op| op.id.as_str() == "geoViz.aggregateViewport"));
        assert!(surface
            .operations
            .iter()
            .any(|op| op.id.as_str() == "geoViz.scalarFieldGrid"));
    }

    #[test]
    fn resample_geometry_delegates_to_map_kernel() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geoViz.resampleGeometry"),
            input: serde_json::json!({
                "coordinates": [0.0, 0.0, 10.0, 0.0],
                "coordinateCount": 3,
                "closed": false
            }),
        })
        .expect("resample geometry");

        assert_eq!(
            response.value["coordinates"],
            serde_json::json!([0.0, 0.0, 5.0, 0.0, 10.0, 0.0])
        );
    }

    #[test]
    fn scalar_field_grid_operation_runs_idw() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geoViz.scalarFieldGrid"),
            input: serde_json::json!({
                "points": [
                    {"id": "cold", "longitude": 0.0, "latitude": 0.0, "metrics": {"temperature": 0.0}},
                    {"id": "warm", "longitude": 2.0, "latitude": 0.0, "metrics": {"temperature": 20.0}}
                ],
                "options": {
                    "domainBounds": [0.0, -1.0, 2.0, 1.0],
                    "fieldColumns": 2,
                    "fieldRows": 1,
                    "interpolationK": 2,
                    "valueMetric": "temperature"
                }
            }),
        })
        .expect("scalar field grid");

        assert_eq!(response.value["columns"], serde_json::json!(2));
        assert_eq!(response.value["rows"], serde_json::json!(1));
        let domain = response.value["valueDomain"].as_array().expect("domain");
        assert!((domain[0].as_f64().expect("min") - 2.0).abs() < 1e-12);
        assert!((domain[1].as_f64().expect("max") - 18.0).abs() < 1e-12);
    }
}
