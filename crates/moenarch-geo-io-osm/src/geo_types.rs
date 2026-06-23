//! Optional conversions to GeoRust `geo-types`.

use geo_core::Coordinate;

/// Converts a `geo-core` coordinate into a `geo-types` coordinate.
pub fn to_geo_types_coord(coordinate: Coordinate) -> geo_types::Coord<f64> {
    geo_types::Coord {
        x: coordinate.lon,
        y: coordinate.lat,
    }
}
