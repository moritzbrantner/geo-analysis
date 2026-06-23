#![doc = include_str!("../README.md")]

pub mod surface;

use geo_core::{
    BBox, Coordinate, GeoError, GeoFeature, GeoFeatureCollection, Geometry, Position, Properties,
    Result,
};
use serde_json::{Map, Value};

fn invalid_argument(message: impl Into<String>) -> GeoError {
    GeoError::invalid_argument(message)
}

#[derive(Debug, Clone, PartialEq)]
/// Parsed GeoJSON document converted into stable `geo-core` domain types.
pub enum GeoJsonDocument {
    /// Geometry document.
    Geometry(Geometry),
    /// Feature document.
    Feature(GeoFeature),
    /// FeatureCollection document.
    FeatureCollection(GeoFeatureCollection),
}

/// Converts internal geometry data to a `geojson` geometry.
pub fn to_geojson_geometry(geometry: &Geometry) -> geojson::Geometry {
    match geometry {
        Geometry::Point { coordinates } => {
            geojson::Geometry::new(geojson::GeometryValue::new_point(*coordinates))
        }
        Geometry::MultiPoint { coordinates } => geojson::Geometry::new(
            geojson::GeometryValue::new_multi_point(coordinates.iter().copied()),
        ),
        Geometry::LineString { coordinates } => geojson::Geometry::new(
            geojson::GeometryValue::new_line_string(coordinates.iter().copied()),
        ),
        Geometry::MultiLineString { coordinates } => {
            geojson::Geometry::new(geojson::GeometryValue::new_multi_line_string(
                coordinates.iter().map(|line| line.iter().copied()),
            ))
        }
        Geometry::Polygon { coordinates } => {
            geojson::Geometry::new(geojson::GeometryValue::new_polygon(
                coordinates.iter().map(|ring| ring.iter().copied()),
            ))
        }
        Geometry::MultiPolygon { coordinates } => {
            geojson::Geometry::new(geojson::GeometryValue::new_multi_polygon(
                coordinates
                    .iter()
                    .map(|polygon| polygon.iter().map(|ring| ring.iter().copied())),
            ))
        }
        Geometry::GeometryCollection { geometries } => {
            let geometries: Vec<geojson::Geometry> =
                geometries.iter().map(to_geojson_geometry).collect();
            geojson::Geometry::new(geojson::GeometryValue::new_geometry_collection(geometries))
        }
    }
}

