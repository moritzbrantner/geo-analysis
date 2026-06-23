#[test]
fn package_endpoint_reports_wrapped_library() {
    let response = geo_io_osm_server::response_for("GET", "/api/package", "");
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("geo-io-osm"));
}

#[test]
fn operations_endpoint_reports_package_operation() {
    let response = geo_io_osm_server::response_for("GET", "/api/operations", "");
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("osm.filterPbfBase64"));
}

#[test]
fn run_endpoint_calls_library_surface() {
    let response = geo_io_osm_server::response_for(
        "POST",
        "/api/run",
        r#"{"operation":"osm.filterPbfBase64","input":{"pbfBase64":"","spec":{"filter":{"types":["node"]}}}}"#,
    );
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("featureCount"));
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("response JSON");
    assert_eq!(body["operation"], "osm.filterPbfBase64");
}
