use std::collections::HashMap;

use geo_core::Result;
use serde::{Deserialize, Serialize};

use crate::{GeoVizBounds, GeoVizMetricRecord, GeoVizPoint};

const EARTH_RADIUS_METERS: f64 = 6_371_008.8;
const DEFAULT_FIELD_COLUMNS: usize = 256;
const DEFAULT_INTERPOLATION_K: usize = 12;
const DEFAULT_INTERPOLATION_POWER: f64 = 2.0;
const DEFAULT_DOMAIN_PADDING_RATIO: f64 = 0.08;
const DEFAULT_EPSILON_METERS: f64 = 1.0;
const MAX_EXPLICIT_FIELD_SIZE: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
/// Options for IDW scalar field generation.
pub struct GeoVizScalarFieldOptions {
    /// Optional explicit domain bounds in `[west, south, east, north]` order.
    #[serde(default)]
    pub domain_bounds: Option<GeoVizBounds>,
    /// Padding ratio used for inferred bounds.
    #[serde(default)]
    pub domain_padding_ratio: Option<f64>,
    /// Desired field cell size in meters.
    #[serde(default)]
    pub field_cell_size_meters: Option<f64>,
    /// Explicit output column count.
    #[serde(default)]
    pub field_columns: Option<usize>,
    /// Explicit output row count.
    #[serde(default)]
    pub field_rows: Option<usize>,
    /// Coordinate distance treated as an exact source-point hit.
    #[serde(default)]
    pub interpolation_epsilon_meters: Option<f64>,
    /// Whether to fall back to nearest points outside `interpolationMaxDistanceMeters`.
    #[serde(default)]
    pub interpolation_extrapolate: Option<bool>,
    /// Number of nearest points used by IDW.
    #[serde(default)]
    pub interpolation_k: Option<usize>,
    /// Maximum candidate distance in meters.
    #[serde(default)]
    pub interpolation_max_distance_meters: Option<f64>,
    /// IDW distance exponent.
    #[serde(default)]
    pub interpolation_power: Option<f64>,
    /// Optional fixed value domain.
    #[serde(default)]
    pub value_domain: Option<[f64; 2]>,
    /// Metric key used as the scalar value. Defaults to `value`, then `weight`.
    #[serde(default)]
    pub value_metric: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Generated scalar field grid.
pub struct GeoVizScalarFieldGrid {
    /// Grid bounds in `[west, south, east, north]` order.
    pub bounds: GeoVizBounds,
    /// Number of columns in row-major `values`.
    pub columns: usize,
    /// Number of rows in row-major `values`.
    pub rows: usize,
    /// Min/max value domain, if resolvable.
    pub value_domain: Option<[f64; 2]>,
    /// Row-major grid values. `None` serializes to `null`.
    pub values: Vec<Option<f64>>,
}

#[derive(Debug, Clone)]
/// Reusable IDW scalar field index.
pub struct GeoVizScalarFieldIndex {
    bounds: GeoVizBounds,
    has_bounds: bool,
    options: GeoVizScalarFieldOptions,
    value_points: Vec<ScalarFieldValuePoint>,
}

#[derive(Debug, Clone)]
struct ScalarFieldValuePoint {
    id: String,
    index: usize,
    longitude: f64,
    latitude: f64,
    value: f64,
}

#[derive(Debug, Clone)]
struct ProjectedValuePoint {
    id: String,
    index: usize,
    value: f64,
    x: f64,
    y: f64,
    grid_column: i64,
    grid_row: i64,
}

#[derive(Debug, Clone, Copy)]
struct MetricPoint {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy)]
struct MetricProjection {
    longitude_scale: f64,
}

#[derive(Debug, Clone)]
struct SpatialGrid {
    cell_size_meters: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    max_column: i64,
    max_row: i64,
    min_column: i64,
    min_row: i64,
}

#[derive(Debug, Clone)]
struct DistanceCandidate {
    distance_meters: f64,
    distance_squared: f64,
    point_index: usize,
}

