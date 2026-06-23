# geo-core

Format-agnostic geospatial domain types, geometry helpers, and transforms.

This crate owns stable internal concepts such as coordinates, bounding boxes,
geometry, features, collections, and geometry simplification. It intentionally
does not depend on GeoJSON or expose wire-format crate types.

## Install

```toml
[dependencies]
geo-core = { package = "moenarch-geo-core", version = "0.1.0" }
```

```rust
use geo_core::{Coordinate, Geometry};
```

## Package surface

Primary workflow: `geo.distance`.

Workflow operations:

- `geo.bounds`: Computes bounds and coordinate counts for an internal geometry document.
- `geo.distance`: Computes haversine meters or planar coordinate-unit distance between lon/lat coordinates.

Debug operations:

- `describe`: inspect package metadata and runtime support.

Runtime support: library, CLI, server, and WASM wrappers expose these operations.

Run the primary workflow through the CLI:

```bash
cargo run -p moenarch-geo-core-cli -- run \
  --operation geo.distance \
  --json '{"from":[8.0,49.0],"mode":"haversine","to":[9.0,49.0]}'
```

Successful responses use the shared package-surface shape with `operation`,
`title`, `message`, `summary`, and `result`. Default surface calls are
deterministic, local-first, and do not download models, write persistent files,
or execute external tools unless an operation explicitly documents native or
external-tool execution.
