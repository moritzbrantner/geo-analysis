//! WASM bindings for `geo-viz`.

use runtime_core::SurfaceRequest;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = GeoPointIndex)]
pub struct WasmGeoPointIndex {
    inner: geo_viz::GeoPointIndex,
}

#[wasm_bindgen(js_name = GeoFlowIndex)]
pub struct WasmGeoFlowIndex {
    inner: geo_viz::GeoFlowIndex,
}

#[wasm_bindgen(js_name = GeoJsonIndex)]
pub struct WasmGeoJsonIndex {
    inner: geo_viz::GeoJsonIndex,
}

#[wasm_bindgen(js_name = ScalarFieldIndex)]
pub struct WasmScalarFieldIndex {
    inner: geo_viz::GeoVizScalarFieldIndex,
}

#[wasm_bindgen(js_class = GeoPointIndex)]
impl WasmGeoPointIndex {
    #[wasm_bindgen(constructor)]
    pub fn new(points: JsValue, options: JsValue) -> Result<WasmGeoPointIndex, JsValue> {
        let points: Vec<geo_viz::GeoVizPoint> =
            serde_wasm_bindgen::from_value(points).map_err(into_js_error)?;
        let options = if options.is_undefined() || options.is_null() {
            geo_viz::GeoVizAggregationOptions::default()
        } else {
            serde_wasm_bindgen::from_value(options).map_err(into_js_error)?
        };
        let inner = geo_viz::GeoPointIndex::new(points, options).map_err(into_js_error)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen(js_name = getBounds)]
    pub fn get_bounds(&self) -> Result<JsValue, JsValue> {
        to_json_value(&self.inner.get_bounds())
    }

    #[wasm_bindgen(js_name = getPointById)]
    pub fn get_point_by_id(&self, point_id: String) -> Result<JsValue, JsValue> {
        to_json_value(&self.inner.get_point_by_id(&point_id))
    }

    #[wasm_bindgen(js_name = getViewportAggregation)]
    pub fn get_viewport_aggregation(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let query: geo_viz::GeoVizViewportQuery =
            serde_wasm_bindgen::from_value(query).map_err(into_js_error)?;
        to_json_value(
            &self
                .inner
                .get_viewport_aggregation(query)
                .map_err(into_js_error)?,
        )
    }

    #[wasm_bindgen(js_name = getClusterExpansionZoom)]
    pub fn get_cluster_expansion_zoom(&self, cluster_id: String) -> usize {
        self.inner.get_cluster_expansion_zoom(&cluster_id)
    }

    #[wasm_bindgen(js_name = getClusterLeaves)]
    pub fn get_cluster_leaves(
        &self,
        cluster_id: String,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<JsValue, JsValue> {
        to_json_value(&self.inner.get_cluster_leaves(
            &cluster_id,
            limit.unwrap_or(10),
            offset.unwrap_or(0),
        ))
    }

    #[wasm_bindgen(js_name = getHeatFeatures)]
    pub fn get_heat_features(&self, query: JsValue, options: JsValue) -> Result<JsValue, JsValue> {
        let query: geo_viz::GeoVizViewportQuery =
            serde_wasm_bindgen::from_value(query).map_err(into_js_error)?;
        let options = if options.is_undefined() || options.is_null() {
            geo_viz::GeoVizHeatOptions {
                radius_meters: None,
                weight_metric: None,
            }
        } else {
            serde_wasm_bindgen::from_value(options).map_err(into_js_error)?
        };
        to_json_value(
            &self
                .inner
                .get_heat_features(query, options)
                .map_err(into_js_error)?,
        )
    }

    #[wasm_bindgen(js_name = nearestPoint)]
    pub fn nearest_point(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let query: geo_viz::GeoVizNearestPointQuery =
            serde_wasm_bindgen::from_value(query).map_err(into_js_error)?;
        to_json_value(&self.inner.nearest_point(query).map_err(into_js_error)?)
    }
}

#[wasm_bindgen(js_class = GeoFlowIndex)]
impl WasmGeoFlowIndex {
    #[wasm_bindgen(constructor)]
    pub fn new(flows: JsValue) -> Result<WasmGeoFlowIndex, JsValue> {
        let flows: Vec<geo_viz::GeoVizFlow> =
            serde_wasm_bindgen::from_value(flows).map_err(into_js_error)?;
        let inner = geo_viz::GeoFlowIndex::new(flows).map_err(into_js_error)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen(js_name = getBounds)]
    pub fn get_bounds(&self) -> Result<JsValue, JsValue> {
        to_json_value(&self.inner.get_bounds())
    }

    #[wasm_bindgen(js_name = getViewportFlows)]
    pub fn get_viewport_flows(&self, query: JsValue, options: JsValue) -> Result<JsValue, JsValue> {
        let query: geo_viz::GeoVizViewportQuery =
            serde_wasm_bindgen::from_value(query).map_err(into_js_error)?;
        let options = if options.is_undefined() || options.is_null() {
            geo_viz::GeoVizFlowOptions::default()
        } else {
            serde_wasm_bindgen::from_value(options).map_err(into_js_error)?
        };
        to_json_value(
            &self
                .inner
                .get_viewport_flows(query, options)
                .map_err(into_js_error)?,
        )
    }
}

#[wasm_bindgen(js_class = GeoJsonIndex)]
impl WasmGeoJsonIndex {
    #[wasm_bindgen(constructor)]
    pub fn new(geo_json: JsValue) -> Result<WasmGeoJsonIndex, JsValue> {
        let geo_json: serde_json::Value =
            serde_wasm_bindgen::from_value(geo_json).map_err(into_js_error)?;
        let inner = geo_viz::GeoJsonIndex::new(geo_json).map_err(into_js_error)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen(js_name = getBounds)]
    pub fn get_bounds(&self) -> Result<JsValue, JsValue> {
        to_json_value(&self.inner.get_bounds())
    }

