use std::collections::BTreeMap;

use geo_clustering::{ClusterIndex, ClusterItem, ClusterOptions, ClusterPoint};
use geo_viz::{
    GeoFlowIndex, GeoPointIndex, GeoVizAggregationOptions, GeoVizFlow, GeoVizFlowAggregateMode,
    GeoVizFlowOptions, GeoVizHeatOptions, GeoVizPoint, GeoVizViewportQuery,
};

fn point(id: Option<&str>, longitude: f64, latitude: f64, weight: f64) -> GeoVizPoint {
    GeoVizPoint {
        id: id.map(str::to_owned),
        label: None,
        longitude,
        latitude,
        metrics: BTreeMap::from([("weight".to_string(), weight)]),
        properties: serde_json::json!({}),
    }
}

fn flow(id: &str, from: [f64; 2], to: [f64; 2], weight: f64) -> GeoVizFlow {
    GeoVizFlow {
        id: Some(id.to_string()),
        label: None,
        from,
        to,
        metrics: BTreeMap::from([("weight".to_string(), weight)]),
        properties: serde_json::json!({}),
    }
}

fn viewport() -> GeoVizViewportQuery {
    GeoVizViewportQuery {
        bounds: [12.0, 51.0, 15.0, 54.0],
        zoom: 8.0,
    }
}

#[test]
fn cluster_index_rejects_duplicate_point_ids() {
    let error = ClusterIndex::new(
        [
            ClusterPoint {
                id: "duplicate".to_string(),
                longitude: 13.0,
                latitude: 52.0,
                properties: (),
            },
            ClusterPoint {
                id: "duplicate".to_string(),
                longitude: 14.0,
                latitude: 53.0,
                properties: (),
            },
        ],
        ClusterOptions::default(),
    )
    .expect_err("duplicate ids would make point identity ambiguous");

    assert!(error.to_string().contains("unique"));
}

#[test]
fn generated_point_ids_cannot_alias_explicit_ids() {
    let error = GeoPointIndex::new(
        [
            point(Some("1"), 13.0, 52.0, 1.0),
            point(None, 14.0, 53.0, 2.0),
        ],
        GeoVizAggregationOptions::default(),
    )
    .expect_err("source index id must not overwrite an explicit id");

    assert!(error.to_string().contains("unique"));
}

#[test]
fn heat_normalization_uses_the_actual_subunit_maximum() {
    let index = GeoPointIndex::new(
        [
            point(Some("a"), 13.0, 52.0, 0.25),
            point(Some("b"), 14.0, 53.0, 0.5),
        ],
        GeoVizAggregationOptions::default(),
    )
    .expect("point index");

    let heat = index
        .get_heat_features(
            viewport(),
            GeoVizHeatOptions {
                radius_meters: None,
                weight_metric: Some("weight".to_string()),
            },
        )
        .expect("heat features");

    assert_eq!(heat.summary.max_weight, 0.5);
    assert_eq!(heat.features.len(), 2);
    assert_eq!(heat.features[0].value, 0.5);
    assert_eq!(heat.features[1].value, 1.0);
}

#[test]
fn empty_heat_viewport_reports_zero_maximum() {
    let index = GeoPointIndex::new(
        [point(Some("outside"), 30.0, 60.0, 0.5)],
        GeoVizAggregationOptions::default(),
    )
    .expect("point index");

    let heat = index
        .get_heat_features(
            viewport(),
            GeoVizHeatOptions {
                radius_meters: None,
                weight_metric: Some("weight".to_string()),
            },
        )
        .expect("heat features");

    assert!(heat.features.is_empty());
    assert_eq!(heat.summary.max_weight, 0.0);
}

