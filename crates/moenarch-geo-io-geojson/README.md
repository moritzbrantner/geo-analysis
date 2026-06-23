# geo-io-geojson

GeoJSON import and export adapters for `geo-core`.

This crate owns the `geojson` dependency. Algorithm and domain crates should
depend on `geo-core` types instead of exposing `geojson` wire-format types.

## Install

```toml
[dependencies]
geo-core = { package = "moenarch-geo-core", version = "0.1.0" }
geo-io-geojson = { package = "moenarch-geo-io-geojson", version = "0.1.0" }
```

```rust
use geo_core::Geometry;
use geo_io_geojson::{from_geojson_geometry, to_geojson_geometry};
```

## Package surface

Primary workflow: `geoJson.bounds`.

Workflow operations:

- `geoJson.bounds`: Computes bounds and coordinate counts for a GeoJSON document.
- `geoJson.distance`: Computes haversine meters or planar coordinate-unit distance between lon/lat coordinates.
- `geoJson.toGeoJson`: Converts the geo-core Geometry JSON shape into a GeoJSON geometry object.

Debug operations:

- `describe`: inspect package metadata and runtime support.

Runtime support: library, CLI, server, and WASM wrappers expose these operations.

Run the primary workflow through the CLI:

```bash
cargo run -p moenarch-geo-io-geojson-cli -- run \
  --operation geoJson.bounds \
  --json '{"geoJson":{"coordinates":[8.0,49.0],"type":"Point"}}'
```

Successful responses use the shared package-surface shape with `operation`,
`title`, `message`, `summary`, and `result`. Default surface calls are
deterministic, local-first, and do not download models, write persistent files,
or execute external tools unless an operation explicitly documents native or
external-tool execution.
