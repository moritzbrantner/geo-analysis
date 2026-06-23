#[test]
fn cli_adapter_reports_wrapped_library() {
    assert_eq!(geo_clustering_cli::LIBRARY_CRATE, "geo-clustering");
    let surface = geo_clustering_cli::package_surface();
    assert_eq!(surface.library, "moenarch-geo-clustering");
    assert!(surface
        .operations
        .iter()
        .any(|operation| operation.id.as_str() == "geoCluster.viewport"));

    let response = geo_clustering_cli::run_operation(
        "describe",
        serde_json::json!({"includeOperations": true}),
    )
    .expect("describe operation");
    assert_eq!(response.operation.as_str(), "describe");
    assert_eq!(response.value["operation"], "describe");
    assert_eq!(
        response.value["result"]["library"],
        "moenarch-geo-clustering"
    );
}
