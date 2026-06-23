//! WASM bindings for `geo-core`.

use runtime_core::SurfaceRequest;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = packageSurface)]
pub fn package_surface() -> Result<JsValue, JsValue> {
    to_json_value(&geo_core::surface::package_surface())
}

#[wasm_bindgen(js_name = runOperation)]
pub fn run_operation(request: JsValue) -> Result<JsValue, JsValue> {
    let request: SurfaceRequest = serde_wasm_bindgen::from_value(request).map_err(into_js_error)?;
    let response = geo_core::surface::run_surface_operation(request).map_err(into_js_error)?;
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
        let surface = geo_core::surface::package_surface();
        assert_eq!(surface.library, "moenarch-geo-core");
        assert!(!surface.operations.is_empty());
        let operation_ids = surface
            .operations
            .iter()
            .map(|operation| operation.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            operation_ids,
            vec!["describe", "geo.bounds", "geo.distance"]
        );
    }

    #[test]
    fn wrapped_surface_runs_operation() {
        let response = geo_core::surface::run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geo.distance"),
            input: serde_json::json!({"from": [0.0, 0.0], "to": [3.0, 4.0], "mode": "planar"}),
        })
        .expect("distance operation");

        assert_eq!(response.value["distanceUnits"], serde_json::json!(5.0));
    }
}
