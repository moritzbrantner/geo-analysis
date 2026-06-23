use std::collections::BTreeSet;
use std::process::Command;

use runtime_core::{OperationId, SurfaceRequest};
use serde_json::Value;

#[test]
fn geo_clustering_surface_exposes_and_runs_viewport_operations() {
    let surface = geo_clustering::surface::package_surface();

    assert_eq!(surface.library, "moenarch-geo-clustering");
    let operations = surface
        .operations
        .iter()
        .map(|operation| operation.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        operations,
        vec!["describe", "geoCluster.viewport", "geoCluster.bounds"]
    );

    let viewport = geo_clustering::surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new("geoCluster.viewport"),
        input: serde_json::json!({
            "points": [
                {"id": "a", "longitude": 13.0, "latitude": 52.0, "properties": {"kind": "station"}},
                {"id": "b", "longitude": 13.0001, "latitude": 52.0001, "properties": {"kind": "station"}}
            ],
            "bounds": [12.0, 51.0, 14.0, 53.0],
            "zoom": 1
        }),
    })
    .expect("viewport operation");
    assert_eq!(viewport.operation.as_str(), "geoCluster.viewport");
    assert_eq!(viewport.value["items"].as_array().expect("items").len(), 1);

    let bounds = geo_clustering::surface::run_surface_operation(SurfaceRequest {
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
        bounds.value["bounds"],
        serde_json::json!([13.0, 52.0, 14.0, 53.0])
    );
}

#[test]
fn geo_viz_surface_exposes_and_runs_renderer_operations() {
    let surface = geo_viz::surface::package_surface();

    assert_eq!(surface.library, "moenarch-geo-viz");
    let operations = surface
        .operations
        .iter()
        .map(|operation| operation.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        operations,
        vec![
            "describe",
            "geoViz.bounds",
            "geoViz.aggregateViewport",
            "geoViz.heatViewport",
            "geoViz.geoJsonViewport",
            "geoViz.flowViewport",
            "geoViz.resampleGeometry",
            "geoViz.scalarFieldGrid"
        ]
    );

    let query = serde_json::json!({"bounds": [7.0, 48.0, 10.0, 51.0], "zoom": 8});
    let points = serde_json::json!([
        {"id": "a", "longitude": 8.0, "latitude": 49.0, "metrics": {"weight": 2.0, "value": 3.0}},
        {"id": "b", "longitude": 9.0, "latitude": 50.0, "metrics": {"weight": 1.0, "value": 5.0}}
    ]);

    let aggregate = geo_viz::surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new("geoViz.aggregateViewport"),
        input: serde_json::json!({"points": points, "query": query}),
    })
    .expect("aggregate viewport");
    assert_eq!(aggregate.value["summary"]["visiblePointCount"], 2);

    let heat = geo_viz::surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new("geoViz.heatViewport"),
        input: serde_json::json!({"points": points, "query": query}),
    })
    .expect("heat viewport");
    assert_eq!(heat.value["summary"]["visiblePointCount"], 2);

    let geojson = geo_viz::surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new("geoViz.geoJsonViewport"),
        input: serde_json::json!({
            "geoJson": {
                "type": "FeatureCollection",
                "features": [
                    {"type": "Feature", "properties": {"name": "inside"}, "geometry": {"type": "Point", "coordinates": [8.0, 49.0]}}
                ]
            },
            "query": query
        }),
    })
    .expect("geojson viewport");
    assert_eq!(geojson.value["featureCount"], 1);

    let flow = geo_viz::surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new("geoViz.flowViewport"),
        input: serde_json::json!({
            "flows": [{"from": [8.0, 49.0], "to": [9.0, 50.0], "metrics": {"weight": 2.0}}],
            "query": query
        }),
    })
    .expect("flow viewport");
    assert_eq!(flow.value["summary"]["visibleFlowCount"], 1);

    let scalar = geo_viz::surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new("geoViz.scalarFieldGrid"),
        input: serde_json::json!({
            "points": points,
            "options": {"domainBounds": [7.0, 48.0, 10.0, 51.0], "fieldColumns": 2, "fieldRows": 1}
        }),
    })
    .expect("scalar field grid");
    assert_eq!(scalar.value["columns"], 2);
    assert_eq!(scalar.value["rows"], 1);
}

#[test]
fn geo_viz_uses_maps_kernels_core_as_internal_foundation_dependency() {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: Value = serde_json::from_slice(&output.stdout).expect("parse cargo metadata");
    let packages = metadata["packages"]
        .as_array()
        .expect("metadata packages array");
    let package = packages
        .iter()
        .find(|candidate| candidate["name"].as_str() == Some("moenarch-geo-viz"))
        .expect("moenarch-geo-viz package");

    let deps = package["dependencies"]
        .as_array()
        .expect("dependencies array")
        .iter()
        .filter(|dependency| dependency["kind"].is_null())
        .map(|dependency| {
            (
                dependency["rename"]
                    .as_str()
                    .or_else(|| dependency["name"].as_str())
                    .expect("dependency name")
                    .to_string(),
                dependency["source"].as_str().map(str::to_string),
            )
        })
        .collect::<BTreeSet<_>>();

    assert!(deps.contains(&("geo-clustering".to_string(), None)));
    assert!(deps.contains(&("maps-kernels-core".to_string(), None)));
}
