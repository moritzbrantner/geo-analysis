use std::collections::BTreeSet;

use geo_core::{
    simplify_geometry, BBox, GeoFeature, GeoFeatureCollection, Geometry as GeoDataGeometry, Result,
};
use geo_io_geojson::{parse_geojson, to_geojson_feature_collection, GeoJsonDocument};
use serde::{Deserialize, Serialize};

use crate::viewport::validate_viewport_query;
use crate::{invalid_argument, GeoVizBounds, GeoVizViewportQuery};

/// GeoJSON-compatible feature collection value returned to renderer adapters.
pub type GeoVizFeatureCollectionValue = serde_json::Value;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// GeoJSON viewport query options.
pub struct GeoVizGeoJsonOptions {
    /// Whether viewport filtering should be applied.
    #[serde(default = "default_clip_to_viewport")]
    pub clip_to_viewport: bool,
    /// Optional simplification tolerance in coordinate units.
    #[serde(default)]
    pub simplify_tolerance: Option<f64>,
}

impl Default for GeoVizGeoJsonOptions {
    fn default() -> Self {
        Self {
            clip_to_viewport: true,
            simplify_tolerance: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// GeoJSON features for one viewport.
pub struct GeoVizGeoJsonViewport {
    /// Bounds for returned features.
    pub bounds: Option<GeoVizBounds>,
    /// GeoJSON FeatureCollection value.
    pub feature_collection: GeoVizFeatureCollectionValue,
    /// Number of returned features.
    pub feature_count: usize,
    /// Queried bounds.
    pub viewport_bounds: GeoVizBounds,
    /// Queried zoom.
    pub zoom: f64,
}

/// GeoJSON viewport index.
#[derive(Debug, Clone)]
pub struct GeoJsonIndex {
    collection: GeoFeatureCollection,
}

impl GeoJsonIndex {
    /// Builds a new GeoJSON index from a GeoJSON object or string value.
    pub fn new(geo_json: serde_json::Value) -> Result<Self> {
        let text = match geo_json {
            serde_json::Value::String(text) => text,
            value if value.is_object() => value.to_string(),
            _ => return Err(invalid_argument("geoJson must be an object or string")),
        };
        let collection = match parse_geojson(&text)? {
            GeoJsonDocument::Geometry(geometry) => {
                GeoFeatureCollection::new(vec![GeoFeature::new(Some(geometry))])
            }
            GeoJsonDocument::Feature(feature) => GeoFeatureCollection::new(vec![feature]),
            GeoJsonDocument::FeatureCollection(collection) => collection,
        };

        Ok(Self { collection })
    }

    /// Returns bounds for all indexed features.
    pub fn get_bounds(&self) -> Option<GeoVizBounds> {
        bounds_for_collection(&self.collection)
    }

    /// Returns GeoJSON features for a viewport.
    pub fn get_viewport_features(
        &self,
        query: GeoVizViewportQuery,
        options: GeoVizGeoJsonOptions,
    ) -> Result<GeoVizGeoJsonViewport> {
        validate_viewport_query(query)?;
        let mut collection = if options.clip_to_viewport {
            filter_collection_for_bounds(&self.collection, query.bounds)?
        } else {
            self.collection.clone()
        };

        if let Some(tolerance) = options.simplify_tolerance {
            collection.features = collection
                .features
                .into_iter()
                .map(|mut feature| {
                    feature.geometry = feature
                        .geometry
                        .as_ref()
                        .map(|geometry| simplify_geometry(geometry, tolerance))
                        .transpose()?;
                    Ok(feature)
                })
                .collect::<Result<Vec<_>>>()?;
        }

        let feature_collection =
            serde_json::to_value(to_geojson_feature_collection(&collection.features))
                .map_err(|error| invalid_argument(error.to_string()))?;

        Ok(GeoVizGeoJsonViewport {
            bounds: bounds_for_collection(&collection),
            feature_collection,
            feature_count: collection.features.len(),
            viewport_bounds: query.bounds,
            zoom: query.zoom,
        })
    }
}

fn bounds_for_collection(collection: &GeoFeatureCollection) -> Option<GeoVizBounds> {
    let mut bounds: Option<GeoVizBounds> = None;
    for feature in &collection.features {
        if let Some(geometry) = &feature.geometry {
            visit_geometry_positions(geometry, &mut |position| {
                bounds = Some(match bounds {
                    Some([west, south, east, north]) => [
                        west.min(position[0]),
                        south.min(position[1]),
                        east.max(position[0]),
                        north.max(position[1]),
                    ],
                    None => [position[0], position[1], position[0], position[1]],
                });
            });
        }
    }
    bounds
}

fn filter_collection_for_bounds(
    collection: &GeoFeatureCollection,
    bounds: GeoVizBounds,
) -> Result<GeoFeatureCollection> {
    if bounds[0] <= bounds[2] {
        let bbox = BBox::new(bounds)?;
        return Ok(collection.filter_intersecting_bbox(bbox));
    }

    let west = BBox::new([bounds[0], bounds[1], 180.0, bounds[3]])?;
    let east = BBox::new([-180.0, bounds[1], bounds[2], bounds[3]])?;
    let mut seen = BTreeSet::new();
    let features = collection
        .features
        .iter()
        .filter(|feature| {
            let intersects = feature.geometry.as_ref().is_some_and(|geometry| {
                west.intersects_geometry(geometry) || east.intersects_geometry(geometry)
            });
            if !intersects {
                return false;
            }
            let key = feature
                .id
                .clone()
                .unwrap_or_else(|| serde_json::to_string(&feature.geometry).unwrap_or_default());
            seen.insert(key)
        })
        .cloned()
        .collect();

    Ok(GeoFeatureCollection {
        bbox: collection.bbox,
        features,
    })
}

fn visit_geometry_positions(geometry: &GeoDataGeometry, visit: &mut dyn FnMut([f64; 2])) {
    match geometry {
        GeoDataGeometry::Point { coordinates } => visit(*coordinates),
        GeoDataGeometry::MultiPoint { coordinates }
        | GeoDataGeometry::LineString { coordinates } => {
            for position in coordinates {
                visit(*position);
            }
        }
        GeoDataGeometry::MultiLineString { coordinates }
        | GeoDataGeometry::Polygon { coordinates } => {
            for line in coordinates {
                for position in line {
                    visit(*position);
                }
            }
        }
        GeoDataGeometry::MultiPolygon { coordinates } => {
            for polygon in coordinates {
                for ring in polygon {
                    for position in ring {
                        visit(*position);
                    }
                }
            }
        }
        GeoDataGeometry::GeometryCollection { geometries } => {
            for geometry in geometries {
                visit_geometry_positions(geometry, visit);
            }
        }
    }
}

fn default_clip_to_viewport() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bounds_for_collection_handles_geometry_types_and_empty_collections() {
        let collection = GeoFeatureCollection::new(vec![
            GeoFeature::new(Some(GeoDataGeometry::Point {
                coordinates: [13.0, 52.0],
            })),
            GeoFeature::new(Some(GeoDataGeometry::LineString {
                coordinates: vec![[12.0, 51.0], [14.0, 53.0]],
            })),
            GeoFeature::new(Some(GeoDataGeometry::Polygon {
                coordinates: vec![vec![
                    [11.0, 50.0],
                    [15.0, 50.0],
                    [15.0, 54.0],
                    [11.0, 54.0],
                    [11.0, 50.0],
                ]],
            })),
        ]);

        assert_eq!(
            bounds_for_collection(&collection),
            Some([11.0, 50.0, 15.0, 54.0])
        );
        assert_eq!(
            bounds_for_collection(&GeoFeatureCollection::new(Vec::new())),
            None
        );
    }

    #[test]
    fn filters_geojson_features_by_viewport() {
        let index = GeoJsonIndex::new(json!({
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "id": "inside",
                    "properties": {"name": "inside"},
                    "geometry": {"type": "Point", "coordinates": [13.0, 52.0]}
                },
                {
                    "type": "Feature",
                    "id": "outside",
                    "properties": {"name": "outside"},
                    "geometry": {"type": "Point", "coordinates": [20.0, 60.0]}
                },
                {
                    "type": "Feature",
                    "id": "crossing",
                    "properties": {"name": "crossing"},
                    "geometry": {"type": "LineString", "coordinates": [[11.0, 52.0], [15.0, 52.0]]}
                }
            ]
        }))
        .expect("geojson index");
        let viewport = index
            .get_viewport_features(
                GeoVizViewportQuery {
                    bounds: [12.0, 51.0, 14.0, 53.0],
                    zoom: 5.0,
                },
                GeoVizGeoJsonOptions::default(),
            )
            .expect("viewport");

        assert_eq!(index.get_bounds(), Some([11.0, 52.0, 20.0, 60.0]));
        assert_eq!(viewport.feature_count, 2);
        assert_eq!(viewport.feature_collection["features"][0]["id"], "inside");
    }

    #[test]
    fn parsed_geojson_collection_feeds_viewport_index() {
        let GeoJsonDocument::FeatureCollection(collection) = parse_geojson(
            r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":"inside","properties":{},"geometry":{"type":"Point","coordinates":[13,52]}},{"type":"Feature","id":"outside","properties":{},"geometry":{"type":"Point","coordinates":[20,60]}}]}"#,
        )
        .unwrap()
        else {
            panic!("expected feature collection");
        };
        let geojson = serde_json::to_value(to_geojson_feature_collection(&collection.features))
            .expect("geojson value");
        let index = GeoJsonIndex::new(geojson).expect("geojson index");
        let viewport = index
            .get_viewport_features(
                GeoVizViewportQuery {
                    bounds: [12.0, 51.0, 14.0, 53.0],
                    zoom: 5.0,
                },
                GeoVizGeoJsonOptions::default(),
            )
            .expect("viewport");

        assert_eq!(viewport.feature_count, 1);
        assert_eq!(viewport.feature_collection["features"][0]["id"], "inside");
    }
}