/// Creates an IDW scalar field grid from geographic points.
pub fn create_scalar_field_grid(
    points: impl IntoIterator<Item = GeoVizPoint>,
    options: GeoVizScalarFieldOptions,
) -> Result<GeoVizScalarFieldGrid> {
    let value_points = resolve_value_points(points, &options);
    let bounds = resolve_scalar_field_bounds(&value_points, &options);

    let Some(bounds) = bounds else {
        return Ok(GeoVizScalarFieldGrid {
            bounds: [0.0, 0.0, 0.0, 0.0],
            columns: 0,
            rows: 0,
            value_domain: None,
            values: Vec::new(),
        });
    };

    let (columns, rows) = resolve_scalar_field_dimensions(bounds, &options);
    if columns == 0 || rows == 0 {
        return Ok(GeoVizScalarFieldGrid {
            bounds,
            columns: 0,
            rows: 0,
            value_domain: resolve_value_domain(&value_points, &[], options.value_domain),
            values: Vec::new(),
        });
    }

    let interpolator = ScalarFieldInterpolator::new(&value_points, bounds, &options);
    let mut values = Vec::with_capacity(columns * rows);
    let [west, south, east, north] = bounds;
    let longitude_step = (east - west) / columns as f64;
    let latitude_step = (north - south) / rows as f64;

    for row in 0..rows {
        let latitude = north - latitude_step * (row as f64 + 0.5);
        for column in 0..columns {
            let longitude = west + longitude_step * (column as f64 + 0.5);
            values.push(interpolator.value_at([longitude, latitude]));
        }
    }

    Ok(GeoVizScalarFieldGrid {
        bounds,
        columns,
        rows,
        value_domain: resolve_value_domain(&value_points, &values, options.value_domain),
        values,
    })
}

impl GeoVizScalarFieldIndex {
    /// Builds a reusable scalar field index.
    pub fn new(
        points: impl IntoIterator<Item = GeoVizPoint>,
        options: GeoVizScalarFieldOptions,
    ) -> Result<Self> {
        let value_points = resolve_value_points(points, &options);
        let bounds = resolve_scalar_field_bounds(&value_points, &options);

        Ok(Self {
            bounds: bounds.unwrap_or([0.0, 0.0, 0.0, 0.0]),
            has_bounds: bounds.is_some(),
            options,
            value_points,
        })
    }

    /// Returns the scalar-field bounds, if available.
    pub fn get_bounds(&self) -> Option<GeoVizBounds> {
        self.has_bounds.then_some(self.bounds)
    }

    /// Returns the number of finite value points.
    pub fn point_count(&self) -> usize {
        self.value_points.len()
    }

    /// Returns the point-derived value domain before grid interpolation.
    pub fn value_domain(&self) -> Option<[f64; 2]> {
        resolve_value_domain(&self.value_points, &[], self.options.value_domain)
    }

    /// Samples the IDW interpolator at a longitude/latitude coordinate.
    pub fn get_value_at_coordinate(&self, coordinate: [f64; 2]) -> Result<Option<f64>> {
        if !coordinate[0].is_finite() || !coordinate[1].is_finite() {
            return Ok(None);
        }

        if self.value_points.is_empty() {
            return Ok(None);
        }

        Ok(
            ScalarFieldInterpolator::new(&self.value_points, self.bounds, &self.options)
                .value_at(coordinate),
        )
    }

