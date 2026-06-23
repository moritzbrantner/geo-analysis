#[test]
fn package_endpoint_reports_wrapped_library() {
    let response = geo_clustering_server::response_for("GET", "/api/package", "");
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("geo-clustering"));
}

#[test]
fn operations_endpoint_reports_package_operation() {
    let response = geo_clustering_server::response_for("GET", "/api/operations", "");
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains("geoCluster.viewport"));
}

#[test]
fn run_endpoint_calls_library_surface() {
    let response = geo_clustering_server::response_for(
        "POST",
        "/api/run",
        r#"{"operation":"geoCluster.bounds","input":{"points":[{"id":"a","longitude":8,"latitude":49,"properties":{}}]}}"#,
    );
    assert_eq!(response.status_code, 200);
    assert!(response.body.contains(r#""bounds""#));
    let body: serde_json::Value = serde_json::from_str(&response.body).expect("response JSON");
    assert_eq!(body["operation"], "geoCluster.bounds");
}
