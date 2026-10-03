#![doc = include_str!("../README.md")]

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::str::FromStr;

use geo::{
    Coord, Geometry as GeoGeometry, GeometryCollection, Intersects, Line, LineString,
    MultiLineString, MultiPoint, MultiPolygon, Point, Polygon, Rect,
};
use geo_core::{Coordinate, GeoError, Geometry, Position, Result};
use h3o::{
    geom::{ContainmentMode, PlotterBuilder, SolventBuilder, TilerBuilder},
    CellIndex, LatLng, Resolution,
};
use serde::{Deserialize, Serialize};

const WEB_MERCATOR_MAX_LATITUDE: f64 = 85.051_128_779_806_6;
const MAX_SQUARE_ZOOM: u8 = 24;
/// Default output budget for discrete-grid conversions.
pub const DEFAULT_CELL_BUDGET: usize = 250_000;
/// Hard ceiling for caller-supplied `maxCells`; protects public surfaces from
/// budgets large enough to disable the work limits.
pub const MAX_CELL_BUDGET: usize = 1_000_000;
/// Request-wide square coverage work limit, in O(1) steps: segment/cell
/// intersection tests plus row/edge crossing evaluations.
const MAX_SQUARE_SCAN_WORK: u64 = 8_000_000;
/// Maximum number of generated H3 cells (duplicates included) per allowed
/// output cell. Bounds the work of collections that repeat the same geometry.
const H3_WORK_FACTOR: usize = 4;
/// Distance, in tile units, within which a coordinate counts as lying on a
/// square-grid tile edge. Absorbs Mercator forward/inverse round-off, which
/// grows with zoom (about 1e-8 tiles at zoom 24). Erring wide only adds a
/// neighbouring candidate; the exact intersection test still decides.
const SQUARE_EDGE_TOLERANCE: f64 = 1e-6;

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
pub fn geometry_to_h3_cells(geometry: &Geometry, options: H3CoverageOptions) -> Result<H3CellSet> {
    geometry.validate()?;
    validate_geographic_geometry(geometry)?;
    validate_max_cells(options.max_cells)?;
    let resolution = Resolution::try_from(options.resolution)
        .map_err(|error| invalid_argument(format!("invalid H3 resolution: {error}")))?;
    let geo_geometry = to_geo_geometry(geometry);
    let mut cells = HashSet::new();
    let mut budget = H3Budget::new(options.max_cells);
    collect_h3_geometry(
        geo_geometry,
        resolution,
        options.containment,
        &mut budget,
        &mut cells,
    )?;

    let mut cells = cells
        .into_iter()
        .map(|cell| cell.to_string())
        .collect::<Vec<_>>();
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
        if cell.resolution()
            != Resolution::try_from(cell_set.resolution)
                .map_err(|error| invalid_argument(format!("invalid H3 resolution: {error}")))?
        {
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
    let mut scan_budget = MAX_SQUARE_SCAN_WORK;
    collect_square_geometry(geometry, options, &mut scan_budget, &mut cells)?;

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
    DEFAULT_CELL_BUDGET
}

fn validate_max_cells(max_cells: usize) -> Result<()> {
    if max_cells == 0 {
        return Err(invalid_argument("maxCells must be greater than zero"));
    }
    if max_cells > MAX_CELL_BUDGET {
        return Err(invalid_argument(format!(
            "maxCells must not exceed {MAX_CELL_BUDGET}"
        )));
    }
    Ok(())
}

fn collect_h3_geometry(
    geometry: GeoGeometry<f64>,
    resolution: Resolution,
    containment: H3Containment,
    budget: &mut H3Budget,
    output: &mut HashSet<CellIndex>,
) -> Result<()> {
    match geometry {
        GeoGeometry::Point(point) => {
            let cell = LatLng::try_from(point.0)
                .map_err(|error| invalid_argument(format!("invalid H3 point: {error}")))?
                .to_cell(resolution);
            insert_h3_cell(output, cell, budget)
        }
        GeoGeometry::MultiPoint(points) => {
            for point in points.0 {
                let cell = LatLng::try_from(point.0)
                    .map_err(|error| invalid_argument(format!("invalid H3 point: {error}")))?
                    .to_cell(resolution);
                insert_h3_cell(output, cell, budget)?;
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
                    budget,
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
                    budget,
                )?;
            }
            Ok(())
        }
        GeoGeometry::MultiLineString(lines) => {
            let mut plotter = PlotterBuilder::new(resolution).build();
            for line in lines.0 {
                plotter.add_batch(line.lines()).map_err(|error| {
                    invalid_argument(format!("invalid H3 line string: {error}"))
                })?;
            }
            for cell in plotter.plot() {
                insert_h3_cell(
                    output,
                    cell.map_err(|error| {
                        invalid_argument(format!("could not plot H3 line strings: {error}"))
                    })?,
                    budget,
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
                insert_h3_cell(output, cell, budget)?;
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
                insert_h3_cell(output, cell, budget)?;
            }
            Ok(())
        }
        GeoGeometry::GeometryCollection(collection) => {
            for geometry in collection.0 {
                collect_h3_geometry(geometry, resolution, containment, budget, output)?;
            }
            Ok(())
        }
        GeoGeometry::Rect(rectangle) => collect_h3_geometry(
            GeoGeometry::Polygon(rectangle.to_polygon()),
            resolution,
            containment,
            budget,
            output,
        ),
        GeoGeometry::Triangle(triangle) => collect_h3_geometry(
            GeoGeometry::Polygon(triangle.to_polygon()),
            resolution,
            containment,
            budget,
            output,
        ),
    }
}

/// Request-wide H3 limits: unique output cells plus total generated cells,
/// shared by every member of a geometry collection.
struct H3Budget {
    max_cells: usize,
    remaining_work: usize,
}

impl H3Budget {
    fn new(max_cells: usize) -> Self {
        Self {
            max_cells,
            remaining_work: max_cells.saturating_mul(H3_WORK_FACTOR),
        }
    }
}

fn insert_h3_cell(
    cells: &mut HashSet<CellIndex>,
    cell: CellIndex,
    budget: &mut H3Budget,
) -> Result<()> {
    let Some(remaining_work) = budget.remaining_work.checked_sub(1) else {
        return Err(invalid_argument(format!(
            "H3 coverage generates more than {H3_WORK_FACTOR}x maxCells ({}) cells",
            budget.max_cells
        )));
    };
    budget.remaining_work = remaining_work;
    cells.insert(cell);
    if cells.len() > budget.max_cells {
        return Err(invalid_argument(format!(
            "H3 coverage exceeds maxCells ({})",
            budget.max_cells
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
    scan_budget: &mut u64,
    output: &mut BTreeSet<SquareCell>,
) -> Result<()> {
    match geometry {
        Geometry::Point { coordinates } => insert_square_cell(
            output,
            square_cell_for_position(*coordinates, options.zoom)?,
            options.max_cells,
        ),
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
                collect_square_geometry(geometry, options, scan_budget, output)?;
            }
            Ok(())
        }
        Geometry::LineString { coordinates } => scan_square_paths(
            std::slice::from_ref(coordinates),
            false,
            options,
            scan_budget,
            output,
        ),
        Geometry::MultiLineString { coordinates } => {
            scan_square_paths(coordinates, false, options, scan_budget, output)
        }
        Geometry::Polygon { coordinates } => {
            scan_square_polygon(coordinates, options, scan_budget, output)
        }
        // Polygons are scanned one by one so distant parts never share one
        // aggregate row range.
        Geometry::MultiPolygon { coordinates } => {
            for polygon in coordinates {
                scan_square_polygon(polygon, options, scan_budget, output)?;
            }
            Ok(())
        }
    }
}

/// Charges `work` unit steps against the request-wide square scan budget.
fn charge_square_scan(scan_budget: &mut u64, work: u64) -> Result<()> {
    if work > *scan_budget {
        return Err(invalid_argument(format!(
            "square-grid coverage would exceed the request scan limit of {MAX_SQUARE_SCAN_WORK} steps"
        )));
    }
    *scan_budget -= work;
    Ok(())
}

/// Iterates a ring or line's segments; a single position is a degenerate
/// segment and rings are implicitly closed.
fn path_segments(
    path: &[Position],
    close: bool,
) -> impl Iterator<Item = (Position, Position)> + '_ {
    let single = (path.len() == 1).then(|| (path[0], path[0]));
    let closing = match (close, path.first(), path.last()) {
        (true, Some(first), Some(last)) if path.len() > 1 && first != last => Some((*last, *first)),
        _ => None,
    };
    path.windows(2)
        .map(|pair| (pair[0], pair[1]))
        .chain(single)
        .chain(closing)
}

/// Selects every cell whose closed rectangle touches one of the segments.
///
/// Candidates are limited to each segment's own tile range, so the work is
/// proportional to the cells near the boundary rather than to
/// `bbox cells x vertex count`. `scan_budget` is shared across a whole request
/// (including every member of a `GeometryCollection`).
fn scan_square_paths(
    paths: &[Vec<Position>],
    close: bool,
    options: SquareCoverageOptions,
    scan_budget: &mut u64,
    output: &mut BTreeSet<SquareCell>,
) -> Result<()> {
    let zoom = options.zoom;
    for path in paths {
        for (start, end) in path_segments(path, close) {
            // Cells are closed rectangles, so a bound lying on a tile edge
            // touches the cells on both sides of that edge.
            let min_x =
                closed_min_tile(longitude_tile_coordinate(start[0].min(end[0]), zoom), zoom);
            let max_x =
                closed_max_tile(longitude_tile_coordinate(start[0].max(end[0]), zoom), zoom);
            let min_y = closed_min_tile(latitude_tile_coordinate(start[1].max(end[1]), zoom), zoom);
            let max_y = closed_max_tile(latitude_tile_coordinate(start[1].min(end[1]), zoom), zoom);
            let candidates = u64::from(max_x - min_x + 1) * u64::from(max_y - min_y + 1);
            charge_square_scan(scan_budget, candidates)?;

            let segment = Line::new(
                Coord {
                    x: start[0],
                    y: start[1],
                },
                Coord {
                    x: end[0],
                    y: end[1],
                },
            );
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let cell = SquareCell { x, y };
                    let [west, south, east, north] = square_cell_bounds(zoom, cell)?;
                    let rectangle =
                        Rect::new(Coord { x: west, y: south }, Coord { x: east, y: north });
                    if rectangle.intersects(&segment) {
                        insert_square_cell(output, cell, options.max_cells)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Selects a polygon's boundary cells plus the cells strictly inside it.
///
/// A cell not touched by any ring is either fully inside or fully outside the
/// polygon, so its centre decides. Each row is filled from the even-odd
/// crossings of the row's centre latitude with all rings, costing
/// `rows x edges` scan steps.
fn scan_square_polygon(
    rings: &[Vec<Position>],
    options: SquareCoverageOptions,
    scan_budget: &mut u64,
    output: &mut BTreeSet<SquareCell>,
) -> Result<()> {
    scan_square_paths(rings, true, options, scan_budget, output)?;

    let Some(exterior) = rings.first().filter(|ring| !ring.is_empty()) else {
        return Ok(());
    };
    let zoom = options.zoom;
    let (south, north) = exterior.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(south, north), position| (south.min(position[1]), north.max(position[1])),
    );
    let min_y = closed_min_tile(latitude_tile_coordinate(north, zoom), zoom);
    let max_y = closed_max_tile(latitude_tile_coordinate(south, zoom), zoom);
    let edges = rings
        .iter()
        .map(|ring| path_segments(ring, true).count() as u64)
        .sum::<u64>()
        .max(1);
    charge_square_scan(
        scan_budget,
        u64::from(max_y - min_y + 1).saturating_mul(edges),
    )?;

    let last_column = square_dimension(zoom) - 1;
    let mut crossings = Vec::new();
    for y in min_y..=max_y {
        let center = (tile_y_to_latitude(y, zoom) + tile_y_to_latitude(y + 1, zoom)) / 2.0;
        crossings.clear();
        for ring in rings {
            for (start, end) in path_segments(ring, true) {
                if (start[1] > center) != (end[1] > center) {
                    crossings.push(
                        start[0] + (center - start[1]) * (end[0] - start[0]) / (end[1] - start[1]),
                    );
                }
            }
        }
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.chunks_exact(2) {
            // Cells whose centre longitude lies inside this crossing interval.
            let first = (longitude_tile_coordinate(pair[0], zoom) - 0.5)
                .ceil()
                .max(0.0);
            let last = (longitude_tile_coordinate(pair[1], zoom) - 0.5)
                .floor()
                .min(f64::from(last_column));
            if first > last {
                continue;
            }
            for x in first as u32..=last as u32 {
                insert_square_cell(output, SquareCell { x, y }, options.max_cells)?;
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

fn longitude_tile_coordinate(longitude: f64, zoom: u8) -> f64 {
    ((longitude + 180.0) / 360.0) * f64::from(square_dimension(zoom))
}

fn latitude_tile_coordinate(latitude: f64, zoom: u8) -> f64 {
    let latitude = latitude.clamp(-WEB_MERCATOR_MAX_LATITUDE, WEB_MERCATOR_MAX_LATITUDE);
    let radians = latitude.to_radians();
    let normalized = (1.0 - radians.tan().asinh() / std::f64::consts::PI) / 2.0;
    normalized * f64::from(square_dimension(zoom))
}

fn tile_index(coordinate: f64, zoom: u8) -> u32 {
    let dimension = f64::from(square_dimension(zoom));
    coordinate.floor().clamp(0.0, dimension - 1.0) as u32
}

/// Nearest tile edge when `coordinate` lies on one within tolerance.
fn tile_edge(coordinate: f64) -> Option<f64> {
    let edge = coordinate.round();
    ((coordinate - edge).abs() <= SQUARE_EDGE_TOLERANCE).then_some(edge)
}

/// Lowest tile index whose closed extent touches `coordinate`.
fn closed_min_tile(coordinate: f64, zoom: u8) -> u32 {
    match tile_edge(coordinate) {
        Some(edge) => tile_index(edge - 1.0, zoom),
        None => tile_index(coordinate, zoom),
    }
}

/// Highest tile index whose closed extent touches `coordinate`.
fn closed_max_tile(coordinate: f64, zoom: u8) -> u32 {
    tile_index(tile_edge(coordinate).unwrap_or(coordinate), zoom)
}

fn longitude_to_tile_x(longitude: f64, zoom: u8) -> u32 {
    tile_index(longitude_tile_coordinate(longitude, zoom), zoom)
}

fn latitude_to_tile_y(latitude: f64, zoom: u8) -> u32 {
    tile_index(latitude_tile_coordinate(latitude, zoom), zoom)
}

fn tile_x_to_longitude(x: u32, zoom: u8) -> f64 {
    f64::from(x) / f64::from(square_dimension(zoom)) * 360.0 - 180.0
}

fn tile_y_to_latitude(y: u32, zoom: u8) -> f64 {
    let n = std::f64::consts::PI * (1.0 - 2.0 * f64::from(y) / f64::from(square_dimension(zoom)));
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
        Geometry::MultiLineString { coordinates } => GeoGeometry::MultiLineString(MultiLineString(
            coordinates
                .iter()
                .map(|line| to_geo_line_string(line))
                .collect(),
        )),
        Geometry::Polygon { coordinates } => GeoGeometry::Polygon(to_geo_polygon(coordinates)),
        Geometry::MultiPolygon { coordinates } => GeoGeometry::MultiPolygon(MultiPolygon(
            coordinates
                .iter()
                .map(|polygon| to_geo_polygon(polygon))
                .collect(),
        )),
        Geometry::GeometryCollection { geometries } => GeoGeometry::GeometryCollection(
            GeometryCollection(geometries.iter().map(to_geo_geometry).collect()),
        ),
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
        .unwrap_or_else(|| LineString(Vec::new()));
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
                rings.extend(polygon.interiors().iter().map(from_geo_line_string));
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
    fn square_supercover_includes_cells_on_both_sides_of_grid_edges() {
        let options = SquareCoverageOptions {
            zoom: 1,
            max_cells: 100,
        };
        let vertical = geometry_to_square_cells(
            &Geometry::LineString {
                coordinates: vec![[0.0, 10.0], [0.0, 20.0]],
            },
            options,
        )
        .expect("vertical line on the prime-meridian tile edge");
        assert_eq!(
            vertical.cells,
            vec![SquareCell { x: 0, y: 0 }, SquareCell { x: 1, y: 0 }]
        );

        let horizontal = geometry_to_square_cells(
            &Geometry::LineString {
                coordinates: vec![[10.0, 0.0], [20.0, 0.0]],
            },
            options,
        )
        .expect("horizontal line on the equator tile edge");
        assert_eq!(
            horizontal.cells,
            vec![SquareCell { x: 1, y: 0 }, SquareCell { x: 1, y: 1 }]
        );
    }

    #[test]
    fn square_scan_budget_is_shared_across_collection_members() {
        let line = Geometry::LineString {
            coordinates: vec![[8.60, 48.75], [8.80, 48.95]],
        };
        let options = SquareCoverageOptions {
            zoom: 12,
            max_cells: 10_000,
        };

        let mut single_budget = u64::MAX;
        let mut cells = BTreeSet::new();
        collect_square_geometry(&line, options, &mut single_budget, &mut cells)
            .expect("single line scan");
        let per_line_cost = u64::MAX - single_budget;
        assert!(per_line_cost > 0);

        let collection = Geometry::GeometryCollection {
            geometries: vec![line.clone(), line.clone(), line],
        };
        let mut budget = per_line_cost * 2;
        let mut cells = BTreeSet::new();
        assert!(collect_square_geometry(&collection, options, &mut budget, &mut cells).is_err());
    }

    #[test]
    fn square_supercover_tolerates_mercator_round_off_on_tile_edges() {
        // Standard zoom-2 row boundary; maps to ~0.9999999999999998 rows.
        let edge_latitude = tile_y_to_latitude(1, 2);
        let cells = geometry_to_square_cells(
            &Geometry::LineString {
                coordinates: vec![[10.0, edge_latitude], [20.0, edge_latitude]],
            },
            SquareCoverageOptions {
                zoom: 2,
                max_cells: 100,
            },
        )
        .expect("horizontal line on a Mercator row edge");
        assert_eq!(
            cells.cells,
            vec![SquareCell { x: 2, y: 0 }, SquareCell { x: 2, y: 1 }]
        );
    }

    #[test]
    fn square_multipart_geometry_scans_components_separately() {
        let tiny = |lon: f64, lat: f64| {
            vec![vec![
                [lon, lat],
                [lon + 0.001, lat],
                [lon + 0.001, lat + 0.001],
                [lon, lat + 0.001],
                [lon, lat],
            ]]
        };
        let cells = geometry_to_square_cells(
            &Geometry::MultiPolygon {
                coordinates: vec![tiny(-179.99, -80.0), tiny(179.98, 80.0)],
            },
            SquareCoverageOptions {
                zoom: 12,
                max_cells: 100,
            },
        )
        .expect("distant multipolygon parts");
        assert!(!cells.cells.is_empty() && cells.cells.len() <= 8);
    }

    #[test]
    fn h3_work_budget_is_shared_across_collection_members() {
        let options = H3CoverageOptions {
            resolution: 8,
            containment: H3Containment::Covers,
            max_cells: 10_000,
        };
        let single = geometry_to_h3_cells(&square_polygon(), options).expect("single coverage");
        let options = H3CoverageOptions {
            max_cells: single.cells.len(),
            ..options
        };
        let repeated = |copies: usize| Geometry::GeometryCollection {
            geometries: vec![square_polygon(); copies],
        };

        assert_eq!(
            geometry_to_h3_cells(&repeated(2), options).expect("two copies"),
            single
        );
        assert!(geometry_to_h3_cells(&repeated(H3_WORK_FACTOR + 1), options).is_err());
    }

    /// Reference supercover: tests every cell of the bounding tile range.
    fn brute_force_square_cells(geometry: &Geometry, zoom: u8) -> Vec<SquareCell> {
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        visit_positions(geometry, &mut |position| {
            bounds = [
                bounds[0].min(position[0]),
                bounds[1].min(position[1]),
                bounds[2].max(position[0]),
                bounds[3].max(position[1]),
            ];
            Ok(())
        })
        .unwrap();
        let min_x = closed_min_tile(longitude_tile_coordinate(bounds[0], zoom), zoom);
        let max_x = closed_max_tile(longitude_tile_coordinate(bounds[2], zoom), zoom);
        let min_y = closed_min_tile(latitude_tile_coordinate(bounds[3], zoom), zoom);
        let max_y = closed_max_tile(latitude_tile_coordinate(bounds[1], zoom), zoom);
        let mut cells = Vec::new();
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let cell = SquareCell { x, y };
                let bbox = geo_core::BBox::new(square_cell_bounds(zoom, cell).unwrap()).unwrap();
                if bbox.intersects_geometry(geometry) {
                    cells.push(cell);
                }
            }
        }
        cells.sort();
        cells
    }

    #[test]
    fn square_coverage_matches_brute_force_supercover() {
        let polygon_with_hole = Geometry::Polygon {
            coordinates: vec![
                vec![
                    [8.60, 48.75],
                    [8.95, 48.70],
                    [8.80, 49.05],
                    [8.62, 48.95],
                    [8.60, 48.75],
                ],
                vec![
                    [8.70, 48.82],
                    [8.78, 48.82],
                    [8.78, 48.90],
                    [8.70, 48.90],
                    [8.70, 48.82],
                ],
            ],
        };
        let zigzag = Geometry::LineString {
            coordinates: (0..40)
                .map(|step| {
                    let step = f64::from(step);
                    [8.5 + step * 0.01, 48.8 + (step * 0.7).sin() * 0.05]
                })
                .collect(),
        };
        for geometry in [square_polygon(), polygon_with_hole, zigzag] {
            for zoom in [10, 13] {
                let cells = geometry_to_square_cells(
                    &geometry,
                    SquareCoverageOptions {
                        zoom,
                        max_cells: 100_000,
                    },
                )
                .expect("square coverage");
                assert_eq!(cells.cells, brute_force_square_cells(&geometry, zoom));
            }
        }
    }

    #[test]
    fn square_scan_cost_does_not_scale_with_bbox_times_vertices() {
        // 1,000-segment diagonal across a ~1,000 x 1,000 tile box at zoom 12.
        let line = Geometry::LineString {
            coordinates: (0..=1_000)
                .map(|step| {
                    let step = f64::from(step);
                    [step * 0.0879, step * 0.0679]
                })
                .collect(),
        };
        let cells = geometry_to_square_cells(
            &line,
            SquareCoverageOptions {
                zoom: 12,
                max_cells: 100_000,
            },
        )
        .expect("long diagonal stays within the scan budget");
        assert!(cells.cells.len() >= 1_000 && cells.cells.len() < 5_000);
    }

    #[test]
    fn square_supercover_tolerates_round_off_at_maximum_zoom() {
        let row = 16_545_366;
        let edge_latitude = tile_y_to_latitude(row, MAX_SQUARE_ZOOM);
        let cells = geometry_to_square_cells(
            &Geometry::LineString {
                coordinates: vec![[10.0, edge_latitude], [10.000_001, edge_latitude]],
            },
            SquareCoverageOptions {
                zoom: MAX_SQUARE_ZOOM,
                max_cells: 100,
            },
        )
        .expect("horizontal line on a zoom-24 row edge");
        let rows = cells
            .cells
            .iter()
            .map(|cell| cell.y)
            .collect::<BTreeSet<_>>();
        assert_eq!(rows, BTreeSet::from([row - 1, row]));
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
                resolution: 8,
                containment: H3Containment::Covers,
                max_cells: MAX_CELL_BUDGET + 1,
            },
        )
        .is_err());
        assert!(geometry_to_square_cells(
            &Geometry::Point {
                coordinates: [0.0, 0.0],
            },
            SquareCoverageOptions {
                zoom: 12,
                max_cells: usize::MAX,
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
