use std::collections::{BTreeMap, BTreeSet};

use geo_core::{Coordinate, Result};
use serde::{Deserialize, Serialize};

use crate::metrics::{accumulate_metrics, finite_sum, geo_weight, maximum_weight, sum_metrics};
use crate::viewport::validate_viewport_query;
use crate::{invalid_argument, GeoVizBounds, GeoVizMetricRecord, GeoVizViewportQuery};

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
        validate_viewport_query(query)?;
        let min_weight = options.min_weight.unwrap_or(0.0);
        if !min_weight.is_finite() || min_weight < 0.0 {
            return Err(invalid_argument(
                "minWeight must be finite and non-negative",
            ));
        }
        let visible = self
            .flows
            .iter()
            .filter(|flow| flow_intersects_bounds(flow, query.bounds))
            .filter_map(|flow| {
                let raw_weight = geo_weight(&flow.metrics, options.weight_metric.as_deref());
                (raw_weight >= min_weight && raw_weight > 0.0).then_some((flow, raw_weight))
            })
            .collect::<Vec<_>>();

        let weighted = if options.aggregate == GeoVizFlowAggregateMode::None {
            visible
                .into_iter()
                .map(|(flow, raw_weight)| (flow.clone(), raw_weight))
                .collect()
        } else {
            aggregate_flows(visible, &self.metric_keys)?
        };

        let max_weight = maximum_weight(&weighted);
        let features = weighted
            .into_iter()
            .map(|(flow, raw_weight)| GeoVizFlowFeature {
                flow,
                raw_weight,
                value: raw_weight / max_weight,
            })
            .collect::<Vec<_>>();

        let metrics = sum_metrics(
            features.iter().map(|feature| &feature.flow.metrics),
            &self.metric_keys,
        )?;

        Ok(GeoVizFlowAggregation {
            summary: GeoVizFlowSummary {
                bounds: bounds_for_flows(features.iter().map(|feature| &feature.flow)),
                viewport_bounds: query.bounds,
                zoom: query.zoom,
                metrics,
                max_weight,
                visible_flow_count: features.len(),
            },
            features,
        })
    }
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

fn collect_flow_metric_keys(flows: &[GeoVizIndexedFlow]) -> Vec<String> {
    let mut keys = BTreeSet::new();
    for flow in flows {
        for key in flow.metrics.keys() {
            keys.insert(key.clone());
        }
    }
    keys.into_iter().collect()
}

fn bounds_for_flows<'a>(
    flows: impl IntoIterator<Item = &'a GeoVizIndexedFlow>,
) -> Option<GeoVizBounds> {
    let mut flows = flows.into_iter();
    let first = flows.next()?;
    let mut west = first.from[0].min(first.to[0]);
    let mut south = first.from[1].min(first.to[1]);
    let mut east = first.from[0].max(first.to[0]);
    let mut north = first.from[1].max(first.to[1]);

    for flow in flows {
        for coordinate in [flow.from, flow.to] {
            west = west.min(coordinate[0]);
            south = south.min(coordinate[1]);
            east = east.max(coordinate[0]);
            north = north.max(coordinate[1]);
        }
    }

    Some([west, south, east, north])
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

fn canonical_coordinate_bits(value: f64) -> u64 {
    if value == 0.0 {
        0.0_f64.to_bits()
    } else {
        value.to_bits()
    }
}

fn aggregate_flows(
    weighted: Vec<(&GeoVizIndexedFlow, f64)>,
    metric_keys: &[String],
) -> Result<Vec<(GeoVizIndexedFlow, f64)>> {
    let mut grouped = BTreeMap::<(u64, u64, u64, u64), (GeoVizIndexedFlow, f64)>::new();

    for (flow, raw_weight) in weighted {
        let key = (
            canonical_coordinate_bits(flow.from[0]),
            canonical_coordinate_bits(flow.from[1]),
            canonical_coordinate_bits(flow.to[0]),
            canonical_coordinate_bits(flow.to[1]),
        );
        let entry = grouped.entry(key).or_insert_with(|| {
            let aggregate = GeoVizIndexedFlow {
                id: format!(
                    "{}:{}:{}:{}",
                    flow.from[0], flow.from[1], flow.to[0], flow.to[1]
                ),
                source_index: flow.source_index,
                label: String::new(),
                from: flow.from,
                to: flow.to,
                metrics: metric_keys.iter().map(|key| (key.clone(), 0.0)).collect(),
                properties: flow.properties.clone(),
            };
            (aggregate, 0.0)
        });
        entry.1 = finite_sum(entry.1, raw_weight, "aggregated flow weight")?;
        accumulate_metrics(&mut entry.0.metrics, &flow.metrics, metric_keys)?;
    }

    Ok(grouped.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
