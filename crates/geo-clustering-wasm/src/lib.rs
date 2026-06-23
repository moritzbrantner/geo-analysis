//! WASM bindings for `geo-clustering`.

use runtime_core::SurfaceRequest;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = packageSurface)]
pub fn package_surface() -> Result<JsValue, JsValue> {
    to_json_value(&geo_clustering::surface::package_surface())
}

#[wasm_bindgen(js_name = runOperation)]
pub fn run_operation(request: JsValue) -> Result<JsValue, JsValue> {
    let request: SurfaceRequest = serde_wasm_bindgen::from_value(request).map_err(into_js_error)?;
    let response =
        geo_clustering::surface::run_surface_operation(request).map_err(into_js_error)?;
    to_json_value(&response)
}

fn to_json_value(value: &impl serde::Serialize) -> Result<JsValue, JsValue> {
    let json = serde_json::to_string(value).map_err(into_js_error)?;
    js_sys::JSON::parse(&json)
}

fn into_js_error(error: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&error.to_string()).into()
}

#[cfg(test)]
mod tests {
    use runtime_core::{OperationId, SurfaceRequest};

    #[test]
    fn wrapped_surface_has_operations() {
        let surface = geo_clustering::surface::package_surface();
        assert_eq!(surface.library, "moenarch-geo-clustering");
        assert!(!surface.operations.is_empty());
        let operation_ids = surface
            .operations
            .iter()
            .map(|operation| operation.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            operation_ids,
            vec!["describe", "geoCluster.viewport", "geoCluster.bounds"]
        );
    }

    #[test]
    fn wrapped_surface_runs_operation() {
        let response = geo_clustering::surface::run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geoCluster.bounds"),
            input: serde_json::json!({
                "points": [{"id": "a", "longitude": 8.0, "latitude": 49.0, "properties": {}}]
            }),
        })
        .expect("bounds operation");

        assert_eq!(
            response.value["bounds"],
            serde_json::json!([8.0, 49.0, 8.0, 49.0])
        );
    }
}
