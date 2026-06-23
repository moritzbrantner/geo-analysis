#[test]
fn cli_adapter_describes_and_runs_geojson_primary_operation() {
    assert_eq!(geo_io_geojson_cli::LIBRARY_CRATE, "geo-io-geojson");
    let surface = geo_io_geojson_cli::package_surface();
    assert_eq!(surface.library, "moenarch-geo-io-geojson");
    assert!(surface
        .operations
        .iter()
        .any(|operation| operation.id.as_str() == "geoJson.bounds"));

    let describe = geo_io_geojson_cli::run_operation(
        "describe",
        serde_json::json!({"includeOperations": true}),
    )
    .expect("describe operation");
    assert_eq!(describe.operation.as_str(), "describe");
    assert_eq!(
        describe.value["result"]["library"],
        "moenarch-geo-io-geojson"
    );

    let bounds = geo_io_geojson_cli::run_operation(
        "geoJson.bounds",
        serde_json::json!({"geoJson": {"type": "Point", "coordinates": [8.0, 49.0]}}),
    )
    .expect("primary operation");
    assert_eq!(bounds.value["result"]["coordinateCount"], 1);
}
