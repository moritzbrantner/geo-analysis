#![doc = include_str!("../README.md")]

pub mod surface;

use std::collections::{BTreeMap, BTreeSet};

use geo_core::{BBox, Coordinate, GeoError, Result};
use rstar::{RTree, RTreeObject, AABB};
use serde::{Deserialize, Serialize};

fn invalid_argument(message: impl Into<String>) -> GeoError {
    GeoError::invalid_argument(message)
}

/// Point accepted by clustering indexes.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ClusterPoint<Properties = ()> {
    /// Stable caller-owned point id.
    pub id: String,
    /// Longitude in degrees.
    pub longitude: f64,
    /// Latitude in degrees.
    pub latitude: f64,
    /// Caller-owned properties kept opaque by the clustering algorithm.
    pub properties: Properties,
}

/// Cluster returned by viewport queries.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Cluster {
    /// Stable cluster id for the current query.
    pub id: String,
    /// Cluster centroid longitude in degrees.
    pub longitude: f64,
    /// Cluster centroid latitude in degrees.
    pub latitude: f64,
    /// Number of source points represented by the cluster.
    pub point_count: usize,
}

/// Point or aggregate cluster returned by viewport queries.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum ClusterItem<Properties = ()> {
    /// Individual source point.
    Point(ClusterPoint<Properties>),
    /// Aggregate cluster.
    Cluster(Cluster),
}

/// Bounding box in `[west, south, east, north]` order.
pub type ClusterBounds = [f64; 4];

/// Minimal grid clustering options.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterOptions {
    /// Minimum zoom that may cluster.
    pub min_zoom: u8,
    /// Zooms at or above this value return individual points.
    pub max_zoom: u8,
    /// Approximate grid divisions at zoom zero. Higher values produce smaller clusters.
    pub base_cell_count: u32,
}

impl Default for ClusterOptions {
    fn default() -> Self {
        Self {
            min_zoom: 0,
            max_zoom: 16,
            base_cell_count: 1,
        }
    }
}

/// Format-agnostic point clustering index.
#[derive(Debug, Clone)]
pub struct ClusterIndex<Properties = ()> {
    points: Vec<ClusterPoint<Properties>>,
    spatial_index: RTree<SpatialPoint>,
    bounds: Option<ClusterBounds>,
    options: ClusterOptions,
}

impl<Properties: Clone> ClusterIndex<Properties> {
    /// Builds an index from finite longitude/latitude points.
    pub fn new(
        points: impl IntoIterator<Item = ClusterPoint<Properties>>,
        options: ClusterOptions,
    ) -> Result<Self> {
        if options.min_zoom > options.max_zoom {
            return Err(invalid_argument("min_zoom must be <= max_zoom"));
        }
        if options.base_cell_count == 0 {
            return Err(invalid_argument(
                "base_cell_count must be greater than zero",
            ));
        }
        let points = points
            .into_iter()
            .map(validate_point)
            .collect::<Result<Vec<_>>>()?;
        let mut point_ids = BTreeSet::new();
        for point in &points {
            if !point_ids.insert(point.id.as_str()) {
                return Err(invalid_argument(format!(
                    "point id must be unique: {}",
                    point.id
                )));
            }
        }
        let bounds = bounds_for_points(&points);
        let spatial_index = RTree::bulk_load(
            points
                .iter()
                .enumerate()
                .map(|(point_index, point)| SpatialPoint {
                    point: [point.longitude, point.latitude],
                    point_index,
                })
                .collect(),
        );

        Ok(Self {
            points,
            spatial_index,
            bounds,
            options,
        })
    }

    /// Returns clusters or points visible in a bounding box at a zoom level.
    pub fn get_clusters(
        &self,
        bounds: ClusterBounds,
        zoom: u8,
    ) -> Result<Vec<ClusterItem<Properties>>> {
        validate_bounds(bounds)?;
        let mut visible_indexes = Vec::new();
        let mut collect_envelope = |west: f64, east: f64| {
            visible_indexes.extend(
                self.spatial_index
                    .locate_in_envelope(AABB::from_corners([west, bounds[1]], [east, bounds[3]]))
                    .map(|point| point.point_index),
            );
        };
        if bounds[0] <= bounds[2] {
            collect_envelope(bounds[0], bounds[2]);
        } else {
            collect_envelope(bounds[0], 180.0);
            collect_envelope(-180.0, bounds[2]);
        }
        visible_indexes.sort_unstable();
        let visible = visible_indexes
            .into_iter()
            .map(|point_index| self.points[point_index].clone())
            .collect::<Vec<_>>();

        if zoom < self.options.min_zoom || zoom >= self.options.max_zoom {
            return Ok(visible.into_iter().map(ClusterItem::Point).collect());
        }

        let mut grouped: BTreeMap<(i64, i64), Vec<ClusterPoint<Properties>>> = BTreeMap::new();
        let cell_size = cell_size_degrees(zoom, self.options);
        for point in visible {
            let key = (
                ((point.longitude + 180.0) / cell_size).floor() as i64,
                ((point.latitude + 90.0) / cell_size).floor() as i64,
            );
            grouped.entry(key).or_default().push(point);
        }

        Ok(grouped
            .into_iter()
            .flat_map(|((x, y), points)| {
                if points.len() == 1 {
                    vec![ClusterItem::Point(points.into_iter().next().unwrap())]
                } else {
                    vec![ClusterItem::Cluster(cluster_for_points(
                        zoom, x, y, &points,
                    ))]
                }
            })
            .collect())
    }