    /// Creates a grid using this index's points and options.
    pub fn create_grid(&self) -> GeoVizScalarFieldGrid {
        let Some(bounds) = self.get_bounds() else {
            return GeoVizScalarFieldGrid {
                bounds: [0.0, 0.0, 0.0, 0.0],
                columns: 0,
                rows: 0,
                value_domain: None,
                values: Vec::new(),
            };
        };
        let (columns, rows) = resolve_scalar_field_dimensions(bounds, &self.options);
        let interpolator = ScalarFieldInterpolator::new(&self.value_points, bounds, &self.options);
        let mut values = Vec::with_capacity(columns * rows);
        let [west, south, east, north] = bounds;
        let longitude_step = (east - west) / columns as f64;
        let latitude_step = (north - south) / rows as f64;

        for row in 0..rows {
            let latitude = north - latitude_step * (row as f64 + 0.5);
            for column in 0..columns {
                let longitude = west + longitude_step * (column as f64 + 0.5);
                values.push(interpolator.value_at([longitude, latitude]));
            }
        }

        GeoVizScalarFieldGrid {
            bounds,
            columns,
            rows,
            value_domain: resolve_value_domain(
                &self.value_points,
                &values,
                self.options.value_domain,
            ),
            values,
        }
    }
}

struct ScalarFieldInterpolator {
    projected_points: Vec<ProjectedValuePoint>,
    projection: MetricProjection,
    spatial_grid: SpatialGrid,
    power: f64,
    k_nearest: usize,
    epsilon_meters: f64,
    max_distance_meters: Option<f64>,
    extrapolate: bool,
}

impl ScalarFieldInterpolator {
    fn new(
        value_points: &[ScalarFieldValuePoint],
        bounds: GeoVizBounds,
        options: &GeoVizScalarFieldOptions,
    ) -> Self {
        let projection = MetricProjection::new(bounds);
        let mut projected_points = value_points
            .iter()
            .map(|entry| {
                let projected = projection.project([entry.longitude, entry.latitude]);
                ProjectedValuePoint {
                    id: entry.id.clone(),
                    index: entry.index,
                    value: entry.value,
                    x: projected.x,
                    y: projected.y,
                    grid_column: 0,
                    grid_row: 0,
                }
            })
            .collect::<Vec<_>>();
        let projected_bounds = projected_bounds(bounds, projection);
        let spatial_grid =
            create_metric_spatial_grid(&mut projected_points, projected_bounds, options);

        Self {
            projected_points,
            projection,
            spatial_grid,
            power: positive_finite(options.interpolation_power, DEFAULT_INTERPOLATION_POWER),
            k_nearest: options
                .interpolation_k
                .filter(|value| *value > 0)
                .unwrap_or(DEFAULT_INTERPOLATION_K),
            epsilon_meters: positive_finite(
                options.interpolation_epsilon_meters,
                DEFAULT_EPSILON_METERS,
            )
            .max(0.0),
            max_distance_meters: positive_finite_option(options.interpolation_max_distance_meters),
            extrapolate: options.interpolation_extrapolate.unwrap_or(true),
        }
    }

    fn value_at(&self, coordinate: [f64; 2]) -> Option<f64> {
        if self.projected_points.is_empty()
            || !coordinate[0].is_finite()
            || !coordinate[1].is_finite()
        {
            return None;
        }

        let projected = self.projection.project(coordinate);
        let mut candidates = spatial_grid_candidates(
            projected,
            &self.spatial_grid,
            &self.projected_points,
            self.k_nearest,
            self.max_distance_meters,
        );

        if let Some(max_distance_meters) = self.max_distance_meters {
            candidates.retain(|candidate| candidate.distance_meters <= max_distance_meters);
            if candidates.is_empty() && self.extrapolate {
                candidates = spatial_grid_candidates(
                    projected,
                    &self.spatial_grid,
                    &self.projected_points,
                    self.k_nearest,
                    None,
                );
            }
        }

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_by(|left, right| compare_candidates(left, right, &self.projected_points));

        if let Some(exact) = candidates
            .iter()
            .find(|candidate| candidate.distance_meters <= self.epsilon_meters)
        {
            return Some(self.projected_points[exact.point_index].value);
        }

        interpolate_idw(
            &candidates[..candidates.len().min(self.k_nearest)],
            &self.projected_points,
            self.power,
        )
    }
}

