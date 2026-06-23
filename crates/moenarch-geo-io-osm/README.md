# geo-io-osm

OpenStreetMap PBF import adapters for `geo-core`.

This crate reads local or in-memory `.osm.pbf` data, applies practical OSM
element, bbox, and tag filters, resolves way geometry through a node coordinate
index, and emits `geo-core` feature collections. It intentionally does not own
HTTP downloads or Geofabrik fetch caching.

## Install

```toml
[dependencies]
geo-io-osm = { package = "moenarch-geo-io-osm", version = "0.1.0" }
```

```rust
use geo_io_osm::{collect_osm_pbf_bytes, CollectOsmBytesOptions, IndexOptions, OsmFilterSpec};

# fn run(pbf_bytes: &[u8]) -> geo_core::Result<()> {
let collected = collect_osm_pbf_bytes(CollectOsmBytesOptions {
    input: pbf_bytes,
    spec: OsmFilterSpec::default(),
    index_options: IndexOptions::default(),
})?;

let geo = collected.into_geo_feature_collection();
# Ok(())
# }
```

## Package surface

Primary workflow: `osm.filterPbfBase64`.

Workflow operations:

- `osm.filterSummary`: Summarizes the effective OSM PBF filter configuration.
- `osm.filterPbfBase64`: Filters base64-encoded OSM PBF bytes into GeoJSON-shaped features.

Debug operations:

- `describe`: inspect package metadata and runtime support.
- `osm.validateSpec`: Validates an OSM PBF filter specification.

Runtime support: library, CLI, server, and WASM wrappers expose these operations.

Run the primary workflow through the CLI:

```bash
cargo run -p moenarch-geo-io-osm-cli -- run \
  --operation osm.filterPbfBase64 \
  --json '{"pbfBase64":"","spec":{"filter":{"include":{"any":[{"key":"amenity","values":["school","hospital"]}]},"types":["node","way"]},"output":{"geometry":"full"}}}'
```

Successful responses use the shared package-surface shape with `operation`,
`title`, `message`, `summary`, and `result`. Default surface calls are
deterministic, local-first, and do not download models, write persistent files,
or execute external tools unless an operation explicitly documents native or
external-tool execution.
