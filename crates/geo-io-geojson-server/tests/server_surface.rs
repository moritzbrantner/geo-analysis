#[test]
fn package_endpoint_reports_wrapped_library() {
    let response = geo_io_geojson_server::response_for("GET", "/api/package", "");
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("geo-io-geojson"));
}

#[test]
fn operations_endpoint_reports_package_operation() {
    let response = geo_io_geojson_server::response_for("GET", "/api/operations", "");
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("geoJson.bounds"));
}

#[test]
fn run_endpoint_calls_library_surface() {
    let response = geo_io_geojson_server::response_for(
        "POST",
        "/api/run",
        r#"{"operation":"geoJson.bounds","input":{"geoJson":{"type":"Point","coordinates":[8,49]}}}"#,
    );
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("coordinateCount"));
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("response JSON");
    assert_eq!(body["operation"], "geoJson.bounds");
}