impl MetricProjection {
    fn new(bounds: GeoVizBounds) -> Self {
        let center_latitude_radians = ((bounds[1] + bounds[3]) / 2.0).to_radians();
        Self {
            longitude_scale: center_latitude_radians.cos(),
        }
    }

    fn project(self, [longitude, latitude]: [f64; 2]) -> MetricPoint {
        MetricPoint {
            x: EARTH_RADIUS_METERS * longitude.to_radians() * self.longitude_scale,
            y: EARTH_RADIUS_METERS * latitude.to_radians(),
        }
    }
}

fn resolve_value_points(
    points: impl IntoIterator<Item = GeoVizPoint>,
    options: &GeoVizScalarFieldOptions,
) -> Vec<ScalarFieldValuePoint> {
    points
        .into_iter()
        .enumerate()
        .filter_map(|(index, point)| {
            if !point.longitude.is_finite() || !point.latitude.is_finite() {
                return None;
            }
            let metrics = finite_metrics(point.metrics);
            let value = resolve_point_value(&metrics, options.value_metric.as_deref());
            value.is_finite().then_some(ScalarFieldValuePoint {
                id: point.id.unwrap_or_else(|| index.to_string()),
                index,
                longitude: point.longitude,
                latitude: point.latitude,
                value,
            })
        })
        .collect()
}

fn finite_metrics(metrics: GeoVizMetricRecord) -> GeoVizMetricRecord {
    metrics
        .into_iter()
        .filter(|(_, value)| value.is_finite())
        .collect()
}

fn resolve_point_value(metrics: &GeoVizMetricRecord, value_metric: Option<&str>) -> f64 {
    value_metric
        .and_then(|key| metrics.get(key))
        .copied()
        .or_else(|| metrics.get("value").copied())
        .or_else(|| metrics.get("weight").copied())
        .unwrap_or(f64::NAN)
}

fn resolve_scalar_field_bounds(
    value_points: &[ScalarFieldValuePoint],
    options: &GeoVizScalarFieldOptions,
) -> Option<GeoVizBounds> {
    if let Some(bounds) = options.domain_bounds {
        return normalize_bounds(bounds);
    }

    let first = value_points.first()?;
    let mut west = first.longitude;
    let mut south = first.latitude;
    let mut east = first.longitude;
    let mut north = first.latitude;

    for point in value_points.iter().skip(1) {
        west = west.min(point.longitude);
        south = south.min(point.latitude);
        east = east.max(point.longitude);
        north = north.max(point.latitude);
    }

    let longitude_span = (east - west).max(1.0);
    let latitude_span = (north - south).max(1.0);
    let padding_ratio = options
        .domain_padding_ratio
        .filter(|value| value.is_finite())
        .unwrap_or(DEFAULT_DOMAIN_PADDING_RATIO)
        .max(0.0);

    normalize_bounds([
        west - longitude_span * padding_ratio,
        south - latitude_span * padding_ratio,
        east + longitude_span * padding_ratio,
        north + latitude_span * padding_ratio,
    ])
}

fn normalize_bounds(bounds: GeoVizBounds) -> Option<GeoVizBounds> {
    if bounds.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let west = bounds[0].min(bounds[2]).clamp(-180.0, 180.0);
    let east = bounds[0].max(bounds[2]).clamp(-180.0, 180.0);
    let south = bounds[1].min(bounds[3]).clamp(-90.0, 90.0);
    let north = bounds[1].max(bounds[3]).clamp(-90.0, 90.0);

    (west != east && south != north).then_some([west, south, east, north])
}

