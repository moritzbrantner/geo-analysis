use std::collections::BTreeSet;
use std::process::Command;

use runtime_core::{OperationId, SurfaceRequest};
use serde_json::Value;

#[test]
fn geo_core_surface_exposes_workflow_operations_under_moenarch_name() {
    let surface = geo_core::surface::package_surface();

    assert_eq!(surface.library, "moenarch-geo-core");
    let operations = surface
        .operations
        .iter()
        .map(|operation| operation.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(operations, vec!["describe", "geo.bounds", "geo.distance"]);
}

#[test]
fn geo_core_distance_operation_runs_through_public_surface() {
    let response = geo_core::surface::run_surface_operation(SurfaceRequest {
        operation: OperationId::new("geo.distance"),
        input: serde_json::json!({
            "from": [0.0, 0.0],
            "to": [3.0, 4.0],
            "mode": "planar"
        }),
    })
    .expect("distance operation");

    assert_eq!(response.operation.as_str(), "geo.distance");
    assert_eq!(response.value["distanceUnits"], serde_json::json!(5.0));
}

#[test]
fn geo_core_uses_published_foundation_dependencies() {
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
        .find(|candidate| candidate["name"].as_str() == Some("moenarch-geo-core"))
        .expect("moenarch-geo-core package");

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

    assert!(deps.contains(&(
        "runtime-core".to_string(),
        Some("registry+https://github.com/rust-lang/crates.io-index".to_string()),
    )));
    assert!(!deps
        .iter()
        .any(|(name, source)| name == "runtime-core" && source.is_none()));
}
