#![doc = include_str!("../README.md")]

pub mod surface;
use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Error type for geospatial domain validation and local data access.
#[derive(Debug)]
pub enum GeoError {
    /// Caller supplied invalid geospatial input.
    InvalidArgument(String),
    /// Source data could not be parsed or read as valid geospatial input.
    Source(String),
    /// Underlying filesystem I/O failed.
    Io(std::io::Error),
}

impl GeoError {
    /// Creates an invalid-argument error.
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument(message.into())
    }

    /// Creates a source-data error.
    pub fn source(message: impl Into<String>) -> Self {
        Self::Source(message.into())
    }
}

impl fmt::Display for GeoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument(message) => write!(formatter, "invalid argument: {message}"),
            Self::Source(message) => write!(formatter, "source error: {message}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
        }
    }
}

impl std::error::Error for GeoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidArgument(_) | Self::Source(_) => None,
        }
    }
}

impl From<std::io::Error> for GeoError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Geo crate result type.
pub type Result<T> = std::result::Result<T, GeoError>;

/// JSON object used for feature properties.
pub type Properties = BTreeMap<String, Value>;

/// Two-dimensional coordinate position in `[longitude, latitude]` order.
pub type Position = [f64; 2];

const GEOMETRY_EPSILON: f64 = 1e-12;

