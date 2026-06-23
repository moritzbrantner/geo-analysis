# geo-viz

Renderer-agnostic geographic visualization indexes for map views.

This crate owns the data-side map aggregation surface used by
`@moenarch/viz-engine`. Rendering remains in downstream packages.

## Install

```toml
[dependencies]
geo-core = { package = "moenarch-geo-core", version = "0.1.0" }
geo-io-geojson = { package = "moenarch-geo-io-geojson", version = "0.1.0" }
geo-clustering = { package = "moenarch-geo-clustering", version = "0.1.0" }
maps-kernels-core = { package = "moenarch-maps-kernels-core", version = "0.1.0" }
geo-viz = { package = "moenarch-geo-viz", version = "0.1.0" }
```

```rust,ignore
use geo_viz::{GeoPointIndex, GeoVizPoint};
```

## Package surface

Primary workflow: `geoViz.aggregateViewport`.

Workflow operations:

- `geoViz.bounds`: Computes geographic bounds for finite lon/lat points.
- `geoViz.aggregateViewport`: Clusters points and returns renderer-agnostic viewport features.
- `geoViz.heatViewport`: Returns weighted heat features for visible geographic points.
- `geoViz.geoJsonViewport`: Filters GeoJSON features for a viewport and returns a FeatureCollection.
- `geoViz.flowViewport`: Filters and optionally aggregates geographic flows for a viewport.
- `geoViz.resampleGeometry`: Resamples flat 2D coordinates as an open line or closed ring.
- `geoViz.scalarFieldGrid`: Creates an IDW scalar field grid for geographic value points.

Debug operations:

- `describe`: inspect package metadata and runtime support.

Runtime support: library, CLI, server, and WASM wrappers expose these operations.

Run the primary workflow through the CLI:

```bash
cargo run -p moenarch-geo-viz-cli -- run \
  --operation geoViz.aggregateViewport \
  --json '{"points":[{"id":"a","latitude":49.0,"longitude":8.0}],"query":{"bounds":[7.0,48.0,9.0,50.0],"zoom":8}}'
```

Successful responses use the shared `SurfaceResponse` envelope with an
operation id, JSON value, diagnostics, and artifacts. Default surface calls are
deterministic, local-first, and do not download models, write persistent files,
or execute external tools.
