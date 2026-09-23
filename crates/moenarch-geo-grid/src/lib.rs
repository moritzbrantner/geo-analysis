#![doc = include_str!("../README.md")]

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::str::FromStr;

use geo::{
    Coord, Geometry as GeoGeometry, GeometryCollection, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon,
};
use geo_core::{BBox, Coordinate, GeoError, Geometry, Position, Result};
use h3o::{
    geom::{ContainmentMode, PlotterBuilder, SolventBuilder, TilerBuilder},
    CellIndex, LatLng, Resolution,
};
use serde::{Deserialize, Serialize};

const WEB_MERCATOR_MAX_LATITUDE: f64 = 85.051_128_779_806_6;
const MAX_SQUARE_ZOOM: u8 = 24;
const DEFAULT_MAX_CELLS: usize = 250_000;
const MAX_SQUARE_SCAN_CELLS: u64 = 2_000_000;

fn invalid_argument(message: impl Into<String>) -> GeoError {
    GeoError::invalid_argument(message)
}

/// H3 polygon containment mode.
///
/// Points and lines use H3's native point/line indexing and ignore this option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum H3Containment {
    /// Include cells whose centroid is contained by the polygon.
    ContainsCentroid,
    /// Include cells whose complete boundary is contained by the polygon.
    ContainsBoundary,
    /// Include cells whose boundary intersects the polygon boundary.
    IntersectsBoundary,
    /// Include every cell needed to cover the polygon, including polygons fully
    /// contained by a single cell.
    #[default]
    Covers,
}

/// H3 coverage configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct H3CoverageOptions {
    /// H3 resolution in the inclusive range 0..=15.
    pub resolution: u8,
    /// Polygon containment semantics.
    #[serde(default)]
    pub containment: H3Containment,
    /// Maximum number of cells returned.
    #[serde(default = "default_max_cells")]
    pub max_cells: usize,
}

/// Canonical H3 cell coverage.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct H3CellSet {
    /// H3 resolution requested for the coverage.
    pub resolution: u8,
    /// Canonical lower-case H3 cell identifiers.
    pub cells: Vec<String>,
}

/// Hierarchical Web-Mercator square cell.
///
/// Coordinates follow XYZ tile semantics at the parent set's zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SquareCell {
    /// X tile coordinate.
    pub x: u32,
    /// Y tile coordinate.
    pub y: u32,
}

impl SquareCell {
    /// Stable `z/x/y` identifier.
    pub fn id(self, zoom: u8) -> String {
        format!("{zoom}/{}/{}", self.x, self.y)
    }
}

/// Web-Mercator square-cell coverage.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SquareCellSet {
    /// XYZ zoom level.
    pub zoom: u8,
    /// Source-order-independent canonical cell order.
    pub cells: Vec<SquareCell>,
}

/// Square-grid coverage configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SquareCoverageOptions {
    /// Web-Mercator XYZ zoom level in the inclusive range 0..=24.
    pub zoom: u8,
    /// Maximum number of selected cells.
    #[serde(default = "default_max_cells")]
    pub max_cells: usize,
}

/// Converts a `geo-core` geometry into canonical H3 cells.
///
/// Polygon coverage is controlled by `containment`. Points and lines use H3's
/// point and line algorithms. Geometry collections are covered recursively.
pub fn geometry_to_h3_cells(
    geometry: &Geometry,
    options: H3CoverageOptions,
) -> Result<H3CellSet> {
    geometry.validate()?;
    validate_geographic_geometry(geometry)?;
    validate_max_cells(options.max_cells)?;
    let resolution = Resolution::try_from(options.resolution)
        .map_err(|error| invalid_argument(format!("invalid H3 resolution: {error}")))?;
    let geo_geometry = to_geo_geometry(geometry);
    let mut cells = HashSet::new();
    collect_h3_geometry(
        geo_geometry,
        resolution,
        options.containment,
        options.max_cells,
        &mut cells,
    )?;

    let mut cells = cells.into_iter().map(|cell| cell.to_string()).collect::<Vec<_>>();
    cells.sort_unstable();

    Ok(H3CellSet {
        resolution: options.resolution,
        cells,
    })
}

