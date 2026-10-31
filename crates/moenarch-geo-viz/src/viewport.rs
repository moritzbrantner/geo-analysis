use geo_core::Result;
use serde::{Deserialize, Serialize};

use crate::invalid_argument;

/// Geographic bounding box in `[west, south, east, north]` order.
pub type GeoVizBounds = [f64; 4];

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// Query for a geographic viewport.
pub struct GeoVizViewportQuery {
    /// Viewport bounds in `[west, south, east, north]` order.
    pub bounds: GeoVizBounds,
    /// Map zoom level.
    pub zoom: f64,
}

pub(crate) fn validate_viewport_query(query: GeoVizViewportQuery) -> Result<()> {
    validate_bounds(query.bounds)?;
    if !query.zoom.is_finite() {
        return Err(invalid_argument("viewport zoom must be finite"));
    }
    Ok(())
}

fn validate_bounds(bounds: GeoVizBounds) -> Result<()> {
    if bounds.iter().any(|value| !value.is_finite()) {
        return Err(invalid_argument("viewport bounds must be finite"));
    }
    if bounds[1] > bounds[3] {
        return Err(invalid_argument("viewport south must be <= north"));
    }
    if bounds[0] < -180.0 || bounds[0] > 180.0 || bounds[2] < -180.0 || bounds[2] > 180.0 {
        return Err(invalid_argument(
            "viewport longitude bounds must stay between -180 and 180",
        ));
    }
    if bounds[1] < -90.0 || bounds[3] > 90.0 {
        return Err(invalid_argument(
            "viewport latitude bounds must stay between -90 and 90",
        ));
    }
    Ok(())
}