    #[wasm_bindgen(js_name = getViewportFeatures)]
    pub fn get_viewport_features(
        &self,
        query: JsValue,
        options: JsValue,
    ) -> Result<JsValue, JsValue> {
        let query: geo_viz::GeoVizViewportQuery =
            serde_wasm_bindgen::from_value(query).map_err(into_js_error)?;
        let options = if options.is_undefined() || options.is_null() {
            geo_viz::GeoVizGeoJsonOptions::default()
        } else {
            serde_wasm_bindgen::from_value(options).map_err(into_js_error)?
        };
        to_json_value(
            &self
                .inner
                .get_viewport_features(query, options)
                .map_err(into_js_error)?,
        )
    }
}

#[wasm_bindgen(js_class = ScalarFieldIndex)]
impl WasmScalarFieldIndex {
    #[wasm_bindgen(constructor)]
    pub fn new(points: JsValue, options: JsValue) -> Result<WasmScalarFieldIndex, JsValue> {
        let points: Vec<geo_viz::GeoVizPoint> =
            serde_wasm_bindgen::from_value(points).map_err(into_js_error)?;
        let options = if options.is_undefined() || options.is_null() {
            geo_viz::GeoVizScalarFieldOptions::default()
        } else {
            serde_wasm_bindgen::from_value(options).map_err(into_js_error)?
        };
        let inner = geo_viz::GeoVizScalarFieldIndex::new(points, options).map_err(into_js_error)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen(js_name = getBounds)]
    pub fn get_bounds(&self) -> Result<JsValue, JsValue> {
        to_json_value(&self.inner.get_bounds())
    }

    #[wasm_bindgen(js_name = getPointCount)]
    pub fn get_point_count(&self) -> usize {
        self.inner.point_count()
    }

    #[wasm_bindgen(js_name = getValueDomain)]
    pub fn get_value_domain(&self) -> Result<JsValue, JsValue> {
        to_json_value(&self.inner.value_domain())
    }

    #[wasm_bindgen(js_name = getValueAtCoordinate)]
    pub fn get_value_at_coordinate(&self, coordinate: JsValue) -> Result<JsValue, JsValue> {
        let coordinate: [f64; 2] =
            serde_wasm_bindgen::from_value(coordinate).map_err(into_js_error)?;
        to_json_value(
            &self
                .inner
                .get_value_at_coordinate(coordinate)
                .map_err(into_js_error)?,
        )
    }

    #[wasm_bindgen(js_name = createGrid)]
    pub fn create_grid(&self) -> Result<JsValue, JsValue> {
        to_json_value(&self.inner.create_grid())
    }
}

#[wasm_bindgen(js_name = createScalarFieldGrid)]
pub fn create_scalar_field_grid(points: JsValue, options: JsValue) -> Result<JsValue, JsValue> {
    let points: Vec<geo_viz::GeoVizPoint> =
        serde_wasm_bindgen::from_value(points).map_err(into_js_error)?;
    let options = if options.is_undefined() || options.is_null() {
        geo_viz::GeoVizScalarFieldOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options).map_err(into_js_error)?
    };
    to_json_value(&geo_viz::create_scalar_field_grid(points, options).map_err(into_js_error)?)
}

#[wasm_bindgen(js_name = packageSurface)]
pub fn package_surface() -> Result<JsValue, JsValue> {
    to_json_value(&geo_viz::surface::package_surface())
}

#[wasm_bindgen(js_name = runOperation)]
pub fn run_operation(request: JsValue) -> Result<JsValue, JsValue> {
    let request: SurfaceRequest = serde_wasm_bindgen::from_value(request).map_err(into_js_error)?;
    let response = geo_viz::surface::run_surface_operation(request).map_err(into_js_error)?;
    to_json_value(&response)
}

fn to_json_value(value: &impl Serialize) -> Result<JsValue, JsValue> {
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
        let surface = geo_viz::surface::package_surface();
        assert_eq!(surface.library, "moenarch-geo-viz");
        let operation_ids = surface
            .operations
            .iter()
            .map(|operation| operation.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            operation_ids,
            vec![
                "describe",
                "geoViz.bounds",
                "geoViz.aggregateViewport",
                "geoViz.heatViewport",
                "geoViz.geoJsonViewport",
                "geoViz.flowViewport",
                "geoViz.resampleGeometry",
                "geoViz.scalarFieldGrid"
            ]
        );
    }

    #[test]
    fn wrapped_surface_runs_operation() {
        let response = geo_viz::surface::run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geoViz.resampleGeometry"),
            input: serde_json::json!({
                "coordinates": [0.0, 0.0, 10.0, 0.0],
                "coordinateCount": 3,
                "closed": false
            }),
        })
        .expect("resample operation");

        assert_eq!(
            response.value["coordinates"],
            serde_json::json!([0.0, 0.0, 5.0, 0.0, 10.0, 0.0])
        );
    }

    #[test]
    fn wrapped_surface_runs_scalar_field_operation() {
        let response = geo_viz::surface::run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geoViz.scalarFieldGrid"),
            input: serde_json::json!({
                "points": [{"id": "a", "longitude": 13.0, "latitude": 52.0, "metrics": {"value": 3.0}}],
                "options": {"domainBounds": [12.0, 51.0, 14.0, 53.0], "fieldColumns": 1, "fieldRows": 1}
            }),
        })
        .expect("scalar field operation");

        assert_eq!(response.value["values"], serde_json::json!([3.0]));
    }
}
