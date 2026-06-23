#[test]
fn package_endpoint_reports_wrapped_library() {
    let response = geo_core_server::response_for("GET", "/api/package", "");
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("geo-core"));
}

#[test]
fn operations_endpoint_reports_package_operation() {
    let response = geo_core_server::response_for("GET", "/api/operations", "");
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("geo.bounds"));
}

#[test]
fn run_endpoint_calls_library_surface() {
    let response = geo_core_server::response_for(
        "POST",
        "/api/run",
        r#"{"operation":"geo.distance","input":{"from":[0,0],"to":[3,4],"mode":"planar"}}"#,
    );
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("distanceUnits"));
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("response JSON");
    assert_eq!(body["operation"], "geo.distance");
}