#[test]
fn flow_normalization_uses_the_actual_subunit_maximum() {
    let index = GeoFlowIndex::new([
        flow("a", [13.0, 52.0], [13.5, 52.5], 0.2),
        flow("b", [14.0, 53.0], [14.5, 53.5], 0.4),
    ])
    .expect("flow index");

    let aggregation = index
        .get_viewport_flows(
            viewport(),
            GeoVizFlowOptions {
                aggregate: GeoVizFlowAggregateMode::None,
                min_weight: None,
                weight_metric: Some("weight".to_string()),
            },
        )
        .expect("flow viewport");

    assert_eq!(aggregation.summary.max_weight, 0.4);
    assert_eq!(aggregation.features.len(), 2);
    assert_eq!(aggregation.features[0].value, 0.5);
    assert_eq!(aggregation.features[1].value, 1.0);
}

#[test]
fn origin_destination_aggregation_does_not_merge_nearby_distinct_flows() {
    let index = GeoFlowIndex::new([
        flow(
            "a",
            [13.000_000_1, 52.0],
            [14.0, 53.0],
            1.0,
        ),
        flow(
            "b",
            [13.000_000_2, 52.0],
            [14.0, 53.0],
            2.0,
        ),
    ])
    .expect("flow index");

    let aggregation = index
        .get_viewport_flows(
            viewport(),
            GeoVizFlowOptions {
                aggregate: GeoVizFlowAggregateMode::OriginDestination,
                min_weight: None,
                weight_metric: Some("weight".to_string()),
            },
        )
        .expect("flow viewport");

    assert_eq!(aggregation.features.len(), 2);
    assert_eq!(aggregation.summary.visible_flow_count, 2);
}

#[test]
fn geographic_viewports_reject_out_of_range_longitudes() {
    let cluster = ClusterIndex::new(
        [ClusterPoint {
            id: "a".to_string(),
            longitude: 13.0,
            latitude: 52.0,
            properties: (),
        }],
        ClusterOptions::default(),
    )
    .expect("cluster index");
    let cluster_error = cluster
        .get_clusters([181.0, 51.0, 182.0, 53.0], 8)
        .expect_err("invalid geographic viewport");
    assert!(cluster_error.to_string().contains("longitude"));

    let points = GeoPointIndex::new(
        [point(Some("a"), 13.0, 52.0, 1.0)],
        GeoVizAggregationOptions::default(),
    )
    .expect("point index");
    let viz_error = points
        .get_heat_features(
            GeoVizViewportQuery {
                bounds: [-181.0, 51.0, -170.0, 53.0],
                zoom: 8.0,
            },
            GeoVizHeatOptions {
                radius_meters: None,
                weight_metric: None,
            },
        )
        .expect_err("invalid geographic viewport");
    assert!(viz_error.to_string().contains("longitude"));
}


#[test]
fn viewport_queries_reject_non_finite_zoom() {
    let points = GeoPointIndex::new(
        [point(Some("a"), 13.0, 52.0, 1.0)],
        GeoVizAggregationOptions::default(),
    )
    .expect("point index");

    let error = points
        .get_heat_features(
            GeoVizViewportQuery {
                bounds: [12.0, 51.0, 15.0, 54.0],
                zoom: f64::NAN,
            },
            GeoVizHeatOptions {
                radius_meters: None,
                weight_metric: None,
            },
        )
        .expect_err("non-finite zoom must not leak into output");

    assert!(error.to_string().contains("zoom"));
}


#[test]
fn dateline_cluster_centroid_stays_near_the_dateline() {
    let index = ClusterIndex::new(
        [
            ClusterPoint {
                id: "west".to_string(),
                longitude: -179.8,
                latitude: 10.0,
                properties: (),
            },
            ClusterPoint {
                id: "east".to_string(),
                longitude: 179.8,
                latitude: 10.0,
                properties: (),
            },
        ],
        ClusterOptions::default(),
    )
    .expect("cluster index");

    let items = index
        .get_clusters([170.0, 0.0, -170.0, 20.0], 0)
        .expect("dateline viewport");
    let cluster = items
        .iter()
        .find_map(|item| match item {
            ClusterItem::Cluster(cluster) => Some(cluster),
            ClusterItem::Point(_) => None,
        })
        .expect("low zoom should cluster both dateline points");

    assert!(
        cluster.longitude.abs() > 170.0,
        "centroid drifted away from dateline: {}",
        cluster.longitude
    );
}