fn invalid_argument(message: impl Into<String>) -> GeoError {
    GeoError::invalid_argument(message)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
/// Data type for a 2D geographic coordinate.
pub struct Coordinate {
    /// Longitude or x coordinate.
    pub lon: f64,
    /// Latitude or y coordinate.
    pub lat: f64,
}

impl Coordinate {
    /// Creates a new value.
    pub fn new(lon: f64, lat: f64) -> Result<Self> {
        let coordinate = Self { lon, lat };
        coordinate.validate()?;
        Ok(coordinate)
    }

    /// Builds this value from a GeoJSON position.
    pub fn from_position(position: Position) -> Result<Self> {
        Self::new(position[0], position[1])
    }

    /// Returns this coordinate as a GeoJSON position.
    pub fn as_position(self) -> Position {
        [self.lon, self.lat]
    }

    /// Validates this value.
    pub fn validate(self) -> Result<()> {
        if !self.lon.is_finite() || !self.lat.is_finite() {
            return Err(invalid_argument("coordinate values must be finite"));
        }
        Ok(())
    }

    /// Validates this value as a longitude/latitude coordinate.
    pub fn validate_geographic(self) -> Result<()> {
        self.validate()?;
        if !(-180.0..=180.0).contains(&self.lon) {
            return Err(invalid_argument(
                "coordinate longitude must be between -180 and 180",
            ));
        }
        if !(-90.0..=90.0).contains(&self.lat) {
            return Err(invalid_argument(
                "coordinate latitude must be between -90 and 90",
            ));
        }
        Ok(())
    }
}

impl From<Coordinate> for Position {
    fn from(value: Coordinate) -> Self {
        value.as_position()
    }
}

impl TryFrom<Position> for Coordinate {
    type Error = GeoError;

    fn try_from(value: Position) -> Result<Self> {
        Self::from_position(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
/// Data type for a 2D bounding box.
pub struct BBox {
    /// Minimum longitude or x coordinate.
    pub min_lon: f64,
    /// Minimum latitude or y coordinate.
    pub min_lat: f64,
    /// Maximum longitude or x coordinate.
    pub max_lon: f64,
    /// Maximum latitude or y coordinate.
    pub max_lat: f64,
}

impl BBox {
    /// Creates a new value from `[min_lon, min_lat, max_lon, max_lat]`.
    pub fn new(values: [f64; 4]) -> Result<Self> {
        let bbox = Self {
            min_lon: values[0],
            min_lat: values[1],
            max_lon: values[2],
            max_lat: values[3],
        };
        bbox.validate()?;
        Ok(bbox)
    }

    /// Returns this value as `[min_lon, min_lat, max_lon, max_lat]`.
    pub fn as_array(self) -> [f64; 4] {
        [self.min_lon, self.min_lat, self.max_lon, self.max_lat]
    }

    /// Validates this value.
    pub fn validate(self) -> Result<()> {
        if !self.min_lon.is_finite()
            || !self.min_lat.is_finite()
            || !self.max_lon.is_finite()
            || !self.max_lat.is_finite()
        {
            return Err(invalid_argument("bbox values must be finite"));
        }
        if self.min_lon > self.max_lon {
            return Err(invalid_argument("bbox min_lon must be <= max_lon"));
        }
        if self.min_lat > self.max_lat {
            return Err(invalid_argument("bbox min_lat must be <= max_lat"));
        }
        Ok(())
    }

    /// Validates this value as a longitude/latitude bounding box.
    pub fn validate_geographic(self) -> Result<()> {
        self.validate()?;
        Coordinate::new(self.min_lon, self.min_lat)?.validate_geographic()?;
        Coordinate::new(self.max_lon, self.max_lat)?.validate_geographic()?;
        Ok(())
    }

    /// Returns true when this bbox contains a coordinate.
    pub fn contains(self, coordinate: Coordinate) -> bool {
        coordinate.lon >= self.min_lon
            && coordinate.lon <= self.max_lon
            && coordinate.lat >= self.min_lat
            && coordinate.lat <= self.max_lat
    }

    /// Returns true when this bbox contains at least one coordinate.
    pub fn intersects_coordinates(self, coordinates: &[Coordinate]) -> bool {
        coordinates
            .iter()
            .copied()
            .any(|coordinate| self.contains(coordinate))
    }

    /// Returns true when this bbox intersects a geometry.
    pub fn intersects_geometry(self, geometry: &Geometry) -> bool {
        geometry_intersects_bbox(geometry, self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
/// GeoJSON-shaped geometry data.
pub enum Geometry {
    /// Point geometry.
    Point {
        /// Coordinate position.
        coordinates: Position,
    },
    /// MultiPoint geometry.
    MultiPoint {
        /// Coordinate positions.
        coordinates: Vec<Position>,
    },
    /// LineString geometry.
    LineString {
        /// Coordinate positions.
        coordinates: Vec<Position>,
    },
    /// MultiLineString geometry.
    MultiLineString {
        /// Coordinate lines.
        coordinates: Vec<Vec<Position>>,
    },
    /// Polygon geometry.
    Polygon {
        /// Linear rings. The first ring is the exterior ring.
        coordinates: Vec<Vec<Position>>,
    },
    /// MultiPolygon geometry.
    MultiPolygon {
        /// Polygon rings grouped by polygon.
        coordinates: Vec<Vec<Vec<Position>>>,
    },
    /// GeometryCollection geometry.
    GeometryCollection {
        /// Child geometries.
        geometries: Vec<Geometry>,
    },
}

impl Geometry {
    /// Validates this geometry.
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Point { coordinates } => {
                Coordinate::from_position(*coordinates)?;
            }
            Self::MultiPoint { coordinates } => {
                validate_positions(coordinates, "multipoint coordinates")?;
            }
            Self::LineString { coordinates } => {
                validate_line_positions(coordinates, "linestring coordinates")?;
            }
            Self::MultiLineString { coordinates } => {
                for line in coordinates {
                    validate_line_positions(line, "multilinestring coordinates")?;
                }
            }
            Self::Polygon { coordinates } => {
                validate_polygon_positions(coordinates, "polygon coordinates")?;
            }
            Self::MultiPolygon { coordinates } => {
                for polygon in coordinates {
                    validate_polygon_positions(polygon, "multipolygon coordinates")?;
                }
            }
            Self::GeometryCollection { geometries } => {
                for geometry in geometries {
                    geometry.validate()?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// GeoJSON-compatible feature data.
pub struct GeoFeature {
    /// Optional feature id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Optional feature bounding box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
    /// Optional feature geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry: Option<Geometry>,
    /// Feature properties.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: Properties,
}

impl GeoFeature {
    /// Creates a new value.
    pub fn new(geometry: Option<Geometry>) -> Self {
        Self {
            id: None,
            bbox: None,
            geometry,
            properties: Properties::new(),
        }
    }

    /// Returns this feature with an id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Returns this feature with a bbox.
    pub fn with_bbox(mut self, bbox: BBox) -> Result<Self> {
        bbox.validate()?;
        self.bbox = Some(bbox);
        Ok(self)
    }

    /// Inserts a property value.
    pub fn insert_property(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.properties.insert(key.into(), value.into());
    }

    /// Validates this feature.
    pub fn validate(&self) -> Result<()> {
        if self.id.as_ref().is_some_and(String::is_empty) {
            return Err(invalid_argument("feature id must not be empty"));
        }
        if let Some(bbox) = self.bbox {
            bbox.validate()?;
        }
        if let Some(geometry) = &self.geometry {
            geometry.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// GeoJSON-compatible feature collection data.
pub struct GeoFeatureCollection {
    /// Optional collection bounding box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
    /// Collection features.
    pub features: Vec<GeoFeature>,
}

impl GeoFeatureCollection {
    /// Creates a new collection.
    pub fn new(features: impl Into<Vec<GeoFeature>>) -> Self {
        Self {
            bbox: None,
            features: features.into(),
        }
    }

    /// Adds a feature to this collection.
    pub fn push(&mut self, feature: GeoFeature) {
        self.features.push(feature);
    }

    /// Filters features whose geometry intersects a bbox.
    pub fn filter_intersecting_bbox(&self, bbox: BBox) -> Self {
        Self {
            bbox: self.bbox,
            features: self
                .features
                .iter()
                .filter(|feature| {
                    feature
                        .geometry
                        .as_ref()
                        .is_some_and(|geometry| bbox.intersects_geometry(geometry))
                })
                .cloned()
                .collect(),
        }
    }

    /// Validates this collection.
    pub fn validate(&self) -> Result<()> {
        if let Some(bbox) = self.bbox {
            bbox.validate()?;
        }
        for feature in &self.features {
            feature.validate()?;
        }
        Ok(())
    }
}

/// Creates point geometry.
pub fn point(coordinate: Coordinate) -> Geometry {
    Geometry::Point {
        coordinates: coordinate.as_position(),
    }
}

/// Creates linestring geometry from coordinates.
pub fn line_string(coordinates: &[Coordinate]) -> Result<Geometry> {
    let coordinates = coordinates_to_positions(coordinates)?;
    validate_line_positions(&coordinates, "linestring coordinates")?;
    Ok(Geometry::LineString { coordinates })
}

/// Creates polygon or multipolygon geometry from polygon rings.
pub fn polygon_or_multipolygon(polygons: Vec<Vec<Vec<Coordinate>>>) -> Option<Geometry> {
    let mut output: Vec<Vec<Vec<Position>>> = polygons
        .into_iter()
        .map(|polygon| {
            polygon
                .into_iter()
                .map(|ring| ring.into_iter().map(Coordinate::as_position).collect())
                .collect()
        })
        .collect();

    match output.len() {
        0 => None,
        1 => Some(Geometry::Polygon {
            coordinates: output.remove(0),
        }),
        _ => Some(Geometry::MultiPolygon {
            coordinates: output,
        }),
    }
}

/// Assembles polygon or multipolygon geometry from outer and inner ring segments.
pub fn assemble_multipolygon(
    outer_segments: Vec<Vec<Coordinate>>,
    inner_segments: Vec<Vec<Coordinate>>,
) -> Result<Geometry> {
    let mut outer_rings = stitch_rings(outer_segments)
        .ok_or_else(|| invalid_argument("outer ring segments could not be stitched"))?;
    let mut inner_rings = stitch_rings(inner_segments)
        .ok_or_else(|| invalid_argument("inner ring segments could not be stitched"))?;

    if outer_rings.is_empty() {
        return Err(invalid_argument(
            "multipolygon requires at least one outer ring",
        ));
    }

    for ring in &mut outer_rings {
        normalize_ring_orientation(ring, true);
    }
    for ring in &mut inner_rings {
        normalize_ring_orientation(ring, false);
    }

    let mut polygons: Vec<Vec<Vec<Coordinate>>> =
        outer_rings.into_iter().map(|outer| vec![outer]).collect();

    for inner in inner_rings {
        let Some(point) = inner.first().copied() else {
            return Err(invalid_argument("inner ring must not be empty"));
        };
        let Some((target_index, _)) = polygons
            .iter()
            .enumerate()
            .filter_map(|(index, polygon)| {
                let outer = &polygon[0];
                if point_in_ring(point, outer) {
                    Some((index, ring_area(outer).abs()))
                } else {
                    None
                }
            })
            .min_by(|(_, left), (_, right)| left.total_cmp(right))
        else {
            return Err(invalid_argument(
                "inner ring is not contained by an outer ring",
            ));
        };
        polygons[target_index].push(inner);
    }

    polygon_or_multipolygon(polygons)
        .ok_or_else(|| invalid_argument("multipolygon requires at least one polygon"))
}

/// Stitches line segments into closed rings.
pub fn stitch_rings(mut segments: Vec<Vec<Coordinate>>) -> Option<Vec<Vec<Coordinate>>> {
    let mut rings = Vec::new();

    while !segments.is_empty() {
        let mut ring = segments.remove(0);
        if ring.len() < 2 {
            return None;
        }

        loop {
            if is_valid_closed_ring(&ring) {
                rings.push(ring);
                break;
            }

            let (index, action) = find_connecting_segment(&ring, &segments)?;
            let segment = segments.remove(index);
            apply_segment(&mut ring, segment, action);
        }
    }

    Some(rings)
}

/// Returns true when a ring has at least four positions and matching first/last points.
pub fn is_valid_closed_ring(ring: &[Coordinate]) -> bool {
    ring.len() >= 4 && ring.first() == ring.last()
}

/// Returns signed planar area for a closed ring.
pub fn ring_area(ring: &[Coordinate]) -> f64 {
    if ring.len() < 4 {
        return 0.0;
    }
    ring.windows(2)
        .map(|window| {
            let a = window[0];
            let b = window[1];
            (a.lon * b.lat) - (b.lon * a.lat)
        })
        .sum::<f64>()
        / 2.0
}

/// Reverses a ring when needed to match the requested orientation.
pub fn normalize_ring_orientation(ring: &mut [Coordinate], counter_clockwise: bool) {
    let is_counter_clockwise = ring_area(ring) > 0.0;
    if is_counter_clockwise != counter_clockwise {
        ring.reverse();
    }
}

/// Returns true when a point lies inside a closed ring.
pub fn point_in_ring(point: Coordinate, ring: &[Coordinate]) -> bool {
    if ring.len() < 4 {
        return false;
    }

    let mut inside = false;
    let mut previous = ring[ring.len() - 1];
    for current in ring.iter().copied() {
        let intersects = ((current.lat > point.lat) != (previous.lat > point.lat))
            && (point.lon
                < (previous.lon - current.lon) * (point.lat - current.lat)
                    / (previous.lat - current.lat)
                    + current.lon);
        if intersects {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

/// Returns true when a geometry intersects a bbox.
pub fn geometry_intersects_bbox(geometry: &Geometry, bbox: BBox) -> bool {
    match geometry {
        Geometry::Point { coordinates } => Coordinate::from_position(*coordinates)
            .map(|coordinate| bbox.contains(coordinate))
            .unwrap_or(false),
        Geometry::MultiPoint { coordinates } => point_positions_intersect_bbox(coordinates, bbox),
        Geometry::LineString { coordinates } => line_positions_intersect_bbox(coordinates, bbox),
        Geometry::MultiLineString { coordinates } => coordinates
            .iter()
            .any(|line| line_positions_intersect_bbox(line, bbox)),
        Geometry::Polygon { coordinates } => polygon_positions_intersect_bbox(coordinates, bbox),
        Geometry::MultiPolygon { coordinates } => coordinates
            .iter()
            .any(|polygon| polygon_positions_intersect_bbox(polygon, bbox)),
        Geometry::GeometryCollection { geometries } => geometries
            .iter()
            .any(|geometry| geometry_intersects_bbox(geometry, bbox)),
    }
}

fn line_positions_intersect_bbox(coordinates: &[Position], bbox: BBox) -> bool {
    if positions_intersect_bbox(coordinates, bbox) {
        return true;
    }
    positions_to_coordinates(coordinates)
        .map(|coordinates| line_segments_intersect_bbox(&coordinates, bbox))
        .unwrap_or(false)
}

fn polygon_positions_intersect_bbox(polygon: &[Vec<Position>], bbox: BBox) -> bool {
    let rings = polygon
        .iter()
        .map(|ring| positions_to_coordinates(ring))
        .collect::<Result<Vec<_>>>();
    let Ok(rings) = rings else {
        return false;
    };
    if rings.iter().any(|ring| {
        ring.iter()
            .copied()
            .any(|coordinate| bbox.contains(coordinate))
            || line_segments_intersect_bbox(ring, bbox)
    }) {
        return true;
    }
    let Some(exterior) = rings.first() else {
        return false;
    };
    bbox_corners(bbox).iter().copied().any(|corner| {
        point_in_ring(corner, exterior)
            && !rings
                .iter()
                .skip(1)
                .any(|interior| point_in_ring(corner, interior))
    })
}

fn line_segments_intersect_bbox(coordinates: &[Coordinate], bbox: BBox) -> bool {
    coordinates
        .windows(2)
        .any(|segment| segment_intersects_bbox(segment[0], segment[1], bbox))
}

fn segment_intersects_bbox(start: Coordinate, end: Coordinate, bbox: BBox) -> bool {
    if bbox.contains(start) || bbox.contains(end) {
        return true;
    }
    if start.lon.max(end.lon) < bbox.min_lon
        || start.lon.min(end.lon) > bbox.max_lon
        || start.lat.max(end.lat) < bbox.min_lat
        || start.lat.min(end.lat) > bbox.max_lat
    {
        return false;
    }
    let corners = bbox_corners(bbox);
    let edges = [
        (corners[0], corners[1]),
        (corners[1], corners[2]),
        (corners[2], corners[3]),
        (corners[3], corners[0]),
    ];
    edges
        .iter()
        .any(|(edge_start, edge_end)| segments_intersect(start, end, *edge_start, *edge_end))
}

fn bbox_corners(bbox: BBox) -> [Coordinate; 4] {
    [
        Coordinate {
            lon: bbox.min_lon,
            lat: bbox.min_lat,
        },
        Coordinate {
            lon: bbox.max_lon,
            lat: bbox.min_lat,
        },
        Coordinate {
            lon: bbox.max_lon,
            lat: bbox.max_lat,
        },
        Coordinate {
            lon: bbox.min_lon,
            lat: bbox.max_lat,
        },
    ]
}

fn segments_intersect(
    first_start: Coordinate,
    first_end: Coordinate,
    second_start: Coordinate,
    second_end: Coordinate,
) -> bool {
    let o1 = orientation_sign(first_start, first_end, second_start);
    let o2 = orientation_sign(first_start, first_end, second_end);
    let o3 = orientation_sign(second_start, second_end, first_start);
    let o4 = orientation_sign(second_start, second_end, first_end);

    if o1 == 0 && coordinate_on_segment(second_start, first_start, first_end) {
        return true;
    }
    if o2 == 0 && coordinate_on_segment(second_end, first_start, first_end) {
        return true;
    }
    if o3 == 0 && coordinate_on_segment(first_start, second_start, second_end) {
        return true;
    }
    if o4 == 0 && coordinate_on_segment(first_end, second_start, second_end) {
        return true;
    }

    o1 != o2 && o3 != o4
}

fn orientation_sign(a: Coordinate, b: Coordinate, c: Coordinate) -> i8 {
    let orientation = (b.lon - a.lon) * (c.lat - a.lat) - (b.lat - a.lat) * (c.lon - a.lon);
    if orientation.abs() <= GEOMETRY_EPSILON {
        0
    } else if orientation > 0.0 {
        1
    } else {
        -1
    }
}

fn coordinate_on_segment(point: Coordinate, start: Coordinate, end: Coordinate) -> bool {
    orientation_sign(start, end, point) == 0
        && point.lon >= start.lon.min(end.lon) - GEOMETRY_EPSILON
        && point.lon <= start.lon.max(end.lon) + GEOMETRY_EPSILON
        && point.lat >= start.lat.min(end.lat) - GEOMETRY_EPSILON
        && point.lat <= start.lat.max(end.lat) + GEOMETRY_EPSILON
}

/// Applies a coordinate transform to every coordinate in a geometry.
pub fn map_geometry_coordinates<F>(geometry: &Geometry, mut transform: F) -> Result<Geometry>
where
    F: FnMut(Coordinate) -> Result<Coordinate>,
{
    map_geometry_coordinates_inner(geometry, &mut transform)
}

/// Applies a coordinate transform to every coordinate in a feature.
pub fn map_feature_coordinates<F>(feature: &GeoFeature, transform: F) -> Result<GeoFeature>
where
    F: FnMut(Coordinate) -> Result<Coordinate>,
{
    let geometry = match &feature.geometry {
        Some(geometry) => Some(map_geometry_coordinates(geometry, transform)?),
        None => None,
    };

    Ok(GeoFeature {
        id: feature.id.clone(),
        bbox: feature.bbox,
        geometry,
        properties: feature.properties.clone(),
    })
}

/// Translates every coordinate in a geometry by a longitude/x and latitude/y delta.
pub fn translate_geometry(geometry: &Geometry, delta_lon: f64, delta_lat: f64) -> Result<Geometry> {
    if !delta_lon.is_finite() || !delta_lat.is_finite() {
        return Err(invalid_argument("translation delta values must be finite"));
    }
    map_geometry_coordinates(geometry, |coordinate| {
        Coordinate::new(coordinate.lon + delta_lon, coordinate.lat + delta_lat)
    })
}

/// Simplifies lines and rings in a geometry using Douglas-Peucker simplification.
pub fn simplify_geometry(geometry: &Geometry, tolerance: f64) -> Result<Geometry> {
    validate_tolerance(tolerance)?;
    match geometry {
        Geometry::Point { .. } | Geometry::MultiPoint { .. } => Ok(geometry.clone()),
        Geometry::LineString { coordinates } => Ok(Geometry::LineString {
            coordinates: coordinates_to_positions(&simplify_line(
                &positions_to_coordinates(coordinates)?,
                tolerance,
            )?)?,
        }),
        Geometry::MultiLineString { coordinates } => Ok(Geometry::MultiLineString {
            coordinates: coordinates
                .iter()
                .map(|line| {
                    let simplified = simplify_line(&positions_to_coordinates(line)?, tolerance)?;
                    coordinates_to_positions(&simplified)
                })
                .collect::<Result<Vec<_>>>()?,
        }),
        Geometry::Polygon { coordinates } => Ok(Geometry::Polygon {
            coordinates: simplify_polygon_positions(coordinates, tolerance)?,
        }),
        Geometry::MultiPolygon { coordinates } => Ok(Geometry::MultiPolygon {
            coordinates: coordinates
                .iter()
                .map(|polygon| simplify_polygon_positions(polygon, tolerance))
                .collect::<Result<Vec<_>>>()?,
        }),
        Geometry::GeometryCollection { geometries } => Ok(Geometry::GeometryCollection {
            geometries: geometries
                .iter()
                .map(|geometry| simplify_geometry(geometry, tolerance))
                .collect::<Result<Vec<_>>>()?,
        }),
    }
}

/// Simplifies an open line using Douglas-Peucker simplification.
pub fn simplify_line(coordinates: &[Coordinate], tolerance: f64) -> Result<Vec<Coordinate>> {
    validate_tolerance(tolerance)?;
    validate_coordinate_slice(coordinates, 2, "line coordinates")?;

    if coordinates.len() <= 2 || tolerance == 0.0 {
        return Ok(coordinates.to_vec());
    }

    let mut keep = vec![false; coordinates.len()];
    keep[0] = true;
    keep[coordinates.len() - 1] = true;
    simplify_line_range(coordinates, 0, coordinates.len() - 1, tolerance, &mut keep);

    Ok(coordinates
        .iter()
        .copied()
        .zip(keep)
        .filter_map(|(coordinate, keep)| keep.then_some(coordinate))
        .collect())
}

/// Simplifies a closed ring and preserves ring closure.
pub fn simplify_ring(ring: &[Coordinate], tolerance: f64) -> Result<Vec<Coordinate>> {
    validate_tolerance(tolerance)?;
    validate_ring(ring, "ring coordinates")?;

    if ring.len() <= 5 || tolerance == 0.0 {
        return Ok(ring.to_vec());
    }

    let mut open_ring = ring[..ring.len() - 1].to_vec();
    let first = open_ring[0];
    open_ring.push(first);
    let mut simplified = simplify_line(&open_ring, tolerance)?;
    simplified.pop();

    if simplified.len() < 3 {
        return Ok(ring.to_vec());
    }

    if simplified.last().copied() != Some(first) {
        simplified.push(first);
    }
    Ok(simplified)
}

fn validate_positions(coordinates: &[Position], label: &str) -> Result<()> {
    for coordinate in coordinates {
        Coordinate::from_position(*coordinate)
            .map_err(|_| invalid_argument(format!("{label} must be finite")))?;
    }
    Ok(())
}

fn validate_line_positions(coordinates: &[Position], label: &str) -> Result<()> {
    if coordinates.len() < 2 {
        return Err(invalid_argument(format!(
            "{label} must contain at least two positions"
        )));
    }
    validate_positions(coordinates, label)
}

fn validate_polygon_positions(coordinates: &[Vec<Position>], label: &str) -> Result<()> {
    if coordinates.is_empty() {
        return Err(invalid_argument(format!(
            "{label} must contain at least one ring"
        )));
    }
    for ring in coordinates {
        let ring = positions_to_coordinates(ring)?;
        validate_ring(&ring, label)?;
    }
    Ok(())
}

fn validate_coordinate_slice(
    coordinates: &[Coordinate],
    minimum: usize,
    label: &str,
) -> Result<()> {
    if coordinates.len() < minimum {
        return Err(invalid_argument(format!(
            "{label} must contain at least {minimum} positions"
        )));
    }
    for coordinate in coordinates {
        coordinate.validate()?;
    }
    Ok(())
}

fn validate_ring(ring: &[Coordinate], label: &str) -> Result<()> {
    validate_coordinate_slice(ring, 4, label)?;
    if ring.first() != ring.last() {
        return Err(invalid_argument(format!("{label} must be closed")));
    }
    Ok(())
}

fn validate_tolerance(tolerance: f64) -> Result<()> {
    if tolerance < 0.0 || !tolerance.is_finite() {
        return Err(invalid_argument(
            "simplification tolerance must be finite and non-negative",
        ));
    }
    Ok(())
}

fn positions_to_coordinates(positions: &[Position]) -> Result<Vec<Coordinate>> {
    positions
        .iter()
        .copied()
        .map(Coordinate::from_position)
        .collect()
}

fn coordinates_to_positions(coordinates: &[Coordinate]) -> Result<Vec<Position>> {
    coordinates
        .iter()
        .map(|coordinate| {
            coordinate.validate()?;
            Ok(coordinate.as_position())
        })
        .collect()
}

fn point_positions_intersect_bbox(coordinates: &[Position], bbox: BBox) -> bool {
    coordinates
        .iter()
        .copied()
        .filter_map(|position| Coordinate::from_position(position).ok())
        .any(|coordinate| bbox.contains(coordinate))
}

fn positions_intersect_bbox(coordinates: &[Position], bbox: BBox) -> bool {
    let coordinates = coordinates
        .iter()
        .copied()
        .filter_map(|position| Coordinate::from_position(position).ok())
        .collect::<Vec<_>>();
    coordinates
        .iter()
        .copied()
        .any(|coordinate| bbox.contains(coordinate))
        || line_segments_intersect_bbox(&coordinates, bbox)
}

fn map_geometry_coordinates_inner(
    geometry: &Geometry,
    transform: &mut dyn FnMut(Coordinate) -> Result<Coordinate>,
) -> Result<Geometry> {
    match geometry {
        Geometry::Point { coordinates } => Ok(Geometry::Point {
            coordinates: transform(Coordinate::from_position(*coordinates)?)?.as_position(),
        }),
        Geometry::MultiPoint { coordinates } => Ok(Geometry::MultiPoint {
            coordinates: map_positions(coordinates, transform)?,
        }),
        Geometry::LineString { coordinates } => Ok(Geometry::LineString {
            coordinates: map_positions(coordinates, transform)?,
        }),
        Geometry::MultiLineString { coordinates } => Ok(Geometry::MultiLineString {
            coordinates: coordinates
                .iter()
                .map(|line| map_positions(line, transform))
                .collect::<Result<Vec<_>>>()?,
        }),
        Geometry::Polygon { coordinates } => Ok(Geometry::Polygon {
            coordinates: coordinates
                .iter()
                .map(|ring| map_positions(ring, transform))
                .collect::<Result<Vec<_>>>()?,
        }),
        Geometry::MultiPolygon { coordinates } => Ok(Geometry::MultiPolygon {
            coordinates: coordinates
                .iter()
                .map(|polygon| {
                    polygon
                        .iter()
                        .map(|ring| map_positions(ring, transform))
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?,
        }),
        Geometry::GeometryCollection { geometries } => Ok(Geometry::GeometryCollection {
            geometries: geometries
                .iter()
                .map(|geometry| map_geometry_coordinates_inner(geometry, transform))
                .collect::<Result<Vec<_>>>()?,
        }),
    }
}

fn map_positions(
    positions: &[Position],
    transform: &mut dyn FnMut(Coordinate) -> Result<Coordinate>,
) -> Result<Vec<Position>> {
    positions
        .iter()
        .copied()
        .map(|position| {
            transform(Coordinate::from_position(position)?).map(Coordinate::as_position)
        })
        .collect()
}

fn simplify_polygon_positions(
    polygon: &[Vec<Position>],
    tolerance: f64,
) -> Result<Vec<Vec<Position>>> {
    polygon
        .iter()
        .map(|ring| {
            let simplified = simplify_ring(&positions_to_coordinates(ring)?, tolerance)?;
            coordinates_to_positions(&simplified)
        })
        .collect()
}

fn simplify_line_range(
    coordinates: &[Coordinate],
    start: usize,
    end: usize,
    tolerance: f64,
    keep: &mut [bool],
) {
    if end <= start + 1 {
        return;
    }

    let mut farthest_index = start + 1;
    let mut farthest_distance = 0.0;
    for index in start + 1..end {
        let distance =
            perpendicular_distance(coordinates[index], coordinates[start], coordinates[end]);
        if distance > farthest_distance {
            farthest_distance = distance;
            farthest_index = index;
        }
    }

    if farthest_distance > tolerance {
        keep[farthest_index] = true;
        simplify_line_range(coordinates, start, farthest_index, tolerance, keep);
        simplify_line_range(coordinates, farthest_index, end, tolerance, keep);
    }
}

fn perpendicular_distance(point: Coordinate, start: Coordinate, end: Coordinate) -> f64 {
    let dx = end.lon - start.lon;
    let dy = end.lat - start.lat;
    if dx == 0.0 && dy == 0.0 {
        return (point.lon - start.lon).hypot(point.lat - start.lat);
    }
    ((dy * point.lon - dx * point.lat + end.lon * start.lat - end.lat * start.lon).abs())
        / dx.hypot(dy)
}

#[derive(Debug, Clone, Copy)]
enum StitchAction {
    AppendForward,
    AppendReverse,
    PrependForward,
    PrependReverse,
}

fn find_connecting_segment(
    ring: &[Coordinate],
    segments: &[Vec<Coordinate>],
) -> Option<(usize, StitchAction)> {
    let first = *ring.first()?;
    let last = *ring.last()?;
    segments.iter().enumerate().find_map(|(index, segment)| {
        let segment_first = *segment.first()?;
        let segment_last = *segment.last()?;
        if last == segment_first {
            Some((index, StitchAction::AppendForward))
        } else if last == segment_last {
            Some((index, StitchAction::AppendReverse))
        } else if first == segment_last {
            Some((index, StitchAction::PrependForward))
        } else if first == segment_first {
            Some((index, StitchAction::PrependReverse))
        } else {
            None
        }
    })
}

fn apply_segment(ring: &mut Vec<Coordinate>, mut segment: Vec<Coordinate>, action: StitchAction) {
    match action {
        StitchAction::AppendForward => ring.extend(segment.into_iter().skip(1)),
        StitchAction::AppendReverse => {
            segment.reverse();
            ring.extend(segment.into_iter().skip(1));
        }
        StitchAction::PrependForward => {
            segment.pop();
            segment.extend(ring.iter().copied());
            *ring = segment;
        }
        StitchAction::PrependReverse => {
            segment.reverse();
            segment.pop();
            segment.extend(ring.iter().copied());
            *ring = segment;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coord(lon: f64, lat: f64) -> Coordinate {
        Coordinate::new(lon, lat).unwrap()
    }

    #[test]
    fn bbox_contains_coordinate() {
        let bbox = BBox::new([8.5, 48.8, 9.3, 49.2]).unwrap();

        assert!(bbox.contains(coord(8.7, 48.9)));
        assert!(!bbox.contains(coord(10.0, 48.9)));
    }

    #[test]
    fn bbox_intersects_crossing_line_segments() {
        let bbox = BBox::new([0.0, 0.0, 1.0, 1.0]).unwrap();
        let geometry = Geometry::LineString {
            coordinates: vec![[-1.0, 0.5], [2.0, 0.5]],
        };

        assert!(bbox.intersects_geometry(&geometry));
    }

    #[test]
    fn segment_intersection_handles_crossing_touching_overlap_and_separated() {
        assert!(segments_intersect(
            coord(0.0, 0.0),
            coord(2.0, 2.0),
            coord(0.0, 2.0),
            coord(2.0, 0.0),
        ));
        assert!(segments_intersect(
            coord(0.0, 0.0),
            coord(1.0, 1.0),
            coord(1.0, 1.0),
            coord(2.0, 0.0),
        ));
        assert!(segments_intersect(
            coord(0.0, 0.0),
            coord(2.0, 0.0),
            coord(1.0, 0.0),
            coord(3.0, 0.0),
        ));
        assert!(!segments_intersect(
            coord(0.0, 0.0),
            coord(1.0, 0.0),
            coord(2.0, 0.0),
            coord(3.0, 0.0),
        ));
    }

    #[test]
    fn coordinate_on_segment_handles_endpoint_interior_and_off_segment() {
        let start = coord(0.0, 0.0);
        let end = coord(2.0, 2.0);

        assert!(coordinate_on_segment(start, start, end));
        assert!(coordinate_on_segment(coord(1.0, 1.0), start, end));
        assert!(!coordinate_on_segment(coord(1.0, 1.1), start, end));
        assert!(!coordinate_on_segment(coord(3.0, 3.0), start, end));
    }

    #[test]
    fn positions_intersect_bbox_detects_crossing_without_inside_vertices() {
        let bbox = BBox::new([0.0, 0.0, 1.0, 1.0]).unwrap();

        assert!(positions_intersect_bbox(&[[-1.0, 0.5], [2.0, 0.5]], bbox));
    }

    #[test]
    fn bbox_intersects_polygon_that_contains_viewport() {
        let bbox = BBox::new([0.0, 0.0, 1.0, 1.0]).unwrap();
        let geometry = Geometry::Polygon {
            coordinates: vec![vec![
                [-1.0, -1.0],
                [2.0, -1.0],
                [2.0, 2.0],
                [-1.0, 2.0],
                [-1.0, -1.0],
            ]],
        };

        assert!(bbox.intersects_geometry(&geometry));
    }

    #[test]
    fn bbox_inside_polygon_hole_does_not_intersect() {
        let bbox = BBox::new([0.25, 0.25, 0.75, 0.75]).unwrap();
        let geometry = Geometry::Polygon {
            coordinates: vec![
                vec![
                    [-1.0, -1.0],
                    [2.0, -1.0],
                    [2.0, 2.0],
                    [-1.0, 2.0],
                    [-1.0, -1.0],
                ],
                vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]],
            ],
        };

        assert!(!bbox.intersects_geometry(&geometry));
    }

    #[test]
    fn normalizes_ring_orientation() {
        let mut ring = vec![
            coord(0.0, 0.0),
            coord(0.0, 1.0),
            coord(1.0, 1.0),
            coord(1.0, 0.0),
            coord(0.0, 0.0),
        ];

        normalize_ring_orientation(&mut ring, true);
        assert!(ring_area(&ring) > 0.0);
        normalize_ring_orientation(&mut ring, false);
        assert!(ring_area(&ring) < 0.0);
    }

    #[test]
    fn point_in_ring_detects_inside_and_outside() {
        let ring = vec![
            coord(0.0, 0.0),
            coord(1.0, 0.0),
            coord(1.0, 1.0),
            coord(0.0, 1.0),
            coord(0.0, 0.0),
        ];

        assert!(point_in_ring(coord(0.5, 0.5), &ring));
        assert!(!point_in_ring(coord(2.0, 0.5), &ring));
    }

    #[test]
    fn stitches_reversed_segments_into_ring() {
        let segments = vec![
            vec![coord(0.0, 0.0), coord(1.0, 0.0), coord(1.0, 1.0)],
            vec![coord(0.0, 0.0), coord(0.0, 1.0), coord(1.0, 1.0)],
        ];

        let rings = stitch_rings(segments).unwrap();

        assert_eq!(rings.len(), 1);
        assert!(is_valid_closed_ring(&rings[0]));
    }

    #[test]
    fn assemble_multipolygon_assigns_inner_ring() {
        let outer = vec![
            coord(0.0, 0.0),
            coord(4.0, 0.0),
            coord(4.0, 4.0),
            coord(0.0, 4.0),
            coord(0.0, 0.0),
        ];
        let inner = vec![
            coord(1.0, 1.0),
            coord(2.0, 1.0),
            coord(2.0, 2.0),
            coord(1.0, 2.0),
            coord(1.0, 1.0),
        ];

        let geometry = assemble_multipolygon(vec![outer], vec![inner]).unwrap();

        let Geometry::Polygon { coordinates } = geometry else {
            panic!("expected polygon");
        };
        assert_eq!(coordinates.len(), 2);
    }

    #[test]
    fn maps_geometry_coordinates() {
        let geometry = point(coord(1.0, 2.0));

        let translated = translate_geometry(&geometry, 3.0, 4.0).unwrap();

        assert_eq!(
            translated,
            Geometry::Point {
                coordinates: [4.0, 6.0]
            }
        );
    }

    #[test]
    fn simplifies_line_coordinates() {
        let line = vec![
            coord(0.0, 0.0),
            coord(1.0, 0.01),
            coord(2.0, -0.01),
            coord(3.0, 0.0),
        ];

        let simplified = simplify_line(&line, 0.1).unwrap();

        assert_eq!(simplified, vec![coord(0.0, 0.0), coord(3.0, 0.0)]);
    }

    #[test]
    fn simplify_ring_preserves_valid_closure() {
        let ring = vec![
            coord(0.0, 0.0),
            coord(1.0, 0.01),
            coord(2.0, 0.0),
            coord(2.0, 2.0),
            coord(0.0, 2.0),
            coord(0.0, 0.0),
        ];

        let simplified = simplify_ring(&ring, 0.1).unwrap();

        assert!(is_valid_closed_ring(&simplified));
        assert_eq!(simplified.first(), simplified.last());
    }

    #[test]
    fn feature_properties_are_generic_json_values() {
        let mut feature = GeoFeature::new(Some(point(coord(8.7, 48.9)))).with_id("node/123");
        feature.insert_property("name", "Test");

        assert_eq!(feature.id.as_deref(), Some("node/123"));
        assert_eq!(feature.properties["name"], Value::from("Test"));
    }
}