/// Reconstructs the approximate polygonal geometry represented by H3 cells.
///
/// The result describes the selected cells' dissolved outline, not the original
/// geometry that was quantized into those cells.
pub fn h3_cells_to_geometry(cell_set: &H3CellSet) -> Result<Geometry> {
    Resolution::try_from(cell_set.resolution)
        .map_err(|error| invalid_argument(format!("invalid H3 resolution: {error}")))?;

    if cell_set.cells.is_empty() {
        return Ok(Geometry::GeometryCollection {
            geometries: Vec::new(),
        });
    }

    let mut seen = HashSet::with_capacity(cell_set.cells.len());
    let mut cells = Vec::with_capacity(cell_set.cells.len());
    for value in &cell_set.cells {
        let cell = CellIndex::from_str(value)
            .map_err(|error| invalid_argument(format!("invalid H3 cell `{value}`: {error}")))?;
        if cell.resolution() != Resolution::try_from(cell_set.resolution).map_err(|error| {
            invalid_argument(format!("invalid H3 resolution: {error}"))
        })? {
            return Err(invalid_argument(format!(
                "H3 cell `{value}` does not match resolution {}",
                cell_set.resolution
            )));
        }
        if seen.insert(cell) {
            cells.push(cell);
        }
    }

    let multi_polygon = SolventBuilder::new()
        .build()
        .dissolve(cells)
        .map_err(|error| invalid_argument(format!("could not dissolve H3 cells: {error}")))?;
    Ok(from_geo_multi_polygon(&multi_polygon))
}

/// Converts a `geo-core` geometry into intersecting Web-Mercator square cells.
///
/// Point geometry maps to exactly one cell. Lines and areal geometry use a
/// supercover: every square whose geographic cell rectangle intersects the
/// geometry is selected.
pub fn geometry_to_square_cells(
    geometry: &Geometry,
    options: SquareCoverageOptions,
) -> Result<SquareCellSet> {
    geometry.validate()?;
    validate_square_geometry(geometry)?;
    validate_square_zoom(options.zoom)?;
    validate_max_cells(options.max_cells)?;

    let mut cells = BTreeSet::new();
    collect_square_geometry(geometry, options, &mut cells)?;

    Ok(SquareCellSet {
        zoom: options.zoom,
        cells: cells.into_iter().collect(),
    })
}

/// Reconstructs the exact selected square-cell area as an approximate
/// MultiPolygon.
///
/// Adjacent cells are coalesced into maximal row-aligned rectangles before
/// conversion, avoiding one polygon per cell while preserving the selected
/// square coverage exactly.
pub fn square_cells_to_geometry(cell_set: &SquareCellSet) -> Result<Geometry> {
    validate_square_zoom(cell_set.zoom)?;
    if cell_set.cells.is_empty() {
        return Ok(Geometry::GeometryCollection {
            geometries: Vec::new(),
        });
    }

    let dimension = square_dimension(cell_set.zoom);
    let mut rows = BTreeMap::<u32, Vec<u32>>::new();
    let mut unique = BTreeSet::new();
    for cell in &cell_set.cells {
        if cell.x >= dimension || cell.y >= dimension {
            return Err(invalid_argument(format!(
                "square cell {}/{}/{} is outside zoom {}",
                cell_set.zoom, cell.x, cell.y, cell_set.zoom
            )));
        }
        if unique.insert(*cell) {
            rows.entry(cell.y).or_default().push(cell.x);
        }
    }

    let rectangles = merge_square_runs(rows);
    let coordinates = rectangles
        .into_iter()
        .map(|rectangle| vec![square_rectangle_ring(cell_set.zoom, rectangle)])
        .collect::<Vec<_>>();

    let geometry = Geometry::MultiPolygon { coordinates };
    geometry.validate()?;
    Ok(geometry)
}

fn default_max_cells() -> usize {
    DEFAULT_MAX_CELLS
}

fn validate_max_cells(max_cells: usize) -> Result<()> {
    if max_cells == 0 {
        return Err(invalid_argument("maxCells must be greater than zero"));
    }
    Ok(())
}

