#![doc = include_str!("../README.md")]

mod flow_index;
mod geojson_index;
mod metrics;
mod point_index;
mod scalar_field;
pub mod surface;
mod viewport;

use geo_core::GeoError;

pub use flow_index::{
    GeoFlowIndex, GeoVizFlow, GeoVizFlowAggregateMode, GeoVizFlowAggregation, GeoVizFlowFeature,
    GeoVizFlowOptions, GeoVizFlowSummary, GeoVizIndexedFlow,
};
pub use geojson_index::{
    GeoJsonIndex, GeoVizFeatureCollectionValue, GeoVizGeoJsonOptions, GeoVizGeoJsonViewport,
};
pub use metrics::GeoVizMetricRecord;
pub use point_index::{
    GeoPointIndex, GeoVizAggregation, GeoVizAggregationFeature, GeoVizAggregationOptions,
    GeoVizAggregationSummary, GeoVizHeatAggregation, GeoVizHeatFeature, GeoVizHeatOptions,
    GeoVizHeatSummary, GeoVizIndexedPoint, GeoVizNearestPointQuery, GeoVizPoint,
};
pub use scalar_field::{
    create_scalar_field_grid, GeoVizScalarFieldGrid, GeoVizScalarFieldIndex,
    GeoVizScalarFieldOptions,
};
pub use viewport::{GeoVizBounds, GeoVizViewportQuery};

fn invalid_argument(message: impl Into<String>) -> GeoError {
    GeoError::invalid_argument(message)
}