fn resolve_scalar_field_dimensions(
    bounds: GeoVizBounds,
    options: &GeoVizScalarFieldOptions,
) -> (usize, usize) {
    let projection = MetricProjection::new(bounds);
    let projected_bounds = projected_bounds(bounds, projection);
    let width_meters = (projected_bounds[2] - projected_bounds[0]).max(1.0);
    let height_meters = (projected_bounds[3] - projected_bounds[1]).max(1.0);
    let aspect_ratio = width_meters / height_meters;
    let explicit_columns = positive_integer(options.field_columns);
    let explicit_rows = positive_integer(options.field_rows);

    if let (Some(columns), Some(rows)) = (explicit_columns, explicit_rows) {
        return (
            columns.clamp(1, MAX_EXPLICIT_FIELD_SIZE),
            rows.clamp(1, MAX_EXPLICIT_FIELD_SIZE),
        );
    }

    if let Some(cell_size) = positive_finite_option(options.field_cell_size_meters) {
        return (
            ((width_meters / cell_size).ceil() as usize).clamp(1, MAX_EXPLICIT_FIELD_SIZE),
            ((height_meters / cell_size).ceil() as usize).clamp(1, MAX_EXPLICIT_FIELD_SIZE),
        );
    }

    if let Some(columns) = explicit_columns {
        return (
            columns.clamp(1, MAX_EXPLICIT_FIELD_SIZE),
            ((columns as f64 / aspect_ratio.max(0.001)).round() as usize)
                .clamp(1, MAX_EXPLICIT_FIELD_SIZE),
        );
    }

    if let Some(rows) = explicit_rows {
        return (
            ((rows as f64 * aspect_ratio).round() as usize).clamp(1, MAX_EXPLICIT_FIELD_SIZE),
            rows.clamp(1, MAX_EXPLICIT_FIELD_SIZE),
        );
    }

    (
        DEFAULT_FIELD_COLUMNS,
        ((DEFAULT_FIELD_COLUMNS as f64 / aspect_ratio.max(0.001)).round() as usize)
            .clamp(1, DEFAULT_FIELD_COLUMNS),
    )
}

fn projected_bounds(bounds: GeoVizBounds, projection: MetricProjection) -> [f64; 4] {
    let south_west = projection.project([bounds[0], bounds[1]]);
    let north_east = projection.project([bounds[2], bounds[3]]);

    [
        south_west.x.min(north_east.x),
        south_west.y.min(north_east.y),
        south_west.x.max(north_east.x),
        south_west.y.max(north_east.y),
    ]
}

fn create_metric_spatial_grid(
    points: &mut [ProjectedValuePoint],
    projected_bounds: [f64; 4],
    options: &GeoVizScalarFieldOptions,
) -> SpatialGrid {
    let domain_width = (projected_bounds[2] - projected_bounds[0]).max(1.0);
    let domain_height = (projected_bounds[3] - projected_bounds[1]).max(1.0);
    let default_cell_size =
        25_000.0_f64.max(((domain_width * domain_height) / points.len().max(1) as f64).sqrt());
    let cell_size_meters = positive_finite_option(options.interpolation_max_distance_meters)
        .unwrap_or(default_cell_size);
    let cell_size_meters = cell_size_meters.max(1.0);
    let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut min_column = i64::MAX;
    let mut min_row = i64::MAX;
    let mut max_column = i64::MIN;
    let mut max_row = i64::MIN;

    for (index, point) in points.iter_mut().enumerate() {
        point.grid_column = (point.x / cell_size_meters).floor() as i64;
        point.grid_row = (point.y / cell_size_meters).floor() as i64;
        min_column = min_column.min(point.grid_column);
        max_column = max_column.max(point.grid_column);
        min_row = min_row.min(point.grid_row);
        max_row = max_row.max(point.grid_row);
        cells
            .entry((point.grid_column, point.grid_row))
            .or_default()
            .push(index);
    }

    SpatialGrid {
        cell_size_meters,
        cells,
        max_column: if max_column == i64::MIN {
            0
        } else {
            max_column
        },
        max_row: if max_row == i64::MIN { 0 } else { max_row },
        min_column: if min_column == i64::MAX {
            0
        } else {
            min_column
        },
        min_row: if min_row == i64::MAX { 0 } else { min_row },
    }
}

