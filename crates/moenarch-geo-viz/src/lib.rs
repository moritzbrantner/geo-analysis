#![doc = include_str!("../README.md")]

mod scalar_field;
pub mod surface;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use geo_clustering::{ClusterIndex, ClusterItem, ClusterOptions, ClusterPoint};
use geo_core::{
    simplify_geometry, BBox, Coordinate, GeoError, GeoFeature, GeoFeatureCollection,
    Geometry as GeoDataGeometry, Result,
};
use geo_io_geojson::{parse_geojson, to_geojson_feature_collection, GeoJsonDocument};
use rstar::{PointDistance, RTree, RTreeObject, AABB};
use serde::{Deserialize, Serialize};

/// Numeric metric bag attached to points and aggregated features.
pub type GeoVizMetricRecord = BTreeMap<String, f64>;

/// Geographic bounding box in `[west, south, east, north]` order.
pub type GeoVizBounds = [f64; 4];

/// GeoJSON-compatible feature collection value returned to renderer adapters.
pub type GeoVizFeatureCollectionValue = serde_json::Value;

pub use scalar_field::{
    create_scalar_field_grid, GeoVizScalarFieldGrid, GeoVizScalarFieldIndex,
    GeoVizScalarFieldOptions,
};

fn invalid_argument(message: impl Into<String>) -> GeoError {
    GeoError::invalid_argument(message)
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Input point for map visualization indexes.
pub struct GeoVizPoint {
    /// Caller-owned optional identifier.
    pub id: Option<String>,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Longitude in degrees.
    pub longitude: f64,
    /// Latitude in degrees.
    pub latitude: f64,
    /// Finite numeric metrics.
    #[serde(default)]
    pub metrics: GeoVizMetricRecord,
    /// Caller-owned JSON properties.
    #[serde(default)]
    pub properties: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Indexed, normalized map point.
pub struct GeoVizIndexedPoint {
    /// Stable point id.
    pub id: String,
    /// Source index from the input array.
    pub source_index: usize,
    /// Label, defaulting to an empty string.
    pub label: String,
    /// Longitude in degrees.
    pub longitude: f64,
    /// Latitude in degrees.
    pub latitude: f64,
    /// Finite numeric metrics.
    pub metrics: GeoVizMetricRecord,
    /// Caller-owned JSON properties.
    pub properties: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Query for a geographic viewport.
pub struct GeoVizViewportQuery {
    /// Viewport bounds in `[west, south, east, north]` order.
    pub bounds: GeoVizBounds,
    /// Map zoom level.
    pub zoom: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Point aggregation configuration.
pub struct GeoVizAggregationOptions {
    /// Cluster radius in pixels.
    pub radius: Option<f64>,
    /// Tile extent hint retained for renderer compatibility.
    pub extent: Option<f64>,
    /// Minimum clustering zoom.
    pub min_zoom: Option<u8>,
    /// Maximum clustering zoom.
    pub max_zoom: Option<u8>,
}

impl Default for GeoVizAggregationOptions {
    fn default() -> Self {
        Self {
            radius: Some(72.0),
            extent: Some(512.0),
            min_zoom: Some(0),
            max_zoom: Some(16),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
/// Aggregated viewport feature.
pub enum GeoVizAggregationFeature {
    /// Individual visible point.
    Point {
        /// `[longitude, latitude]`.
        coordinates: [f64; 2],
        /// Aggregated metrics.
        metrics: GeoVizMetricRecord,
        /// Original point.
        point: GeoVizIndexedPoint,
    },
    /// Visible cluster.
    Cluster {
        /// Stable cluster id for this query.
        cluster_id: String,
        /// `[longitude, latitude]`.
        coordinates: [f64; 2],
        /// Zoom where this cluster expands.
        expansion_zoom: usize,
        /// Aggregated metrics.
        metrics: GeoVizMetricRecord,
        /// Number of source points represented.
        point_count: usize,
        /// Compact count label.
        point_count_abbreviated: String,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Summary for visible aggregated features.
pub struct GeoVizAggregationSummary {
    /// Queried bounds.
    pub bounds: GeoVizBounds,
    /// Queried zoom.
    pub zoom: f64,
    /// Aggregated visible metrics.
    pub metrics: GeoVizMetricRecord,
    /// Source point count represented by visible features.
    pub visible_point_count: usize,
    /// Visible cluster count.
    pub visible_cluster_count: usize,
    /// Visible unclustered point count.
    pub visible_unclustered_count: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Aggregation result for one viewport.
pub struct GeoVizAggregation {
    /// Visible features.
    pub features: Vec<GeoVizAggregationFeature>,
    /// Visible summary.
    pub summary: GeoVizAggregationSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Query for the nearest indexed point.
pub struct GeoVizNearestPointQuery {
    /// Longitude in degrees.
    pub longitude: f64,
    /// Latitude in degrees.
    pub latitude: f64,
    /// Optional maximum distance in coordinate degrees.
    #[serde(default)]
    pub max_distance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Point heat feature options.
pub struct GeoVizHeatOptions {
    /// Optional radius hint for renderers.
    #[serde(default)]
    pub radius_meters: Option<f64>,
    /// Metric key used as heat weight. Defaults to `weight`, then `1`.
    #[serde(default)]
    pub weight_metric: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Weighted point feature for geographic heat layers.
pub struct GeoVizHeatFeature {
    /// `[longitude, latitude]`.
    pub coordinates: [f64; 2],
    /// Stable point id.
    pub id: String,
    /// Point label.
    pub label: String,
    /// Point metrics.
    pub metrics: GeoVizMetricRecord,
    /// Source point.
    pub point: GeoVizIndexedPoint,
    /// Number of represented points.
    pub point_count: usize,
    /// Unnormalized feature weight.
    pub raw_weight: f64,
    /// Weight normalized to `[0, 1]`.
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Summary for geographic heat features.
pub struct GeoVizHeatSummary {
    /// Queried bounds.
    pub bounds: GeoVizBounds,
    /// Queried zoom.
    pub zoom: f64,
    /// Aggregated visible metrics.
    pub metrics: GeoVizMetricRecord,
    /// Maximum raw feature weight.
    pub max_weight: f64,
    /// Visible weighted point count.
    pub visible_point_count: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Heat features for one viewport.
pub struct GeoVizHeatAggregation {
    /// Visible heat features.
    pub features: Vec<GeoVizHeatFeature>,
    /// Visible summary.
    pub summary: GeoVizHeatSummary,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Input flow between two geographic coordinates.
pub struct GeoVizFlow {
    /// Caller-owned optional identifier.
    pub id: Option<String>,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Origin `[longitude, latitude]`.
    pub from: [f64; 2],
    /// Destination `[longitude, latitude]`.
    pub to: [f64; 2],
    /// Finite numeric metrics.
    #[serde(default)]
    pub metrics: GeoVizMetricRecord,
    /// Caller-owned JSON properties.
    #[serde(default)]
    pub properties: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Indexed, normalized geographic flow.
pub struct GeoVizIndexedFlow {
    /// Stable flow id.
    pub id: String,
    /// Source index from the input array.
    pub source_index: usize,
    /// Label, defaulting to an empty string.
    pub label: String,
    /// Origin `[longitude, latitude]`.
    pub from: [f64; 2],
    /// Destination `[longitude, latitude]`.
    pub to: [f64; 2],
    /// Finite numeric metrics.
    pub metrics: GeoVizMetricRecord,
    /// Caller-owned JSON properties.
    pub properties: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Flow aggregation mode.
pub enum GeoVizFlowAggregateMode {
    /// Return individual flows.
    #[default]
    None,
    /// Combine identical origin/destination pairs.
    OriginDestination,
    /// Reserved for future grid aggregation; currently equivalent to origin/destination.
    Grid,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Geographic flow query options.
pub struct GeoVizFlowOptions {
    /// Aggregation mode.
    #[serde(default)]
    pub aggregate: GeoVizFlowAggregateMode,
    /// Minimum raw weight to include.
    #[serde(default)]
    pub min_weight: Option<f64>,
    /// Metric key used as flow weight. Defaults to `weight`, then `1`.
    #[serde(default)]
    pub weight_metric: Option<String>,
}

impl Default for GeoVizFlowOptions {
    fn default() -> Self {
        Self {
            aggregate: GeoVizFlowAggregateMode::None,
            min_weight: None,
            weight_metric: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Weighted geographic flow feature.
pub struct GeoVizFlowFeature {
    /// Source flow.
    pub flow: GeoVizIndexedFlow,
    /// Unnormalized feature weight.
    pub raw_weight: f64,
    /// Weight normalized to `[0, 1]`.
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Summary for geographic flow features.
pub struct GeoVizFlowSummary {
    /// Bounds for all returned flows.
    pub bounds: Option<GeoVizBounds>,
    /// Queried bounds.
    pub viewport_bounds: GeoVizBounds,
    /// Queried zoom.
    pub zoom: f64,
    /// Aggregated visible metrics.
    pub metrics: GeoVizMetricRecord,
    /// Maximum raw feature weight.
    pub max_weight: f64,
    /// Visible flow count.
    pub visible_flow_count: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Flow features for one viewport.
pub struct GeoVizFlowAggregation {
    /// Visible flow features.
    pub features: Vec<GeoVizFlowFeature>,
    /// Visible summary.
    pub summary: GeoVizFlowSummary,
}

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

/// Geographic point aggregation index.
#[derive(Debug, Clone)]
pub struct GeoPointIndex {
    points: Vec<GeoVizIndexedPoint>,
    point_lookup: HashMap<String, GeoVizIndexedPoint>,
    metric_keys: Vec<String>,
    spatial_index: RTree<SpatialPoint>,
    clusters: ClusterIndex<String>,
}

impl GeoPointIndex {
    /// Builds a new point index.
    pub fn new(
        points: impl IntoIterator<Item = GeoVizPoint>,
        options: GeoVizAggregationOptions,
    ) -> Result<Self> {
        let normalized = points
            .into_iter()
            .enumerate()
            .map(|(index, point)| normalize_point(point, index))
            .collect::<Result<Vec<_>>>()?;
        let metric_keys = collect_metric_keys(&normalized);
        let point_lookup = normalized
            .iter()
            .cloned()
            .map(|point| (point.id.clone(), point))
            .collect::<HashMap<_, _>>();
        let spatial_index = RTree::bulk_load(
            normalized
                .iter()
                .map(|point| SpatialPoint {
                    point: [point.longitude, point.latitude],
                    point_id: point.id.clone(),
                })
                .collect(),
        );
        let clusters = ClusterIndex::new(
            normalized.iter().map(|point| ClusterPoint {
                id: point.id.clone(),
                longitude: point.longitude,
                latitude: point.latitude,
                properties: point.id.clone(),
            }),
            ClusterOptions {
                min_zoom: options.min_zoom.unwrap_or(0),
                max_zoom: options.max_zoom.unwrap_or(16),
                base_cell_count: 1,
            },
        )?;

        Ok(Self {
            points: normalized,
            point_lookup,
            metric_keys,
            spatial_index,
            clusters,
        })
    }

    /// Returns bounds for all indexed points.
    pub fn get_bounds(&self) -> Option<GeoVizBounds> {
        bounds_for_points(&self.points)
    }

    /// Returns one point by id.
    pub fn get_point_by_id(&self, point_id: &str) -> Option<GeoVizIndexedPoint> {
        self.point_lookup.get(point_id).cloned()
    }

    /// Returns visible features for a viewport.
    pub fn get_viewport_aggregation(
        &self,
        query: GeoVizViewportQuery,
    ) -> Result<GeoVizAggregation> {
        validate_bounds(query.bounds)?;
        let zoom = query.zoom.round().clamp(0.0, u8::MAX as f64) as u8;
        let raw_features = self.clusters.get_clusters(query.bounds, zoom)?;
        let mut seen = BTreeSet::new();
        let features = raw_features
            .into_iter()
            .filter_map(|feature| self.to_aggregation_feature(feature).transpose())
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|feature| {
                let key = feature_key(feature);
                if seen.contains(&key) {
                    return false;
                }
                seen.insert(key);
                true
            })
            .collect::<Vec<_>>();

        Ok(GeoVizAggregation {
            summary: summarize_features(query, &features, &self.metric_keys),
            features,
        })
    }

    /// Returns weighted heat features for a viewport.
    pub fn get_heat_features(
        &self,
        query: GeoVizViewportQuery,
        options: GeoVizHeatOptions,
    ) -> Result<GeoVizHeatAggregation> {
        validate_bounds(query.bounds)?;
        let points = self
            .points
            .iter()
            .filter(|point| point_in_bounds(point.longitude, point.latitude, query.bounds))
            .cloned()
            .collect::<Vec<_>>();
        let weighted = points
            .into_iter()
            .filter_map(|point| {
                let raw_weight = geo_weight(&point.metrics, options.weight_metric.as_deref());
                (raw_weight > 0.0).then_some((point, raw_weight))
            })
            .collect::<Vec<_>>();
        let max_weight = weighted
            .iter()
            .map(|(_, weight)| *weight)
            .fold(1.0_f64, f64::max);
        let features = weighted
            .into_iter()
            .map(|(point, raw_weight)| GeoVizHeatFeature {
                coordinates: [point.longitude, point.latitude],
                id: point.id.clone(),
                label: point.label.clone(),
                metrics: point.metrics.clone(),
                point,
                point_count: 1,
                raw_weight,
                value: raw_weight / max_weight,
            })
            .collect::<Vec<_>>();

        Ok(GeoVizHeatAggregation {
            summary: GeoVizHeatSummary {
                bounds: query.bounds,
                zoom: query.zoom,
                metrics: sum_metrics(
                    features.iter().map(|feature| &feature.metrics),
                    &self.metric_keys,
                ),
                max_weight,
                visible_point_count: features.len(),
            },
            features,
        })
    }

    /// Returns the nearest point to a coordinate.
    pub fn nearest_point(
        &self,
        query: GeoVizNearestPointQuery,
    ) -> Result<Option<GeoVizIndexedPoint>> {
        Coordinate::new(query.longitude, query.latitude)?.validate_geographic()?;
        if let Some(max_distance) = query.max_distance {
            if !max_distance.is_finite() || max_distance < 0.0 {
                return Err(invalid_argument(
                    "maxDistance must be finite and non-negative",
                ));
            }
        }
        let nearest = self
            .spatial_index
            .nearest_neighbor([query.longitude, query.latitude]);
        let Some(nearest) = nearest else {
            return Ok(None);
        };
        if let Some(max_distance) = query.max_distance {
            if nearest.distance_2(&[query.longitude, query.latitude]) > max_distance * max_distance
            {
                return Ok(None);
            }
        }

        Ok(self.point_lookup.get(&nearest.point_id).cloned())
    }

    /// Returns the zoom where a cluster expands.
    pub fn get_cluster_expansion_zoom(&self, cluster_id: &str) -> usize {
        self.clusters.get_cluster_expansion_zoom(cluster_id)
    }

    /// Returns source leaves for a cluster.
    pub fn get_cluster_leaves(
        &self,
        cluster_id: &str,
        limit: usize,
        offset: usize,
    ) -> Vec<GeoVizIndexedPoint> {
        self.clusters
            .get_leaves(cluster_id, limit, offset)
            .into_iter()
            .map(|point| point.id)
            .filter_map(|point_id| self.point_lookup.get(&point_id).cloned())
            .collect()
    }

    fn to_aggregation_feature(
        &self,
        item: ClusterItem<String>,
    ) -> Result<Option<GeoVizAggregationFeature>> {
        match item {
            ClusterItem::Cluster(cluster) => {
                let cluster_id = cluster.id;
                let point_count = cluster.point_count;
                let point_count_abbreviated = abbreviate_count(point_count);
                let leaves = self.get_cluster_leaves(&cluster_id, point_count, 0);
                let metrics =
                    sum_metrics(leaves.iter().map(|point| &point.metrics), &self.metric_keys);

                Ok(Some(GeoVizAggregationFeature::Cluster {
                    expansion_zoom: self.get_cluster_expansion_zoom(&cluster_id),
                    cluster_id,
                    coordinates: [cluster.longitude, cluster.latitude],
                    metrics,
                    point_count,
                    point_count_abbreviated,
                }))
            }
            ClusterItem::Point(cluster_point) => {
                let Some(point) = self.point_lookup.get(&cluster_point.id).cloned() else {
                    return Ok(None);
                };
                Ok(Some(GeoVizAggregationFeature::Point {
                    coordinates: [point.longitude, point.latitude],
                    metrics: point.metrics.clone(),
                    point,
                }))
            }
        }
    }
}

#[derive(Debug, Clone)]
struct SpatialPoint {
    point: [f64; 2],
    point_id: String,
}

impl RTreeObject for SpatialPoint {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.point)
    }
}

impl PointDistance for SpatialPoint {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        let dx = self.point[0] - point[0];
        let dy = self.point[1] - point[1];
        dx * dx + dy * dy
    }
}

/// Geographic flow index.
#[derive(Debug, Clone)]
pub struct GeoFlowIndex {
    flows: Vec<GeoVizIndexedFlow>,
    metric_keys: Vec<String>,
}

impl GeoFlowIndex {
    /// Builds a new flow index.
    pub fn new(flows: impl IntoIterator<Item = GeoVizFlow>) -> Result<Self> {
        let flows = flows
            .into_iter()
            .enumerate()
            .map(|(index, flow)| normalize_flow(flow, index))
            .collect::<Result<Vec<_>>>()?;
        let metric_keys = collect_flow_metric_keys(&flows);

        Ok(Self { flows, metric_keys })
    }

    /// Returns bounds for all indexed flows.
    pub fn get_bounds(&self) -> Option<GeoVizBounds> {
        bounds_for_flows(&self.flows)
    }

    /// Returns visible weighted flow features for a viewport.
    pub fn get_viewport_flows(
        &self,
        query: GeoVizViewportQuery,
        options: GeoVizFlowOptions,
    ) -> Result<GeoVizFlowAggregation> {
        validate_bounds(query.bounds)?;
        let min_weight = options.min_weight.unwrap_or(0.0);
        if !min_weight.is_finite() || min_weight < 0.0 {
            return Err(invalid_argument(
                "minWeight must be finite and non-negative",
            ));
        }
        let mut weighted = self
            .flows
            .iter()
            .filter(|flow| flow_intersects_bounds(flow, query.bounds))
            .filter_map(|flow| {
                let raw_weight = geo_weight(&flow.metrics, options.weight_metric.as_deref());
                (raw_weight >= min_weight && raw_weight > 0.0).then_some((flow.clone(), raw_weight))
            })
            .collect::<Vec<_>>();

        if options.aggregate != GeoVizFlowAggregateMode::None {
            weighted = aggregate_flows(weighted, &self.metric_keys);
        }

        let max_weight = weighted
            .iter()
            .map(|(_, weight)| *weight)
            .fold(1.0_f64, f64::max);
        let features = weighted
            .into_iter()
            .map(|(flow, raw_weight)| GeoVizFlowFeature {
                flow,
                raw_weight,
                value: raw_weight / max_weight,
            })
            .collect::<Vec<_>>();

        Ok(GeoVizFlowAggregation {
            summary: GeoVizFlowSummary {
                bounds: bounds_for_flows(features.iter().map(|feature| &feature.flow)),
                viewport_bounds: query.bounds,
                zoom: query.zoom,
                metrics: sum_metrics(
                    features.iter().map(|feature| &feature.flow.metrics),
                    &self.metric_keys,
                ),
                max_weight,
                visible_flow_count: features.len(),
            },
            features,
        })
    }
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
        validate_bounds(query.bounds)?;
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

fn normalize_point(point: GeoVizPoint, source_index: usize) -> Result<GeoVizIndexedPoint> {
    Coordinate::new(point.longitude, point.latitude)?.validate_geographic()?;
    Ok(GeoVizIndexedPoint {
        id: point.id.unwrap_or_else(|| source_index.to_string()),
        source_index,
        label: point.label.unwrap_or_default(),
        longitude: point.longitude,
        latitude: point.latitude,
        metrics: point
            .metrics
            .into_iter()
            .filter(|(_, value)| value.is_finite())
            .collect(),
        properties: point.properties,
    })
}

fn normalize_flow(flow: GeoVizFlow, source_index: usize) -> Result<GeoVizIndexedFlow> {
    Coordinate::from_position(flow.from)?.validate_geographic()?;
    Coordinate::from_position(flow.to)?.validate_geographic()?;
    Ok(GeoVizIndexedFlow {
        id: flow.id.unwrap_or_else(|| source_index.to_string()),
        source_index,
        label: flow.label.unwrap_or_default(),
        from: flow.from,
        to: flow.to,
        metrics: flow
            .metrics
            .into_iter()
            .filter(|(_, value)| value.is_finite())
            .collect(),
        properties: flow.properties,
    })
}

fn collect_metric_keys(points: &[GeoVizIndexedPoint]) -> Vec<String> {
    let mut keys = BTreeSet::new();
    for point in points {
        for key in point.metrics.keys() {
            keys.insert(key.clone());
        }
    }
    keys.into_iter().collect()
}

fn collect_flow_metric_keys(flows: &[GeoVizIndexedFlow]) -> Vec<String> {
    let mut keys = BTreeSet::new();
    for flow in flows {
        for key in flow.metrics.keys() {
            keys.insert(key.clone());
        }
    }
    keys.into_iter().collect()
}

fn bounds_for_points(points: &[GeoVizIndexedPoint]) -> Option<GeoVizBounds> {
    let first = points.first()?;
    let mut west = first.longitude;
    let mut south = first.latitude;
    let mut east = first.longitude;
    let mut north = first.latitude;

    for point in points.iter().skip(1) {
        west = west.min(point.longitude);
        south = south.min(point.latitude);
        east = east.max(point.longitude);
        north = north.max(point.latitude);
    }

    Some([west, south, east, north])
}

fn bounds_for_flows<'a>(
    flows: impl IntoIterator<Item = &'a GeoVizIndexedFlow>,
) -> Option<GeoVizBounds> {
    let mut coordinates = flows
        .into_iter()
        .flat_map(|flow| [flow.from, flow.to])
        .collect::<Vec<_>>();
    let first = coordinates.pop()?;
    let mut west = first[0];
    let mut south = first[1];
    let mut east = first[0];
    let mut north = first[1];

    for coordinate in coordinates {
        west = west.min(coordinate[0]);
        south = south.min(coordinate[1]);
        east = east.max(coordinate[0]);
        north = north.max(coordinate[1]);
    }

    Some([west, south, east, north])
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
            if seen.contains(&key) {
                return false;
            }
            seen.insert(key);
            true
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

fn validate_bounds(bounds: GeoVizBounds) -> Result<()> {
    if bounds.iter().any(|value| !value.is_finite()) {
        return Err(invalid_argument("viewport bounds must be finite"));
    }
    if bounds[1] > bounds[3] {
        return Err(invalid_argument("viewport south must be <= north"));
    }
    if bounds[1] < -90.0 || bounds[3] > 90.0 {
        return Err(invalid_argument(
            "viewport latitude bounds must stay between -90 and 90",
        ));
    }
    Ok(())
}

fn point_in_bounds(longitude: f64, latitude: f64, bounds: GeoVizBounds) -> bool {
    let longitude_visible = if bounds[0] <= bounds[2] {
        longitude >= bounds[0] && longitude <= bounds[2]
    } else {
        longitude >= bounds[0] || longitude <= bounds[2]
    };

    longitude_visible && latitude >= bounds[1] && latitude <= bounds[3]
}

fn flow_intersects_bounds(flow: &GeoVizIndexedFlow, bounds: GeoVizBounds) -> bool {
    point_in_bounds(flow.from[0], flow.from[1], bounds)
        || point_in_bounds(flow.to[0], flow.to[1], bounds)
        || flow_bbox_intersects_bounds(flow, bounds)
}

fn flow_bbox_intersects_bounds(flow: &GeoVizIndexedFlow, bounds: GeoVizBounds) -> bool {
    let flow_west = flow.from[0].min(flow.to[0]);
    let flow_east = flow.from[0].max(flow.to[0]);
    let flow_south = flow.from[1].min(flow.to[1]);
    let flow_north = flow.from[1].max(flow.to[1]);
    let latitude_intersects = flow_south <= bounds[3] && flow_north >= bounds[1];

    if !latitude_intersects {
        return false;
    }

    if bounds[0] <= bounds[2] {
        flow_west <= bounds[2] && flow_east >= bounds[0]
    } else {
        flow_east >= bounds[0] || flow_west <= bounds[2]
    }
}

fn geo_weight(metrics: &GeoVizMetricRecord, weight_metric: Option<&str>) -> f64 {
    let weight = weight_metric
        .and_then(|key| metrics.get(key))
        .copied()
        .or_else(|| metrics.get("weight").copied())
        .unwrap_or(1.0);

    if weight.is_finite() {
        weight.max(0.0)
    } else {
        0.0
    }
}

fn aggregate_flows(
    weighted: Vec<(GeoVizIndexedFlow, f64)>,
    metric_keys: &[String],
) -> Vec<(GeoVizIndexedFlow, f64)> {
    let mut grouped = BTreeMap::<String, (GeoVizIndexedFlow, f64)>::new();

    for (flow, raw_weight) in weighted {
        let key = format!(
            "{:.6},{:.6}->{:.6},{:.6}",
            flow.from[0], flow.from[1], flow.to[0], flow.to[1]
        );
        let entry = grouped.entry(key).or_insert_with(|| {
            let mut flow = flow.clone();
            flow.id = format!(
                "{}:{}:{}:{}",
                flow.from[0], flow.from[1], flow.to[0], flow.to[1]
            );
            flow.label.clear();
            flow.metrics = metric_keys.iter().map(|key| (key.clone(), 0.0)).collect();
            (flow, 0.0)
        });
        entry.1 += raw_weight;
        for key in metric_keys {
            *entry.0.metrics.entry(key.clone()).or_insert(0.0) +=
                flow.metrics.get(key).copied().unwrap_or(0.0);
        }
    }

    grouped.into_values().collect()
}

fn default_clip_to_viewport() -> bool {
    true
}

fn sum_metrics<'a>(
    records: impl IntoIterator<Item = &'a GeoVizMetricRecord>,
    metric_keys: &[String],
) -> GeoVizMetricRecord {
    let mut metrics = metric_keys
        .iter()
        .map(|key| (key.clone(), 0.0))
        .collect::<GeoVizMetricRecord>();

    for record in records {
        for key in metric_keys {
            *metrics.entry(key.clone()).or_insert(0.0) += record.get(key).copied().unwrap_or(0.0);
        }
    }

    metrics
}

fn summarize_features(
    query: GeoVizViewportQuery,
    features: &[GeoVizAggregationFeature],
    metric_keys: &[String],
) -> GeoVizAggregationSummary {
    let mut metrics = metric_keys
        .iter()
        .map(|key| (key.clone(), 0.0))
        .collect::<GeoVizMetricRecord>();
    let mut visible_point_count = 0;
    let mut visible_cluster_count = 0;
    let mut visible_unclustered_count = 0;

    for feature in features {
        let (point_count, feature_metrics) = match feature {
            GeoVizAggregationFeature::Point { metrics, .. } => {
                visible_unclustered_count += 1;
                (1, metrics)
            }
            GeoVizAggregationFeature::Cluster {
                metrics,
                point_count,
                ..
            } => {
                visible_cluster_count += 1;
                (*point_count, metrics)
            }
        };
        visible_point_count += point_count;
        for key in metric_keys {
            *metrics.entry(key.clone()).or_insert(0.0) +=
                feature_metrics.get(key).copied().unwrap_or(0.0);
        }
    }

    GeoVizAggregationSummary {
        bounds: query.bounds,
        zoom: query.zoom,
        metrics,
        visible_point_count,
        visible_cluster_count,
        visible_unclustered_count,
    }
}

fn feature_key(feature: &GeoVizAggregationFeature) -> String {
    match feature {
        GeoVizAggregationFeature::Point { point, .. } => format!("point:{}", point.id),
        GeoVizAggregationFeature::Cluster { cluster_id, .. } => format!("cluster:{cluster_id}"),
    }
}

fn abbreviate_count(count: usize) -> String {
    if count >= 10_000 {
        format!("{}k", count / 1_000)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn point(id: &str, longitude: f64, latitude: f64, value: f64) -> GeoVizPoint {
        GeoVizPoint {
            id: Some(id.to_string()),
            label: Some(id.to_string()),
            longitude,
            latitude,
            metrics: BTreeMap::from([("value".to_string(), value)]),
            properties: json!({"id": id}),
        }
    }

    #[test]
    fn normalize_point_rejects_invalid_coordinates_and_preserves_finite_metrics() {
        let normalized = normalize_point(
            GeoVizPoint {
                id: Some("a".to_string()),
                label: Some("Alpha".to_string()),
                longitude: 13.0,
                latitude: 52.0,
                metrics: BTreeMap::from([
                    ("value".to_string(), 2.0),
                    ("bad".to_string(), f64::NAN),
                ]),
                properties: json!({"source": "test"}),
            },
            7,
        )
        .expect("normalized point");

        assert_eq!(normalized.id, "a");
        assert_eq!(normalized.source_index, 7);
        assert_eq!(
            normalized.metrics,
            BTreeMap::from([("value".to_string(), 2.0)])
        );
        assert_eq!(normalized.properties["source"], "test");
        assert!(normalize_point(point("bad", 181.0, 52.0, 1.0), 0).is_err());
    }

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
    fn flow_bbox_intersection_detects_crossing_and_non_crossing_flows() {
        let crossing = normalize_flow(
            GeoVizFlow {
                id: Some("crossing".to_string()),
                label: None,
                from: [10.0, 52.0],
                to: [16.0, 52.0],
                metrics: BTreeMap::new(),
                properties: json!({}),
            },
            0,
        )
        .unwrap();
        let outside = normalize_flow(
            GeoVizFlow {
                id: Some("outside".to_string()),
                label: None,
                from: [20.0, 60.0],
                to: [21.0, 61.0],
                metrics: BTreeMap::new(),
                properties: json!({}),
            },
            1,
        )
        .unwrap();
        let bounds = [12.0, 51.0, 14.0, 53.0];

        assert!(flow_bbox_intersects_bounds(&crossing, bounds));
        assert!(!flow_bbox_intersects_bounds(&outside, bounds));
    }

    #[test]
    fn feature_key_is_stable_for_points_and_clusters() {
        let point_feature = GeoVizAggregationFeature::Point {
            coordinates: [13.0, 52.0],
            metrics: BTreeMap::new(),
            point: normalize_point(point("a", 13.0, 52.0, 1.0), 0).unwrap(),
        };
        let cluster_feature = GeoVizAggregationFeature::Cluster {
            cluster_id: "z1:2:3".to_string(),
            coordinates: [13.0, 52.0],
            expansion_zoom: 2,
            metrics: BTreeMap::new(),
            point_count: 2,
            point_count_abbreviated: "2".to_string(),
        };

        assert_eq!(feature_key(&point_feature), "point:a");
        assert_eq!(feature_key(&point_feature), "point:a");
        assert_eq!(feature_key(&cluster_feature), "cluster:z1:2:3");
    }

    #[test]
    fn reports_bounds_and_lookup() {
        let index = GeoPointIndex::new(
            [point("a", 13.0, 52.0, 2.0), point("b", 14.0, 53.0, 3.0)],
            GeoVizAggregationOptions::default(),
        )
        .expect("index");

        assert_eq!(index.get_bounds(), Some([13.0, 52.0, 14.0, 53.0]));
        assert_eq!(index.get_point_by_id("a").unwrap().metrics["value"], 2.0);
    }

    #[test]
    fn rejects_invalid_coordinates() {
        let error = GeoPointIndex::new(
            [point("bad", 181.0, 52.0, 1.0)],
            GeoVizAggregationOptions::default(),
        )
        .expect_err("invalid longitude");
        assert!(error.to_string().contains("longitude"));
    }

    #[test]
    fn aggregates_cluster_metrics_and_leaves() {
        let index = GeoPointIndex::new(
            [
                point("a", 13.0, 52.0, 2.0),
                point("b", 13.0001, 52.0001, 3.0),
                point("c", 13.0002, 52.0002, 5.0),
            ],
            GeoVizAggregationOptions {
                radius: Some(80.0),
                ..GeoVizAggregationOptions::default()
            },
        )
        .expect("index");
        let aggregation = index
            .get_viewport_aggregation(GeoVizViewportQuery {
                bounds: [12.9, 51.9, 13.1, 52.1],
                zoom: 1.0,
            })
            .expect("aggregation");
        let cluster = aggregation
            .features
            .iter()
            .find_map(|feature| match feature {
                GeoVizAggregationFeature::Cluster {
                    cluster_id,
                    metrics,
                    point_count,
                    ..
                } => Some((cluster_id.clone(), metrics.clone(), *point_count)),
                _ => None,
            })
            .expect("cluster");

        assert_eq!(cluster.1["value"], 10.0);
        assert_eq!(cluster.2, 3);
        assert_eq!(index.get_cluster_leaves(&cluster.0, 2, 1).len(), 2);
        assert!(index.get_cluster_expansion_zoom(&cluster.0) >= 1);
    }

    #[test]
    fn supports_antimeridian_bounds() {
        let index = GeoPointIndex::new(
            [
                point("west", -179.8, 10.0, 2.0),
                point("east", 179.8, 10.0, 3.0),
            ],
            GeoVizAggregationOptions::default(),
        )
        .expect("index");
        let aggregation = index
            .get_viewport_aggregation(GeoVizViewportQuery {
                bounds: [179.0, 0.0, -179.0, 20.0],
                zoom: 8.0,
            })
            .expect("aggregation");

        assert_eq!(aggregation.summary.visible_point_count, 2);
    }

    #[test]
    fn returns_heat_features_and_nearest_points() {
        let index = GeoPointIndex::new(
            [point("a", 13.0, 52.0, 2.0), point("b", 14.0, 53.0, 4.0)],
            GeoVizAggregationOptions::default(),
        )
        .expect("index");
        let heat = index
            .get_heat_features(
                GeoVizViewportQuery {
                    bounds: [12.0, 51.0, 14.5, 53.5],
                    zoom: 7.0,
                },
                GeoVizHeatOptions {
                    radius_meters: Some(1000.0),
                    weight_metric: Some("value".to_string()),
                },
            )
            .expect("heat");

        assert_eq!(heat.features.len(), 2);
        assert_eq!(heat.summary.max_weight, 4.0);
        assert_eq!(
            index
                .nearest_point(GeoVizNearestPointQuery {
                    longitude: 13.1,
                    latitude: 52.1,
                    max_distance: Some(1.0),
                })
                .expect("nearest")
                .map(|point| point.id),
            Some("a".to_string())
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

    #[test]
    fn clustering_output_can_feed_geo_viz_aggregation() {
        let cluster_index = ClusterIndex::new(
            [
                ClusterPoint {
                    id: "a".to_string(),
                    longitude: 13.0,
                    latitude: 52.0,
                    properties: BTreeMap::from([("value".to_string(), 2.0)]),
                },
                ClusterPoint {
                    id: "b".to_string(),
                    longitude: 13.0001,
                    latitude: 52.0001,
                    properties: BTreeMap::from([("value".to_string(), 3.0)]),
                },
            ],
            ClusterOptions::default(),
        )
        .expect("cluster index");
        let points = cluster_index
            .get_clusters([12.0, 51.0, 14.0, 53.0], 16)
            .unwrap()
            .into_iter()
            .filter_map(|item| match item {
                ClusterItem::Point(point) => Some(GeoVizPoint {
                    id: Some(point.id),
                    label: None,
                    longitude: point.longitude,
                    latitude: point.latitude,
                    metrics: point.properties,
                    properties: json!({}),
                }),
                ClusterItem::Cluster(_) => None,
            });
        let viz =
            GeoPointIndex::new(points, GeoVizAggregationOptions::default()).expect("viz index");
        let aggregation = viz
            .get_viewport_aggregation(GeoVizViewportQuery {
                bounds: [12.0, 51.0, 14.0, 53.0],
                zoom: 16.0,
            })
            .expect("aggregation");

        assert_eq!(aggregation.summary.visible_point_count, 2);
        assert_eq!(aggregation.summary.metrics["value"], 5.0);
    }

    #[test]
    fn filters_and_aggregates_flows() {
        let index = GeoFlowIndex::new([
            GeoVizFlow {
                id: Some("a".to_string()),
                label: None,
                from: [13.0, 52.0],
                to: [14.0, 53.0],
                metrics: BTreeMap::from([("value".to_string(), 2.0)]),
                properties: json!({}),
            },
            GeoVizFlow {
                id: Some("b".to_string()),
                label: None,
                from: [13.0, 52.0],
                to: [14.0, 53.0],
                metrics: BTreeMap::from([("value".to_string(), 3.0)]),
                properties: json!({}),
            },
            GeoVizFlow {
                id: Some("outside".to_string()),
                label: None,
                from: [40.0, 40.0],
                to: [41.0, 41.0],
                metrics: BTreeMap::from([("value".to_string(), 10.0)]),
                properties: json!({}),
            },
        ])
        .expect("flow index");
        let aggregation = index
            .get_viewport_flows(
                GeoVizViewportQuery {
                    bounds: [12.0, 51.0, 15.0, 54.0],
                    zoom: 4.0,
                },
                GeoVizFlowOptions {
                    aggregate: GeoVizFlowAggregateMode::OriginDestination,
                    min_weight: Some(1.0),
                    weight_metric: Some("value".to_string()),
                },
            )
            .expect("flows");

        assert_eq!(aggregation.features.len(), 1);
        assert_eq!(aggregation.features[0].raw_weight, 5.0);
        assert_eq!(aggregation.summary.visible_flow_count, 1);
    }
}