fn collect_h3_geometry(
    geometry: GeoGeometry<f64>,
    resolution: Resolution,
    containment: H3Containment,
    max_cells: usize,
    output: &mut HashSet<CellIndex>,
) -> Result<()> {
    match geometry {
        GeoGeometry::Point(point) => {
            let cell = LatLng::try_from(point.0)
                .map_err(|error| invalid_argument(format!("invalid H3 point: {error}")))?
                .to_cell(resolution);
            insert_h3_cell(output, cell, max_cells)
        }
        GeoGeometry::MultiPoint(points) => {
            for point in points.0 {
                let cell = LatLng::try_from(point.0)
                    .map_err(|error| invalid_argument(format!("invalid H3 point: {error}")))?
                    .to_cell(resolution);
                insert_h3_cell(output, cell, max_cells)?;
            }
            Ok(())
        }
        GeoGeometry::Line(line) => {
            let mut plotter = PlotterBuilder::new(resolution).build();
            plotter
                .add(line)
                .map_err(|error| invalid_argument(format!("invalid H3 line: {error}")))?;
            for cell in plotter.plot() {
                insert_h3_cell(
                    output,
                    cell.map_err(|error| {
                        invalid_argument(format!("could not plot H3 line: {error}"))
                    })?,
                    max_cells,
                )?;
            }
            Ok(())
        }
        GeoGeometry::LineString(line_string) => {
            let mut plotter = PlotterBuilder::new(resolution).build();
            plotter
                .add_batch(line_string.lines())
                .map_err(|error| invalid_argument(format!("invalid H3 line string: {error}")))?;
            for cell in plotter.plot() {
                insert_h3_cell(
                    output,
                    cell.map_err(|error| {
                        invalid_argument(format!("could not plot H3 line string: {error}"))
                    })?,
                    max_cells,
                )?;
            }
            Ok(())
        }
        GeoGeometry::MultiLineString(lines) => {
            let mut plotter = PlotterBuilder::new(resolution).build();
            for line in lines.0 {
                plotter
                    .add_batch(line.lines())
                    .map_err(|error| invalid_argument(format!("invalid H3 line string: {error}")))?;
            }
            for cell in plotter.plot() {
                insert_h3_cell(
                    output,
                    cell.map_err(|error| {
                        invalid_argument(format!("could not plot H3 line strings: {error}"))
                    })?,
                    max_cells,
                )?;
            }
            Ok(())
        }
        GeoGeometry::Polygon(polygon) => {
            let mut tiler = TilerBuilder::new(resolution)
                .containment_mode(h3_containment(containment))
                .build();
            tiler
                .add(polygon)
                .map_err(|error| invalid_argument(format!("invalid H3 polygon: {error}")))?;
            for cell in tiler.into_coverage() {
                insert_h3_cell(output, cell, max_cells)?;
            }
            Ok(())
        }
        GeoGeometry::MultiPolygon(polygons) => {
            let mut tiler = TilerBuilder::new(resolution)
                .containment_mode(h3_containment(containment))
                .build();
            tiler
                .add_batch(polygons.0)
                .map_err(|error| invalid_argument(format!("invalid H3 multipolygon: {error}")))?;
            for cell in tiler.into_coverage() {
                insert_h3_cell(output, cell, max_cells)?;
            }
            Ok(())
        }
        GeoGeometry::GeometryCollection(collection) => {
            for geometry in collection.0 {
                collect_h3_geometry(
                    geometry,
                    resolution,
                    containment,
                    max_cells,
                    output,
                )?;
            }
            Ok(())
        }
        GeoGeometry::Rect(rectangle) => {
            collect_h3_geometry(
                GeoGeometry::Polygon(rectangle.to_polygon()),
                resolution,
                containment,
                max_cells,
                output,
            )
        }
        GeoGeometry::Triangle(triangle) => collect_h3_geometry(
            GeoGeometry::Polygon(triangle.to_polygon()),
            resolution,
            containment,
            max_cells,
            output,
        ),
    }
}

fn insert_h3_cell(
    cells: &mut HashSet<CellIndex>,
    cell: CellIndex,
    max_cells: usize,
) -> Result<()> {
    cells.insert(cell);
    if cells.len() > max_cells {
        return Err(invalid_argument(format!(
            "H3 coverage exceeds maxCells ({max_cells})"
        )));
    }
    Ok(())
}

fn h3_containment(value: H3Containment) -> ContainmentMode {
    match value {
        H3Containment::ContainsCentroid => ContainmentMode::ContainsCentroid,
        H3Containment::ContainsBoundary => ContainmentMode::ContainsBoundary,
        H3Containment::IntersectsBoundary => ContainmentMode::IntersectsBoundary,
        H3Containment::Covers => ContainmentMode::Covers,
    }
}