fn spatial_grid_candidates(
    target: MetricPoint,
    grid: &SpatialGrid,
    points: &[ProjectedValuePoint],
    k_nearest: usize,
    max_distance_meters: Option<f64>,
) -> Vec<DistanceCandidate> {
    if grid.cells.is_empty() {
        return Vec::new();
    }

    let target_column = (target.x / grid.cell_size_meters).floor() as i64;
    let target_row = (target.y / grid.cell_size_meters).floor() as i64;
    let mut candidates = Vec::new();
    let max_ring = max_distance_meters
        .map(|distance| (distance / grid.cell_size_meters).ceil() as i64)
        .unwrap_or_else(|| {
            (target_column - grid.min_column)
                .abs()
                .max((target_column - grid.max_column).abs())
                .max((target_row - grid.min_row).abs())
                .max((target_row - grid.max_row).abs())
        });

    for ring in 0..=max_ring {
        for_each_spatial_grid_ring_cell(target_column, target_row, ring, |column, row| {
            if let Some(cell) = grid.cells.get(&(column, row)) {
                for point_index in cell {
                    let point = &points[*point_index];
                    let dx = point.x - target.x;
                    let dy = point.y - target.y;
                    let distance_squared = dx * dx + dy * dy;
                    candidates.push(DistanceCandidate {
                        distance_meters: distance_squared.sqrt(),
                        distance_squared,
                        point_index: *point_index,
                    });
                }
            }
        });

        if max_distance_meters.is_some() {
            continue;
        }

        if candidates.len() >= k_nearest {
            candidates.sort_by(|left, right| compare_candidates(left, right, points));
            let farthest_distance_squared = candidates
                .get(k_nearest - 1)
                .map(|candidate| candidate.distance_squared)
                .unwrap_or(0.0);

            if farthest_distance_squared
                <= distance_squared_to_spatial_grid_ring_exit(
                    target,
                    grid,
                    target_column,
                    target_row,
                    ring,
                )
            {
                break;
            }
        }
    }

    candidates.sort_by(|left, right| compare_candidates(left, right, points));
    candidates.truncate(k_nearest);
    candidates
}

fn distance_squared_to_spatial_grid_ring_exit(
    target: MetricPoint,
    grid: &SpatialGrid,
    target_column: i64,
    target_row: i64,
    ring: i64,
) -> f64 {
    let min_x = (target_column - ring) as f64 * grid.cell_size_meters;
    let max_x = (target_column + ring + 1) as f64 * grid.cell_size_meters;
    let min_y = (target_row - ring) as f64 * grid.cell_size_meters;
    let max_y = (target_row + ring + 1) as f64 * grid.cell_size_meters;
    let distance_to_exit = (target.x - min_x)
        .abs()
        .min((target.x - max_x).abs())
        .min((target.y - min_y).abs())
        .min((target.y - max_y).abs());

    distance_to_exit * distance_to_exit
}

fn for_each_spatial_grid_ring_cell(
    target_column: i64,
    target_row: i64,
    ring: i64,
    mut visit: impl FnMut(i64, i64),
) {
    if ring == 0 {
        visit(target_column, target_row);
        return;
    }

    for column in (target_column - ring)..=(target_column + ring) {
        visit(column, target_row - ring);
        visit(column, target_row + ring);
    }

    for row in (target_row - ring + 1)..=(target_row + ring - 1) {
        visit(target_column - ring, row);
        visit(target_column + ring, row);
    }
}

fn compare_candidates(
    left: &DistanceCandidate,
    right: &DistanceCandidate,
    points: &[ProjectedValuePoint],
) -> std::cmp::Ordering {
    left.distance_squared
        .total_cmp(&right.distance_squared)
        .then_with(|| {
            points[left.point_index]
                .index
                .cmp(&points[right.point_index].index)
        })
        .then_with(|| {
            points[left.point_index]
                .id
                .cmp(&points[right.point_index].id)
        })
}

