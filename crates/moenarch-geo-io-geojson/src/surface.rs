//! Library-owned runtime surface for `geo-io-geojson`.

use geo_core::{Coordinate, GeoFeature, GeoFeatureCollection, Geometry};
use runtime_core::{
    OperationId, PackageSurface, RuntimeCapabilities, SurfaceOperation, SurfaceRequest,
    SurfaceResponse,
};
use serde::Deserialize;

use crate::{parse_geojson, to_geojson_geometry, GeoJsonDocument};

const EARTH_RADIUS_METERS: f64 = 6_371_000.0;

/// Returns the package surface exposed by every transport wrapper.
pub fn package_surface() -> PackageSurface {
    PackageSurface {
        library: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: RuntimeCapabilities::pure_rust(),
        operations: vec![
            operation(
                "describe",
                "Describe package",
                "GeoJSON import and export adapters for geo-core.",
                serde_json::json!({"includeOperations": true}),
            ),
            operation(
                "geoJson.bounds",
                "GeoJSON bounds",
                "Computes bounds and coordinate counts for a GeoJSON document.",
                serde_json::json!({"geoJson": {"type": "Point", "coordinates": [8.0, 49.0]}}),
            ),
            operation(
                "geoJson.distance",
                "Geo distance",
                "Computes haversine meters or planar coordinate-unit distance between lon/lat coordinates.",
                serde_json::json!({"from": [8.0, 49.0], "to": [9.0, 49.0], "mode": "haversine"}),
            ),
            operation(
                "geoJson.toGeoJson",
                "Geometry to GeoJSON",
                "Converts the geo-core Geometry JSON shape into a GeoJSON geometry object.",
                serde_json::json!({"geometry": {"type": "Point", "coordinates": [8.0, 49.0]}}),
            ),
        ],
    }
}

fn operation(
    id: &str,
    name: &str,
    description: &str,
    example_request: serde_json::Value,
) -> SurfaceOperation {
    SurfaceOperation {
        id: OperationId::new(id),
        name: name.to_string(),
        description: Some(description.to_string()),
        curation: runtime_core::SurfaceOperationCuration::from_operation_id(id),
        input_schema: serde_json::json!({"type": "object", "additionalProperties": true, "xOperationCategory": runtime_core::operation_category(id)}),
        output_schema: serde_json::json!({"type": "object", "xOperationCategory": runtime_core::operation_category(id)}),
        example_request,
        wasm_supported: true,
        server_supported: true,
    }
}

/// Runs one library-owned operation.
pub fn run_surface_operation(request: SurfaceRequest) -> Result<SurfaceResponse, String> {
    let operation = request.operation.clone();
    let value = match request.operation.as_str() {
        "describe" | "geoJson.describe" => describe_value(request.input),
        "geo.bounds" | "geoJson.bounds" => bounds_value(parse_input(request.input)?)?,
        "geo.distance" | "geoJson.distance" => distance_value(parse_input(request.input)?)?,
        "geo.toGeoJson" | "geoJson.toGeoJson" => to_geojson_value(parse_input(request.input)?)?,
        operation => {
            return Err(format!(
                "unsupported operation `{operation}` for {}",
                env!("CARGO_PKG_NAME")
            ));
        }
    };
    Ok(response(operation, value))
}

fn describe_value(input: serde_json::Value) -> serde_json::Value {
    let surface = package_surface();
    serde_json::json!({
        "library": surface.library,
        "version": surface.version,
        "operationCount": surface.operations.len(),
        "operations": surface
            .operations
            .iter()
            .map(|operation| operation.id.as_str())
            .collect::<Vec<_>>(),
        "input": input
    })
}