/// Converts a `geojson` geometry to internal geometry data.
pub fn from_geojson_geometry(geometry: &geojson::Geometry) -> Result<Geometry> {
    let geometry = match &geometry.value {
        geojson::GeometryValue::Point { coordinates } => Geometry::Point {
            coordinates: array_position(coordinates)?,
        },
        geojson::GeometryValue::MultiPoint { coordinates } => Geometry::MultiPoint {
            coordinates: array_positions(coordinates)?,
        },
        geojson::GeometryValue::LineString { coordinates } => Geometry::LineString {
            coordinates: array_positions(coordinates)?,
        },
        geojson::GeometryValue::MultiLineString { coordinates } => Geometry::MultiLineString {
            coordinates: coordinates
                .iter()
                .map(|positions| array_positions(positions))
                .collect::<Result<Vec<_>>>()?,
        },
        geojson::GeometryValue::Polygon { coordinates } => Geometry::Polygon {
            coordinates: coordinates
                .iter()
                .map(|positions| array_positions(positions))
                .collect::<Result<Vec<_>>>()?,
        },
        geojson::GeometryValue::MultiPolygon { coordinates } => Geometry::MultiPolygon {
            coordinates: coordinates
                .iter()
                .map(|polygon| {
                    polygon
                        .iter()
                        .map(|ring| array_positions(ring))
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?,
        },
        geojson::GeometryValue::GeometryCollection { geometries } => Geometry::GeometryCollection {
            geometries: geometries
                .iter()
                .map(from_geojson_geometry)
                .collect::<Result<Vec<_>>>()?,
        },
    };
    geometry.validate()?;
    Ok(geometry)
}

/// Converts internal feature data to a `geojson` feature.
pub fn to_geojson_feature(feature: &GeoFeature) -> geojson::Feature {
    geojson::Feature {
        bbox: feature.bbox.map(|bbox| bbox.as_array().to_vec()),
        geometry: feature.geometry.as_ref().map(to_geojson_geometry),
        id: feature
            .id
            .as_ref()
            .map(|id| geojson::feature::Id::String(id.clone())),
        properties: Some(map_properties(&feature.properties)),
        foreign_members: None,
    }
}

/// Converts a `geojson` feature to internal feature data.
pub fn from_geojson_feature(feature: &geojson::Feature) -> Result<GeoFeature> {
    let bbox = match feature.bbox.as_deref() {
        Some(values) => Some(bbox_from_values(values)?),
        None => None,
    };
    let id = feature.id.as_ref().map(|id| match id {
        geojson::feature::Id::String(value) => value.clone(),
        geojson::feature::Id::Number(value) => value.to_string(),
    });
    let geometry = feature
        .geometry
        .as_ref()
        .map(from_geojson_geometry)
        .transpose()?;
    let properties = feature
        .properties
        .as_ref()
        .map(btree_properties)
        .unwrap_or_default();
    let feature = GeoFeature {
        id,
        bbox,
        geometry,
        properties,
    };
    feature.validate()?;
    Ok(feature)
}

/// Converts internal feature collection data to a `geojson` feature collection.
pub fn to_geojson_feature_collection(features: &[GeoFeature]) -> geojson::FeatureCollection {
    geojson::FeatureCollection {
        bbox: None,
        features: features.iter().map(to_geojson_feature).collect(),
        foreign_members: None,
    }
}

/// Converts a `geojson` feature collection to internal feature collection data.
pub fn from_geojson_feature_collection(
    collection: &geojson::FeatureCollection,
) -> Result<GeoFeatureCollection> {
    let bbox = match collection.bbox.as_deref() {
        Some(values) => Some(bbox_from_values(values)?),
        None => None,
    };
    let collection = GeoFeatureCollection {
        bbox,
        features: collection
            .features
            .iter()
            .map(from_geojson_feature)
            .collect::<Result<Vec<_>>>()?,
    };
    collection.validate()?;
    Ok(collection)
}

/// Parses GeoJSON text into internal geometry, feature, or feature collection data.
pub fn parse_geojson(input: &str) -> Result<GeoJsonDocument> {
    let geojson = input
        .parse::<geojson::GeoJson>()
        .map_err(|err| invalid_argument(format!("invalid GeoJSON: {err}")))?;

    match geojson {
        geojson::GeoJson::Geometry(geometry) => {
            Ok(GeoJsonDocument::Geometry(from_geojson_geometry(&geometry)?))
        }
        geojson::GeoJson::Feature(feature) => {
            Ok(GeoJsonDocument::Feature(from_geojson_feature(&feature)?))
        }
        geojson::GeoJson::FeatureCollection(collection) => Ok(GeoJsonDocument::FeatureCollection(
            from_geojson_feature_collection(&collection)?,
        )),
    }
}

fn array_position(position: &geojson::Position) -> Result<Position> {
    let values = position.as_slice();
    if values.len() < 2 {
        return Err(invalid_argument(
            "GeoJSON position must contain at least two numbers",
        ));
    }
    Coordinate::new(values[0], values[1]).map(Coordinate::as_position)
}

fn array_positions(positions: &[geojson::Position]) -> Result<Vec<Position>> {
    positions.iter().map(array_position).collect()
}

fn bbox_from_values(values: &[f64]) -> Result<BBox> {
    if values.len() != 4 {
        return Err(invalid_argument("bbox must contain four numbers"));
    }
    BBox::new([values[0], values[1], values[2], values[3]])
}

fn map_properties(properties: &Properties) -> Map<String, Value> {
    properties
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn btree_properties(properties: &Map<String, Value>) -> Properties {
    properties
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo_core::{point, Coordinate};

    fn coord(lon: f64, lat: f64) -> Coordinate {
        Coordinate::new(lon, lat).unwrap()
    }

    #[test]
    fn converts_geojson_feature_collection() {
        let mut feature = GeoFeature::new(Some(point(coord(8.7, 48.9)))).with_id("node/123");
        feature.insert_property("name", "Test");
        let collection = to_geojson_feature_collection(&[feature]);
        let parsed = from_geojson_feature_collection(&collection).unwrap();

        assert_eq!(parsed.features.len(), 1);
        assert_eq!(parsed.features[0].id.as_deref(), Some("node/123"));
        assert_eq!(parsed.features[0].properties["name"], Value::from("Test"));
    }

    #[test]
    fn rejects_invalid_bbox_values() {
        assert!(bbox_from_values(&[0.0, 0.0, 1.0]).is_err());
        assert!(bbox_from_values(&[2.0, 0.0, 1.0, 1.0]).is_err());
        assert!(bbox_from_values(&[0.0, f64::NAN, 1.0, 1.0]).is_err());
    }

    #[test]
    fn rejects_invalid_geojson_positions() {
        assert!(array_position(&geojson::Position::from(vec![8.0])).is_err());
        assert!(array_position(&geojson::Position::from([f64::INFINITY, 48.0])).is_err());
        assert!(array_position(&geojson::Position::from([8.0, f64::NAN])).is_err());
    }

    #[test]
    fn property_maps_round_trip_json_values() {
        let properties = Properties::from([
            ("name".to_string(), Value::from("Park")),
            ("rank".to_string(), Value::from(3)),
            ("nested".to_string(), serde_json::json!({"open": true})),
        ]);

        let mapped = map_properties(&properties);
        let round_tripped = btree_properties(&mapped);

        assert_eq!(round_tripped, properties);
    }

    #[test]
    fn geo_core_geojson_round_trip_preserves_feature_contract() {
        let mut feature = GeoFeature::new(Some(Geometry::LineString {
            coordinates: vec![[8.0, 48.0], [9.0, 49.0]],
        }))
        .with_id("line/1")
        .with_bbox(BBox::new([8.0, 48.0, 9.0, 49.0]).unwrap())
        .unwrap();
        feature.insert_property("name", "Trail");
        feature.insert_property("score", 7);

        let geojson = to_geojson_feature(&feature);
        let parsed = from_geojson_feature(&geojson).unwrap();

        assert_eq!(parsed.id.as_deref(), Some("line/1"));
        assert_eq!(parsed.bbox.unwrap().as_array(), [8.0, 48.0, 9.0, 49.0]);
        assert_eq!(parsed.properties["name"], Value::from("Trail"));
        assert_eq!(parsed.properties["score"], Value::from(7));
        assert_eq!(parsed.geometry, feature.geometry);
    }

    #[test]
    fn parses_geojson_feature_collection() {
        let document = parse_geojson(
            r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"Park"},"geometry":{"type":"Point","coordinates":[8.7,48.9]}}]}"#,
        )
        .unwrap();

        let GeoJsonDocument::FeatureCollection(collection) = document else {
            panic!("expected collection");
        };
        assert_eq!(
            collection.features[0].properties["name"],
            Value::from("Park")
        );
    }

    #[test]
    fn rejects_invalid_geojson_line() {
        let err = parse_geojson(r#"{"type":"LineString","coordinates":[[0.0,0.0]]}"#).unwrap_err();

        assert!(err.to_string().contains("at least two positions"));
    }
}