fn collect_square_geometry(
    geometry: &Geometry,
    options: SquareCoverageOptions,
    output: &mut BTreeSet<SquareCell>,
) -> Result<()> {
    match geometry {
        Geometry::Point { coordinates } => {
            insert_square_cell(
                output,
                square_cell_for_position(*coordinates, options.zoom)?,
                options.max_cells,
            )
        }
        Geometry::MultiPoint { coordinates } => {
            for position in coordinates {
                insert_square_cell(
                    output,
                    square_cell_for_position(*position, options.zoom)?,
                    options.max_cells,
                )?;
            }
            Ok(())
        }
        Geometry::GeometryCollection { geometries } => {
            for geometry in geometries {
                collect_square_geometry(geometry, options, output)?;
            }
            Ok(())
        }
        _ => scan_square_geometry(geometry, options, output),
    }
}

fn scan_square_geometry(
    geometry: &Geometry,
    options: SquareCoverageOptions,
    output: &mut BTreeSet<SquareCell>,
) -> Result<()> {
    let Some([west, south, east, north]) = geometry_bounds(geometry) else {
        return Ok(());
    };

    let min_x = longitude_to_tile_x(west, options.zoom);
    let max_x = longitude_to_tile_x(east, options.zoom);
    let min_y = latitude_to_tile_y(north, options.zoom);
    let max_y = latitude_to_tile_y(south, options.zoom);

    let candidate_count = u64::from(max_x - min_x + 1)
        .checked_mul(u64::from(max_y - min_y + 1))
        .ok_or_else(|| invalid_argument("square-grid candidate count overflow"))?;
    if candidate_count > MAX_SQUARE_SCAN_CELLS {
        return Err(invalid_argument(format!(
            "square-grid scan would inspect {candidate_count} cells; limit is {MAX_SQUARE_SCAN_CELLS}"
        )));
    }

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let cell = SquareCell { x, y };
            let bounds = square_cell_bounds(options.zoom, cell)?;
            let bbox = BBox::new(bounds)?;
            if bbox.intersects_geometry(geometry) {
                insert_square_cell(output, cell, options.max_cells)?;
            }
        }
    }
    Ok(())
}

fn insert_square_cell(
    cells: &mut BTreeSet<SquareCell>,
    cell: SquareCell,
    max_cells: usize,
) -> Result<()> {
    cells.insert(cell);
    if cells.len() > max_cells {
        return Err(invalid_argument(format!(
            "square coverage exceeds maxCells ({max_cells})"
        )));
    }
    Ok(())
}

fn validate_square_zoom(zoom: u8) -> Result<()> {
    if zoom > MAX_SQUARE_ZOOM {
        return Err(invalid_argument(format!(
            "square zoom must not exceed {MAX_SQUARE_ZOOM}"
        )));
    }
    Ok(())
}

fn square_dimension(zoom: u8) -> u32 {
    1_u32 << zoom
}

fn square_cell_for_position(position: Position, zoom: u8) -> Result<SquareCell> {
    let coordinate = Coordinate::from_position(position)?;
    validate_square_coordinate(coordinate)?;
    Ok(SquareCell {
        x: longitude_to_tile_x(coordinate.lon, zoom),
        y: latitude_to_tile_y(coordinate.lat, zoom),
    })
}

fn longitude_to_tile_x(longitude: f64, zoom: u8) -> u32 {
    let dimension = f64::from(square_dimension(zoom));
    (((longitude + 180.0) / 360.0) * dimension)
        .floor()
        .clamp(0.0, dimension - 1.0) as u32
}

fn latitude_to_tile_y(latitude: f64, zoom: u8) -> u32 {
    let dimension = f64::from(square_dimension(zoom));
    let latitude = latitude.clamp(-WEB_MERCATOR_MAX_LATITUDE, WEB_MERCATOR_MAX_LATITUDE);
    let radians = latitude.to_radians();
    let normalized = (1.0 - radians.tan().asinh() / std::f64::consts::PI) / 2.0;
    (normalized * dimension)
        .floor()
        .clamp(0.0, dimension - 1.0) as u32
}