fn response(operation: OperationId, value: serde_json::Value) -> SurfaceResponse {
    SurfaceResponse {
        operation,
        value,
        diagnostics: Vec::new(),
        artifacts: Vec::new(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoundsRequest {
    geo_json: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DistanceRequest {
    from: [f64; 2],
    to: [f64; 2],
    #[serde(default = "default_distance_mode")]
    mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToGeoJsonRequest {
    geometry: Geometry,
}

#[derive(Debug, Default)]
struct BoundsSummary {
    min_lon: f64,
    min_lat: f64,
    max_lon: f64,
    max_lat: f64,
    coordinate_count: usize,
    geometry_count: usize,
    feature_count: usize,
}

impl BoundsSummary {
    fn push(&mut self, position: [f64; 2]) -> Result<(), String> {
        let coordinate = Coordinate::from_position(position).map_err(|error| error.to_string())?;
        if self.coordinate_count == 0 {
            self.min_lon = coordinate.lon;
            self.max_lon = coordinate.lon;
            self.min_lat = coordinate.lat;
            self.max_lat = coordinate.lat;
        } else {
            self.min_lon = self.min_lon.min(coordinate.lon);
            self.max_lon = self.max_lon.max(coordinate.lon);
            self.min_lat = self.min_lat.min(coordinate.lat);
            self.max_lat = self.max_lat.max(coordinate.lat);
        }
        self.coordinate_count += 1;
        Ok(())
    }
}

fn bounds_value(request: BoundsRequest) -> Result<serde_json::Value, String> {
    let document = geojson_document(request.geo_json)?;
    let mut summary = BoundsSummary::default();
    visit_document(&document, &mut summary)?;
    if summary.coordinate_count == 0 {
        return Err("GeoJSON document must contain at least one coordinate".to_string());
    }
    Ok(serde_json::json!({
        "bbox": [summary.min_lon, summary.min_lat, summary.max_lon, summary.max_lat],
        "coordinateCount": summary.coordinate_count,
        "geometryCount": summary.geometry_count,
        "featureCount": summary.feature_count
    }))
}

fn distance_value(request: DistanceRequest) -> Result<serde_json::Value, String> {
    let from = Coordinate::from_position(request.from).map_err(|error| error.to_string())?;
    let to = Coordinate::from_position(request.to).map_err(|error| error.to_string())?;
    match request.mode.as_str() {
        "haversine" => {
            from.validate_geographic()
                .map_err(|error| error.to_string())?;
            to.validate_geographic()
                .map_err(|error| error.to_string())?;
            Ok(serde_json::json!({
                "mode": "haversine",
                "from": request.from,
                "to": request.to,
                "distanceMeters": haversine_distance(from, to)
            }))
        }
        "planar" => Ok(serde_json::json!({
            "mode": "planar",
            "from": request.from,
            "to": request.to,
            "distanceUnits": (to.lon - from.lon).hypot(to.lat - from.lat)
        })),
        mode => Err(format!("unsupported geo distance mode `{mode}`")),
    }
}

fn to_geojson_value(request: ToGeoJsonRequest) -> Result<serde_json::Value, String> {
    request
        .geometry
        .validate()
        .map_err(|error| error.to_string())?;
    let mut summary = BoundsSummary::default();
    visit_geometry(&request.geometry, &mut summary)?;
    let bbox = (summary.coordinate_count > 0).then_some(serde_json::json!([
        summary.min_lon,
        summary.min_lat,
        summary.max_lon,
        summary.max_lat
    ]));
    Ok(serde_json::json!({
        "geoJson": serde_json::to_value(to_geojson_geometry(&request.geometry)).map_err(|error| error.to_string())?,
        "bbox": bbox
    }))
}

fn geojson_document(input: serde_json::Value) -> Result<GeoJsonDocument, String> {
    match input {
        serde_json::Value::String(text) => parse_geojson(&text).map_err(|error| error.to_string()),
        value if value.is_object() => {
            parse_geojson(&value.to_string()).map_err(|error| error.to_string())
        }
        _ => Err("geoJson must be an object or string".to_string()),
    }
}

fn visit_document(document: &GeoJsonDocument, summary: &mut BoundsSummary) -> Result<(), String> {
    match document {
        GeoJsonDocument::Geometry(geometry) => visit_geometry(geometry, summary),
        GeoJsonDocument::Feature(feature) => visit_feature(feature, summary),
        GeoJsonDocument::FeatureCollection(collection) => visit_collection(collection, summary),
    }
}

fn visit_feature(feature: &GeoFeature, summary: &mut BoundsSummary) -> Result<(), String> {
    summary.feature_count += 1;
    if let Some(geometry) = &feature.geometry {
        visit_geometry(geometry, summary)?;
    }
    Ok(())
}

fn visit_collection(
    collection: &GeoFeatureCollection,
    summary: &mut BoundsSummary,
) -> Result<(), String> {
    for feature in &collection.features {
        visit_feature(feature, summary)?;
    }
    Ok(())
}

fn visit_geometry(geometry: &Geometry, summary: &mut BoundsSummary) -> Result<(), String> {
    summary.geometry_count += 1;
    match geometry {
        Geometry::Point { coordinates } => summary.push(*coordinates)?,
        Geometry::MultiPoint { coordinates } | Geometry::LineString { coordinates } => {
            for position in coordinates {
                summary.push(*position)?;
            }
        }
        Geometry::MultiLineString { coordinates } | Geometry::Polygon { coordinates } => {
            for line in coordinates {
                for position in line {
                    summary.push(*position)?;
                }
            }
        }
        Geometry::MultiPolygon { coordinates } => {
            for polygon in coordinates {
                for ring in polygon {
                    for position in ring {
                        summary.push(*position)?;
                    }
                }
            }
        }
        Geometry::GeometryCollection { geometries } => {
            for geometry in geometries {
                visit_geometry(geometry, summary)?;
            }
        }
    }
    Ok(())
}

fn haversine_distance(from: Coordinate, to: Coordinate) -> f64 {
    let from_lat = from.lat.to_radians();
    let to_lat = to.lat.to_radians();
    let delta_lat = (to.lat - from.lat).to_radians();
    let delta_lon = (to.lon - from.lon).to_radians();
    let a = (delta_lat / 2.0).sin().powi(2)
        + from_lat.cos() * to_lat.cos() * (delta_lon / 2.0).sin().powi(2);
    EARTH_RADIUS_METERS * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

fn parse_input<T: for<'de> Deserialize<'de>>(input: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(input).map_err(|error| format!("invalid request: {error}"))
}

fn default_distance_mode() -> String {
    "haversine".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_counts_geojson_coordinates() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geoJson.bounds"),
            input: serde_json::json!({"geoJson": {"type": "LineString", "coordinates": [[8.0, 49.0], [9.0, 50.0]]}}),
        })
        .expect("bounds operation");

        assert_eq!(
            response.value["bbox"],
            serde_json::json!([8.0, 49.0, 9.0, 50.0])
        );
        assert_eq!(response.value["coordinateCount"], 2);
    }

    #[test]
    fn converts_geometry_to_geojson() {
        let response = run_surface_operation(SurfaceRequest {
            operation: OperationId::new("geoJson.toGeoJson"),
            input: serde_json::json!({"geometry": {"type": "Point", "coordinates": [8.0, 49.0]}}),
        })
        .expect("to geojson operation");

        assert_eq!(response.value["geoJson"]["type"], "Point");
        assert_eq!(
            response.value["bbox"],
            serde_json::json!([8.0, 49.0, 8.0, 49.0])
        );
    }
}
