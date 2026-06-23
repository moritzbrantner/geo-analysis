#[test]
fn cli_adapter_describes_and_runs_osm_primary_operation() {
    assert_eq!(geo_io_osm_cli::LIBRARY_CRATE, "geo-io-osm");
    let surface = geo_io_osm_cli::package_surface();
    assert_eq!(surface.library, "moenarch-geo-io-osm");
    assert!(surface
        .operations
        .iter()
        .any(|operation| operation.id.as_str() == "osm.filterPbfBase64"));

    let describe =
        geo_io_osm_cli::run_operation("describe", serde_json::json!({"includeOperations": true}))
            .expect("describe operation");
    assert_eq!(describe.operation.as_str(), "describe");
    assert_eq!(describe.value["result"]["library"], "moenarch-geo-io-osm");

    let filtered = geo_io_osm_cli::run_operation(
        "osm.filterPbfBase64",
        serde_json::json!({"pbfBase64": "", "spec": {"filter": {"types": ["node"]}}}),
    )
    .expect("primary operation");
    assert_eq!(filtered.value["result"]["featureCount"], 0);
}
