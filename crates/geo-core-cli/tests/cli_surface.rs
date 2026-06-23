#[test]
fn cli_adapter_reports_wrapped_library() {
    assert_eq!(geo_core_cli::LIBRARY_CRATE, "geo-core");
    let surface = geo_core_cli::package_surface();
    assert_eq!(surface.library, "moenarch-geo-core");
    assert!(surface
        .operations
        .iter()
        .any(|operation| operation.id.as_str() == "geo.bounds"));

    let response =
        geo_core_cli::run_operation("describe", serde_json::json!({"includeOperations": true}))
            .expect("describe operation");
    assert_eq!(response.operation.as_str(), "describe");
    assert_eq!(response.value["operation"], "describe");
    assert_eq!(response.value["result"]["library"], "moenarch-geo-core");
}