    /// Returns bounds for all indexed points.
    pub fn get_bounds(&self) -> Option<ClusterBounds> {
        self.bounds
    }

    /// Returns source points represented by a cluster id from this index.
    pub fn get_leaves(
        &self,
        cluster_id: &str,
        limit: usize,
        offset: usize,
    ) -> Vec<ClusterPoint<Properties>> {
        let Some((x, y)) = parse_cluster_id(cluster_id) else {
            return Vec::new();
        };
        self.points
            .iter()
            .filter(|point| cluster_id_for_point(point, x.zoom, self.options) == (x.cell, y.cell))
            .skip(offset)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Returns a conservative expansion zoom for the cluster.
    pub fn get_cluster_expansion_zoom(&self, cluster_id: &str) -> usize {
        parse_cluster_id(cluster_id)
            .map(|(x, _)| usize::from((x.zoom + 1).min(self.options.max_zoom)))
            .unwrap_or_else(|| usize::from(self.options.max_zoom))
    }
}

#[derive(Debug, Clone)]
struct SpatialPoint {
    point: [f64; 2],
    point_index: usize,
}

impl RTreeObject for SpatialPoint {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.point)
    }
}

#[derive(Debug, Clone, Copy)]
struct ClusterCell {
    zoom: u8,
    cell: i64,
}

fn validate_point<Properties>(point: ClusterPoint<Properties>) -> Result<ClusterPoint<Properties>> {
    if point.id.is_empty() {
        return Err(invalid_argument("point id must not be empty"));
    }
    Coordinate::new(point.longitude, point.latitude)?.validate_geographic()?;
    Ok(point)
}

fn validate_bounds(bounds: ClusterBounds) -> Result<()> {
    if bounds.iter().any(|value| !value.is_finite()) {
        return Err(invalid_argument("bounds must be finite"));
    }
    if bounds[1] > bounds[3] {
        return Err(invalid_argument("bounds south must be <= north"));
    }
    if bounds[0] < -180.0 || bounds[0] > 180.0 || bounds[2] < -180.0 || bounds[2] > 180.0 {
        return Err(invalid_argument(
            "bounds longitude values must be between -180 and 180",
        ));
    }
    BBox::new([
        bounds[0].min(bounds[2]),
        bounds[1],
        bounds[0].max(bounds[2]),
        bounds[3],
    ])?;
    if bounds[1] < -90.0 || bounds[3] > 90.0 {
        return Err(invalid_argument(
            "bounds latitude values must be between -90 and 90",
        ));
    }
    Ok(())
}

