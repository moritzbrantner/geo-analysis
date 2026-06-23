use std::collections::BTreeSet;
use std::process::Command;

use runtime_core::{OperationId, SurfaceRequest};
use serde_json::Value;

#[test]
fn geojson_surface_exposes_workflow_operations_under_moenarch_name() {
    let surface = geo_io_geojson::surface::package_surface();

    assert_eq!(surface.library, "moenarch-geo-io-geojson");
    let operations = surface
        .operations
        .iter()
        .map(|operation| operation.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        operations,
        vec![
            "describe",
            "geoJson.bounds",
            "geoJson.distance",
            "geoJson.toGeoJson"
        ]
    );
}

#[test]
fn geojson_primary_operation_runs_through_public_surface() {
    let response = geo_io_geojson::surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new("geoJson.bounds"),
        input: serde_json::json!({"geoJson": {"type": "Point", "coordinates": [8.0, 49.0]}}),
    })
    .expect("bounds operation");

    assert_eq!(response.operation.as_str(), "geoJson.bounds");
    assert_eq!(response.value["coordinateCount"], 1);
}

#[test]
fn osm_surface_exposes_workflow_operations_under_moenarch_name() {
    let surface = geo_io_osm::surface::package_surface();

    assert_eq!(surface.library, "moenarch-geo-io-osm");
    let operations = surface
        .operations
        .iter()
        .map(|operation| operation.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        operations,
        vec![
            "describe",
            "osm.validateSpec",
            "osm.filterSummary",
            "osm.filterPbfBase64"
        ]
    );
}

#[test]
fn osm_primary_operation_runs_through_public_surface() {
    let response = geo_io_osm::surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new("osm.filterPbfBase64"),
        input: serde_json::json!({"pbfBase64": "", "spec": {"filter": {"types": ["node"]}}}),
    })
    .expect("filter pbf operation");

    assert_eq!(response.operation.as_str(), "osm.filterPbfBase64");
    assert_eq!(response.value["featureCount"], 0);
}

#[test]
fn geo_io_uses_workspace_internal_geo_dependencies() {
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
    for package_name in ["moenarch-geo-io-geojson", "moenarch-geo-io-osm"] {
        let package = packages
            .iter()
            .find(|candidate| candidate["name"].as_str() == Some(package_name))
            .expect("geo I/O package");

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

        assert!(deps.contains(&("geo-core".to_string(), None)));
    }
}