fn interpolate_idw(
    candidates: &[DistanceCandidate],
    points: &[ProjectedValuePoint],
    power: f64,
) -> Option<f64> {
    let mut weighted_value = 0.0;
    let mut total_weight = 0.0;

    for candidate in candidates {
        if candidate.distance_meters <= 0.0 {
            return Some(points[candidate.point_index].value);
        }

        let weight = 1.0 / candidate.distance_meters.powf(power);
        weighted_value += weight * points[candidate.point_index].value;
        total_weight += weight;
    }

    (total_weight > 0.0).then_some(weighted_value / total_weight)
}

fn resolve_value_domain(
    value_points: &[ScalarFieldValuePoint],
    grid_values: &[Option<f64>],
    value_domain: Option<[f64; 2]>,
) -> Option<[f64; 2]> {
    if let Some([min, max]) =
        value_domain.filter(|domain| domain.iter().all(|value| value.is_finite()))
    {
        return Some(if min <= max { [min, max] } else { [max, min] });
    }

    let mut values = if grid_values.iter().any(Option::is_some) {
        grid_values.iter().flatten().copied().collect::<Vec<_>>()
    } else {
        value_points
            .iter()
            .map(|point| point.value)
            .collect::<Vec<_>>()
    };
    values.retain(|value| value.is_finite());
    let first = *values.first()?;
    let mut min = first;
    let mut max = first;

    for value in values.into_iter().skip(1) {
        min = min.min(value);
        max = max.max(value);
    }

    Some([min, max])
}

fn positive_integer(value: Option<usize>) -> Option<usize> {
    value.filter(|value| *value > 0)
}

fn positive_finite(value: Option<f64>, fallback: f64) -> f64 {
    value
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(fallback)
}

