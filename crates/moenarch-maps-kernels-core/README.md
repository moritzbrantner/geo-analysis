# maps-kernels-core

Numeric kernels for map and temporal GeoJSON processing.
This crate is part of the Analytical Math Crates family.

The crate intentionally starts with small, deterministic `f64` kernels so
TypeScript and WASM implementations can be A/B tested against each other.

## Highlights

- Flat 2D line and ring path summaries
- Deterministic line resampling, simplification, and densification
- Bounds and path-length calculations for map-oriented geometry payloads

## Package surface

Primary workflow: `maps.kernelSummary`.

Workflow operations:

- `maps.kernelSummary`: Summarizes flat 2D coordinates as an open line or closed ring.
- `maps.applyKernel`: Resamples flat 2D coordinates as an open line or closed ring.
- `maps.pathSummary`: Reports point count, segment count, length, and bounds for a flat 2D path.
- `maps.simplifyLine`: Simplifies a flat open 2D line with deterministic Douglas-Peucker simplification.
- `maps.densifyLine`: Inserts flat 2D line points so no segment exceeds the requested length.

Debug operations:

- `describe`: inspect package metadata and runtime support.

Runtime support: the library surface exposes these operations for downstream
packages such as `geo-viz`.

Run the primary workflow through the library surface:

```rust,ignore
use maps_kernels_core::surface;
```

Successful responses use the shared `SurfaceResponse` envelope with an
operation id, JSON value, diagnostics, and artifacts. Default surface calls are
deterministic, local-first, and do not download models, write persistent files,
or execute external tools.