fn bounds_for_points<Properties>(points: &[ClusterPoint<Properties>]) -> Option<ClusterBounds> {
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

fn cell_size_degrees(zoom: u8, options: ClusterOptions) -> f64 {
    let divisions = options
        .base_cell_count
        .saturating_mul(2_u32.saturating_pow(u32::from(zoom)));
    360.0 / f64::from(divisions.max(1))
}

fn cluster_for_points<Properties>(
    zoom: u8,
    x: i64,
    y: i64,
    points: &[ClusterPoint<Properties>],
) -> Cluster {
    let point_count = points.len();
    let (sin_sum, cos_sum, lat_sum) =
        points
            .iter()
            .fold((0.0, 0.0, 0.0), |(sin_sum, cos_sum, lat_sum), point| {
                let longitude = point.longitude.to_radians();
                (
                    sin_sum + longitude.sin(),
                    cos_sum + longitude.cos(),
                    lat_sum + point.latitude,
                )
            });
    let longitude = if sin_sum.abs() <= f64::EPSILON && cos_sum.abs() <= f64::EPSILON {
        points.iter().map(|point| point.longitude).sum::<f64>() / point_count as f64
    } else {
        sin_sum.atan2(cos_sum).to_degrees()
    };
    Cluster {
        id: format!("z{zoom}:{x}:{y}"),
        longitude,
        latitude: lat_sum / point_count as f64,
        point_count,
    }
}

fn cluster_id_for_point<Properties>(
    point: &ClusterPoint<Properties>,
    zoom: u8,
    options: ClusterOptions,
) -> (i64, i64) {
    let cell_size = cell_size_degrees(zoom, options);
    (
        ((point.longitude + 180.0) / cell_size).floor() as i64,
        ((point.latitude + 90.0) / cell_size).floor() as i64,
    )
}

fn parse_cluster_id(id: &str) -> Option<(ClusterCell, ClusterCell)> {
    let mut parts = id.split(':');
    let zoom = parts.next()?.strip_prefix('z')?.parse::<u8>().ok()?;
    let x = parts.next()?.parse::<i64>().ok()?;
    let y = parts.next()?.parse::<i64>().ok()?;
    Some((ClusterCell { zoom, cell: x }, ClusterCell { zoom, cell: y }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(id: &str, longitude: f64, latitude: f64) -> ClusterPoint {
        ClusterPoint {
            id: id.to_string(),
            longitude,
            latitude,
            properties: (),
        }
    }

    #[test]
    fn validates_points_and_bounds() {
        assert!(validate_point(point("bad-lon", 181.0, 0.0)).is_err());
        assert!(validate_point(point("bad-lat", 0.0, 91.0)).is_err());
        assert!(validate_point(point("", 0.0, 0.0)).is_err());

        assert!(validate_bounds([0.0, 1.0, 1.0, 0.0]).is_err());
        assert!(validate_bounds([0.0, -91.0, 1.0, 0.0]).is_err());
        assert!(validate_bounds([170.0, -10.0, -170.0, 10.0]).is_ok());
    }

    #[test]
    fn cell_size_decreases_as_zoom_increases() {
        let options = ClusterOptions {
            base_cell_count: 2,
            ..ClusterOptions::default()
        };

        assert!(cell_size_degrees(4, options) < cell_size_degrees(3, options));
    }

    #[test]
    fn parses_generated_cluster_ids_and_rejects_malformed_ids() {
        let cluster = cluster_for_points(3, 42, 17, &[point("a", 0.0, 0.0), point("b", 1.0, 1.0)]);
        let (x, y) = parse_cluster_id(&cluster.id).expect("generated cluster id");

        assert_eq!(x.zoom, 3);
        assert_eq!(x.cell, 42);
        assert_eq!(y.zoom, 3);
        assert_eq!(y.cell, 17);
        assert!(parse_cluster_id("3:42:17").is_none());
        assert!(parse_cluster_id("z3:42").is_none());
        assert!(parse_cluster_id("z3:x:17").is_none());
    }

    #[test]
    fn clusters_points_for_bbox_and_zoom() {
        let index = ClusterIndex::new(
            [
                point("a", 13.0, 52.0),
                point("b", 13.0001, 52.0001),
                point("far", 80.0, 0.0),
            ],
            ClusterOptions::default(),
        )
        .unwrap();

        let items = index.get_clusters([12.0, 51.0, 14.0, 53.0], 1).unwrap();

        assert_eq!(items.len(), 1);
        assert!(matches!(
            &items[0],
            ClusterItem::Cluster(Cluster { point_count: 2, .. })
        ));
    }

    #[test]
    fn clusters_geo_core_coordinate_derived_points() {
        let coordinates = [
            Coordinate::new(13.0, 52.0).unwrap(),
            Coordinate::new(13.0001, 52.0001).unwrap(),
        ];
        let points = coordinates
            .into_iter()
            .enumerate()
            .map(|(index, coordinate)| point(&format!("p{index}"), coordinate.lon, coordinate.lat));
        let index = ClusterIndex::new(points, ClusterOptions::default()).unwrap();

        let items = index.get_clusters([12.0, 51.0, 14.0, 53.0], 1).unwrap();

        assert!(matches!(
            &items[0],
            ClusterItem::Cluster(Cluster { point_count: 2, .. })
        ));
    }

    #[test]
    fn returns_points_at_max_zoom() {
        let index = ClusterIndex::new([point("a", 13.0, 52.0)], ClusterOptions::default()).unwrap();

        let items = index.get_clusters([12.0, 51.0, 14.0, 53.0], 16).unwrap();

        assert_eq!(items, vec![ClusterItem::Point(point("a", 13.0, 52.0))]);
    }

    #[test]
    fn spatial_query_preserves_source_order() {
        let index = ClusterIndex::new(
            [
                point("first", 13.4, 52.4),
                point("outside", 80.0, 0.0),
                point("second", 13.2, 52.2),
            ],
            ClusterOptions::default(),
        )
        .unwrap();

        let items = index.get_clusters([13.0, 52.0, 14.0, 53.0], 16).unwrap();

        assert_eq!(
            items,
            vec![
                ClusterItem::Point(point("first", 13.4, 52.4)),
                ClusterItem::Point(point("second", 13.2, 52.2)),
            ]
        );
    }

    #[test]
    fn spatial_query_supports_antimeridian_bounds() {
        let index = ClusterIndex::new(
            [
                point("east", 179.5, 1.0),
                point("middle", 0.0, 1.0),
                point("west", -179.5, 1.0),
            ],
            ClusterOptions::default(),
        )
        .unwrap();

        let items = index.get_clusters([170.0, -5.0, -170.0, 5.0], 16).unwrap();

        assert_eq!(
            items,
            vec![
                ClusterItem::Point(point("east", 179.5, 1.0)),
                ClusterItem::Point(point("west", -179.5, 1.0)),
            ]
        );
    }
}
