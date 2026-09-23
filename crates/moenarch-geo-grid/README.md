# geo-grid

Discrete spatial coverage algorithms over `geo-core` geometry.

The crate owns conversion between continuous geometry and two approximate cell
representations:

- **H3** cells using the canonical H3 index and `h3o` implementation.
- **Square** cells using hierarchical Web-Mercator XYZ tile coordinates.

The algorithm layer accepts and returns `geo-core::Geometry`; it does not own
GeoJSON parsing. `geo-io-geojson` exposes the wire-format operations.

## Semantics

H3 polygon coverage supports centroid, contained-boundary,
intersects-boundary, and covers containment. Point and line geometry use the
native H3 point/line indexing paths.

Square coverage is an intersection/supercover: a square is included when its
Web-Mercator cell intersects the input geometry. Point inputs map to exactly one
cell. Reconstruction merges rectangular runs of adjacent square cells before
emitting a MultiPolygon, so the reconstructed geometry covers exactly the
selected square cells without emitting one polygon per cell.

Both conversions are approximate: converting geometry to cells quantizes its
boundary, and converting cells back reconstructs the selected cell coverage,
not the original coordinates.