fn positive_finite_option(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idw_returns_source_value_for_exact_match() {
        let index = GeoVizScalarFieldIndex::new(
            [point("a", 13.0, 52.0, 21.5)],
            GeoVizScalarFieldOptions {
                domain_bounds: Some([12.0, 51.0, 14.0, 53.0]),
                value_metric: Some("temperature".to_string()),
                ..Default::default()
            },
        )
        .expect("index");

        assert_eq!(
            index.get_value_at_coordinate([13.0, 52.0]).expect("sample"),
            Some(21.5)
        );
    }

    #[test]
    fn idw_averages_two_equal_distance_points() {
        let index = GeoVizScalarFieldIndex::new(
            [point("west", 0.0, 0.0, 10.0), point("east", 2.0, 0.0, 20.0)],
            GeoVizScalarFieldOptions {
                domain_bounds: Some([0.0, -1.0, 2.0, 1.0]),
                interpolation_k: Some(2),
                value_metric: Some("temperature".to_string()),
                ..Default::default()
            },
        )
        .expect("index");

        let value = index
            .get_value_at_coordinate([1.0, 0.0])
            .expect("sample")
            .expect("value");

        assert!((value - 15.0).abs() < 1e-8);
    }

    #[test]
    fn grid_ignores_invalid_coordinates_and_values() {
        let grid = create_scalar_field_grid(
            [
                point("valid", 0.0, 0.0, 8.0),
                point("bad-latitude", 1.0, f64::NAN, 99.0),
                point("bad-value", 2.0, 0.0, f64::NAN),
            ],
            GeoVizScalarFieldOptions {
                domain_bounds: Some([-1.0, -1.0, 1.0, 1.0]),
                field_columns: Some(2),
                field_rows: Some(2),
                value_metric: Some("temperature".to_string()),
                ..Default::default()
            },
        )
        .expect("grid");

        assert_eq!(
            grid.values,
            vec![Some(8.0), Some(8.0), Some(8.0), Some(8.0)]
        );
        assert_eq!(grid.value_domain, Some([8.0, 8.0]));
    }

    #[test]
    fn grid_returns_nulls_for_empty_points_with_explicit_bounds() {
        let grid = create_scalar_field_grid(
            [],
            GeoVizScalarFieldOptions {
                domain_bounds: Some([0.0, 0.0, 1.0, 1.0]),
                field_columns: Some(2),
                field_rows: Some(2),
                value_metric: Some("temperature".to_string()),
                ..Default::default()
            },
        )
        .expect("grid");

        assert_eq!(grid.bounds, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(grid.columns, 2);
        assert_eq!(grid.rows, 2);
        assert_eq!(grid.value_domain, None);
        assert_eq!(grid.values, vec![None, None, None, None]);
    }

    #[test]
    fn max_distance_without_extrapolation_returns_none() {
        let index = GeoVizScalarFieldIndex::new(
            [point("a", 0.0, 0.0, 12.0)],
            GeoVizScalarFieldOptions {
                domain_bounds: Some([-10.0, -10.0, 10.0, 10.0]),
                interpolation_extrapolate: Some(false),
                interpolation_max_distance_meters: Some(1_000.0),
                value_metric: Some("temperature".to_string()),
                ..Default::default()
            },
        )
        .expect("index");

        assert_eq!(
            index.get_value_at_coordinate([5.0, 5.0]).expect("sample"),
            None
        );
    }

    #[test]
    fn positive_option_helpers_ignore_non_positive_or_non_finite_values() {
        assert_eq!(positive_integer(Some(3)), Some(3));
        assert_eq!(positive_integer(Some(0)), None);
        assert_eq!(positive_integer(None), None);
        assert_eq!(positive_finite(Some(2.5), 1.0), 2.5);
        assert_eq!(positive_finite(Some(-2.5), 1.0), 1.0);
        assert_eq!(positive_finite(Some(f64::NAN), 1.0), 1.0);
        assert_eq!(positive_finite_option(Some(2.5)), Some(2.5));
        assert_eq!(positive_finite_option(Some(0.0)), None);
        assert_eq!(positive_finite_option(Some(f64::INFINITY)), None);
    }

    #[test]
    fn normalize_bounds_rejects_invalid_or_degenerate_bounds() {
        assert_eq!(
            normalize_bounds([1.0, 1.0, 0.0, 0.0]),
            Some([0.0, 0.0, 1.0, 1.0])
        );
        assert_eq!(normalize_bounds([0.0, 0.0, 0.0, 1.0]), None);
        assert_eq!(normalize_bounds([0.0, f64::NAN, 1.0, 1.0]), None);
    }

    #[test]
    fn scalar_field_dimensions_use_explicit_clamped_and_fallback_sizes() {
        let explicit = resolve_scalar_field_dimensions(
            [0.0, 0.0, 1.0, 1.0],
            &GeoVizScalarFieldOptions {
                field_columns: Some(MAX_EXPLICIT_FIELD_SIZE + 1),
                field_rows: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(explicit.0, MAX_EXPLICIT_FIELD_SIZE);
        assert!(explicit.1 >= 1);

        let from_cell_size = resolve_scalar_field_dimensions(
            [0.0, 0.0, 1.0, 1.0],
            &GeoVizScalarFieldOptions {
                field_cell_size_meters: Some(50_000.0),
                ..Default::default()
            },
        );
        assert!(from_cell_size.0 > 1);
        assert!(from_cell_size.1 > 1);

        let fallback = resolve_scalar_field_dimensions([0.0, 0.0, 1.0, 1.0], &Default::default());
        assert_eq!(fallback.0, DEFAULT_FIELD_COLUMNS);
        assert!(fallback.1 >= 1);
    }

    fn point(id: &str, longitude: f64, latitude: f64, temperature: f64) -> GeoVizPoint {
        let metrics = [("temperature".to_string(), temperature)]
            .into_iter()
            .collect();
        GeoVizPoint {
            id: Some(id.to_string()),
            label: None,
            longitude,
            latitude,
            metrics,
            properties: serde_json::Value::Object(Default::default()),
        }
    }
}
