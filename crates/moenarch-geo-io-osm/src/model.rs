use std::collections::BTreeMap;

use geo_core::{GeoFeature, GeoFeatureCollection, Geometry};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Normalized OSM tag map.
pub type OsmTags = BTreeMap<String, String>;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
/// OSM element kind.
pub enum OsmElementKind {
    /// OSM node.
    Node,
    /// OSM way.
    Way,
    /// OSM relation.
    Relation,
}

impl OsmElementKind {
    /// Returns the lower-case OSM type name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Way => "way",
            Self::Relation => "relation",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
/// OSM feature with geometry already converted to `geo-core`.
pub struct OsmFeature {
    /// OSM numeric id.
    pub id: i64,
    /// OSM element kind.
    #[serde(rename = "type")]
    pub kind: OsmElementKind,
    /// OSM tags.
    pub tags: OsmTags,
    /// Converted geometry.
    pub geometry: Geometry,
}

impl OsmFeature {
    /// Returns the stable feature id used in `geo-core` and GeoJSON.
    pub fn stable_id(&self) -> String {
        format!("{}/{}", self.kind.as_str(), self.id)
    }

    /// Converts this feature into a `geo-core` feature.
    pub fn into_geo_feature(self) -> GeoFeature {
        let stable_id = self.stable_id();
        let mut feature = GeoFeature::new(Some(self.geometry)).with_id(stable_id);
        feature.insert_property("osm_id", Value::from(self.id));
        feature.insert_property("osm_type", Value::from(self.kind.as_str()));
        for (key, value) in self.tags {
            feature.insert_property(key, Value::from(value));
        }
        feature
    }

    /// Converts this feature into a `geo-core` feature without consuming it.
    pub fn to_geo_feature(&self) -> GeoFeature {
        self.clone().into_geo_feature()
    }
}

pub(crate) fn geo_collection_from_osm(features: Vec<OsmFeature>) -> GeoFeatureCollection {
    GeoFeatureCollection::new(
        features
            .into_iter()
            .map(OsmFeature::into_geo_feature)
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_to_geo_feature_with_osm_metadata() {
        let feature = OsmFeature {
            id: 123,
            kind: OsmElementKind::Node,
            tags: OsmTags::from([("name".into(), "School".into())]),
            geometry: Geometry::Point {
                coordinates: [8.7, 48.9],
            },
        };

        let geo = feature.into_geo_feature();
        assert_eq!(geo.id.as_deref(), Some("node/123"));
        assert_eq!(geo.properties["osm_id"], 123);
        assert_eq!(geo.properties["osm_type"], "node");
        assert_eq!(geo.properties["name"], "School");
    }
}