fn tile_x_to_longitude(x: u32, zoom: u8) -> f64 {
    f64::from(x) / f64::from(square_dimension(zoom)) * 360.0 - 180.0
}

fn tile_y_to_latitude(y: u32, zoom: u8) -> f64 {
    let n = std::f64::consts::PI
        * (1.0 - 2.0 * f64::from(y) / f64::from(square_dimension(zoom)));
    n.sinh().atan().to_degrees()
}

fn square_cell_bounds(zoom: u8, cell: SquareCell) -> Result<[f64; 4]> {
    validate_square_zoom(zoom)?;
    let dimension = square_dimension(zoom);
    if cell.x >= dimension || cell.y >= dimension {
        return Err(invalid_argument(format!(
            "square cell {}/{}/{} is outside zoom {zoom}",
            zoom, cell.x, cell.y
        )));
    }
    Ok([
        tile_x_to_longitude(cell.x, zoom),
        tile_y_to_latitude(cell.y + 1, zoom),
        tile_x_to_longitude(cell.x + 1, zoom),
        tile_y_to_latitude(cell.y, zoom),
    ])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SquareRectangle {
    start_x: u32,
    end_x: u32,
    start_y: u32,
    end_y: u32,
}

fn merge_square_runs(mut rows: BTreeMap<u32, Vec<u32>>) -> Vec<SquareRectangle> {
    let mut active = BTreeMap::<(u32, u32), SquareRectangle>::new();
    let mut rectangles = Vec::new();
    let mut previous_y = None;

    for (y, xs) in &mut rows {
        xs.sort_unstable();
        xs.dedup();
        if previous_y.is_some_and(|previous| *y != previous + 1) {
            rectangles.extend(active.into_values());
            active = BTreeMap::new();
        }

        let mut next = BTreeMap::new();
        for (start_x, end_x) in contiguous_runs(xs) {
            let key = (start_x, end_x);
            let rectangle = active.remove(&key).map_or(
                SquareRectangle {
                    start_x,
                    end_x,
                    start_y: *y,
                    end_y: *y,
                },
                |mut rectangle| {
                    rectangle.end_y = *y;
                    rectangle
                },
            );
            next.insert(key, rectangle);
        }
        rectangles.extend(active.into_values());
        active = next;
        previous_y = Some(*y);
    }

    rectangles.extend(active.into_values());
    rectangles
}

fn contiguous_runs(xs: &[u32]) -> Vec<(u32, u32)> {
    let Some(&first) = xs.first() else {
        return Vec::new();
    };
    let mut runs = Vec::new();
    let mut start = first;
    let mut end = first;
    for &x in xs.iter().skip(1) {
        if x == end + 1 {
            end = x;
        } else {
            runs.push((start, end));
            start = x;
            end = x;
        }
    }
    runs.push((start, end));
    runs
}

fn square_rectangle_ring(zoom: u8, rectangle: SquareRectangle) -> Vec<Position> {
    let west = tile_x_to_longitude(rectangle.start_x, zoom);
    let east = tile_x_to_longitude(rectangle.end_x + 1, zoom);
    let north = tile_y_to_latitude(rectangle.start_y, zoom);
    let south = tile_y_to_latitude(rectangle.end_y + 1, zoom);
    vec![
        [west, north],
        [east, north],
        [east, south],
        [west, south],
        [west, north],
    ]
}

fn validate_geographic_geometry(geometry: &Geometry) -> Result<()> {
    visit_positions(geometry, &mut |position| {
        Coordinate::from_position(position)?.validate_geographic()
    })
}

fn validate_square_geometry(geometry: &Geometry) -> Result<()> {
    visit_positions(geometry, &mut |position| {
        let coordinate = Coordinate::from_position(position)?;
        coordinate.validate_geographic()?;
        validate_square_coordinate(coordinate)
    })
}

fn validate_square_coordinate(coordinate: Coordinate) -> Result<()> {
    if !(-WEB_MERCATOR_MAX_LATITUDE..=WEB_MERCATOR_MAX_LATITUDE).contains(&coordinate.lat) {
        return Err(invalid_argument(format!(
            "square Web-Mercator latitude must stay between -{WEB_MERCATOR_MAX_LATITUDE} and {WEB_MERCATOR_MAX_LATITUDE}"
        )));
    }
    Ok(())
}

fn visit_positions(
    geometry: &Geometry,
    visit: &mut dyn FnMut(Position) -> Result<()>,
) -> Result<()> {
    match geometry {
        Geometry::Point { coordinates } => visit(*coordinates)?,
        Geometry::MultiPoint { coordinates } | Geometry::LineString { coordinates } => {
            for position in coordinates {
                visit(*position)?;
            }
        }
        Geometry::MultiLineString { coordinates } | Geometry::Polygon { coordinates } => {
            for line in coordinates {
                for position in line {
                    visit(*position)?;
                }
            }
        }
        Geometry::MultiPolygon { coordinates } => {
            for polygon in coordinates {
                for ring in polygon {
                    for position in ring {
                        visit(*position)?;
                    }
                }
            }
        }
        Geometry::GeometryCollection { geometries } => {
            for geometry in geometries {
                visit_positions(geometry, visit)?;
            }
        }
    }
    Ok(())
}

fn geometry_bounds(geometry: &Geometry) -> Option<[f64; 4]> {
    let mut bounds: Option<[f64; 4]> = None;
    let _ = visit_positions(geometry, &mut |position| {
        bounds = Some(match bounds {
            None => [position[0], position[1], position[0], position[1]],
            Some([west, south, east, north]) => [
                west.min(position[0]),
                south.min(position[1]),
                east.max(position[0]),
                north.max(position[1]),
            ],
        });
        Ok(())
    });
    bounds
}

fn to_geo_geometry(geometry: &Geometry) -> GeoGeometry<f64> {
    match geometry {
        Geometry::Point { coordinates } => {
            GeoGeometry::Point(Point::new(coordinates[0], coordinates[1]))
        }
        Geometry::MultiPoint { coordinates } => GeoGeometry::MultiPoint(MultiPoint(
            coordinates
                .iter()
                .map(|position| Point::new(position[0], position[1]))
                .collect(),
        )),
        Geometry::LineString { coordinates } => {
            GeoGeometry::LineString(to_geo_line_string(coordinates))
        }
        Geometry::MultiLineString { coordinates } => GeoGeometry::MultiLineString(
            MultiLineString(
                coordinates
                    .iter()
                    .map(|line| to_geo_line_string(line))
                    .collect(),
            ),
        ),
        Geometry::Polygon { coordinates } => {
            GeoGeometry::Polygon(to_geo_polygon(coordinates))
        }
        Geometry::MultiPolygon { coordinates } => GeoGeometry::MultiPolygon(MultiPolygon(
            coordinates
                .iter()
                .map(|polygon| to_geo_polygon(polygon))
                .collect(),
        )),
        Geometry::GeometryCollection { geometries } => {
            GeoGeometry::GeometryCollection(GeometryCollection(
                geometries.iter().map(to_geo_geometry).collect(),
            ))
        }
    }
}

fn to_geo_line_string(coordinates: &[Position]) -> LineString<f64> {
    LineString(
        coordinates
            .iter()
            .map(|position| Coord {
                x: position[0],
                y: position[1],
            })
            .collect(),
    )
}

fn to_geo_polygon(rings: &[Vec<Position>]) -> Polygon<f64> {
    let exterior = rings
        .first()
        .map(|ring| to_geo_line_string(ring))
        .unwrap_or_default();
    let interiors = rings
        .iter()
        .skip(1)
        .map(|ring| to_geo_line_string(ring))
        .collect();
    Polygon::new(exterior, interiors)
}

fn from_geo_multi_polygon(multi_polygon: &MultiPolygon<f64>) -> Geometry {
    Geometry::MultiPolygon {
        coordinates: multi_polygon
            .0
            .iter()
            .map(|polygon| {
                let mut rings = Vec::with_capacity(1 + polygon.interiors().len());
                rings.push(from_geo_line_string(polygon.exterior()));
                rings.extend(
                    polygon
                        .interiors()
                        .iter()
                        .map(from_geo_line_string),
                );
                rings
            })
            .collect(),
    }
}

fn from_geo_line_string(line: &LineString<f64>) -> Vec<Position> {
    line.0.iter().map(|coord| [coord.x, coord.y]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_polygon() -> Geometry {
        Geometry::Polygon {
            coordinates: vec![vec![
                [8.60, 48.75],
                [8.80, 48.75],
                [8.80, 48.95],
                [8.60, 48.95],
                [8.60, 48.75],
            ]],
        }
    }

    #[test]
    fn h3_point_round_trip_produces_one_cell_and_polygon() {
        let source = Geometry::Point {
            coordinates: [8.68, 48.89],
        };
        let cells = geometry_to_h3_cells(
            &source,
            H3CoverageOptions {
                resolution: 8,
                containment: H3Containment::Covers,
                max_cells: 100,
            },
        )
        .expect("H3 point");

        assert_eq!(cells.cells.len(), 1);
        let reconstructed = h3_cells_to_geometry(&cells).expect("H3 geometry");
        assert!(matches!(
            reconstructed,
            Geometry::MultiPolygon { ref coordinates } if !coordinates.is_empty()
        ));
    }

    #[test]
    fn h3_polygon_coverage_is_deterministic_and_round_trips() {
        let options = H3CoverageOptions {
            resolution: 8,
            containment: H3Containment::Covers,
            max_cells: 10_000,
        };
        let first = geometry_to_h3_cells(&square_polygon(), options).expect("first coverage");
        let second = geometry_to_h3_cells(&square_polygon(), options).expect("second coverage");

        assert!(!first.cells.is_empty());
        assert_eq!(first, second);
        assert!(matches!(
            h3_cells_to_geometry(&first).expect("round trip"),
            Geometry::MultiPolygon { .. }
        ));
    }

    #[test]
    fn square_point_maps_to_exactly_one_xyz_cell() {
        let source = Geometry::Point {
            coordinates: [8.68, 48.89],
        };
        let cells = geometry_to_square_cells(
            &source,
            SquareCoverageOptions {
                zoom: 12,
                max_cells: 100,
            },
        )
        .expect("square point");

        assert_eq!(cells.cells.len(), 1);
        assert_eq!(cells.cells[0].id(12).split('/').count(), 3);
    }

    #[test]
    fn square_polygon_supercover_round_trips_to_selected_area() {
        let cells = geometry_to_square_cells(
            &square_polygon(),
            SquareCoverageOptions {
                zoom: 12,
                max_cells: 10_000,
            },
        )
        .expect("square coverage");

        assert!(!cells.cells.is_empty());
        let reconstructed = square_cells_to_geometry(&cells).expect("square geometry");
        assert!(matches!(
            reconstructed,
            Geometry::MultiPolygon { ref coordinates } if !coordinates.is_empty()
        ));
    }

    #[test]
    fn square_reconstruction_merges_full_two_by_two_block() {
        let geometry = square_cells_to_geometry(&SquareCellSet {
            zoom: 4,
            cells: vec![
                SquareCell { x: 3, y: 5 },
                SquareCell { x: 4, y: 5 },
                SquareCell { x: 3, y: 6 },
                SquareCell { x: 4, y: 6 },
            ],
        })
        .expect("square reconstruction");

        let Geometry::MultiPolygon { coordinates } = geometry else {
            panic!("expected multipolygon");
        };
        assert_eq!(coordinates.len(), 1);
        assert_eq!(coordinates[0][0].len(), 5);
    }

    #[test]
    fn empty_cell_sets_round_trip_to_empty_geometry_collection() {
        assert_eq!(
            h3_cells_to_geometry(&H3CellSet {
                resolution: 8,
                cells: Vec::new(),
            })
            .unwrap(),
            Geometry::GeometryCollection {
                geometries: Vec::new()
            }
        );
        assert_eq!(
            square_cells_to_geometry(&SquareCellSet {
                zoom: 8,
                cells: Vec::new(),
            })
            .unwrap(),
            Geometry::GeometryCollection {
                geometries: Vec::new()
            }
        );
    }

    #[test]
    fn rejects_invalid_limits_and_square_latitudes() {
        let point = Geometry::Point {
            coordinates: [0.0, 89.0],
        };
        assert!(geometry_to_square_cells(
            &point,
            SquareCoverageOptions {
                zoom: 12,
                max_cells: 10,
            },
        )
        .is_err());
        assert!(geometry_to_h3_cells(
            &Geometry::Point {
                coordinates: [0.0, 0.0],
            },
            H3CoverageOptions {
                resolution: 16,
                containment: H3Containment::Covers,
                max_cells: 10,
            },
        )
        .is_err());
    }
}
