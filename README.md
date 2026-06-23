# geo-analysis

Rust geo analysis package family extracted from `rust-packages`.

The extracted package surfaces are:

- Rust library crate `moenarch-geo-core` with lib target `geo_core`
- Rust library crate `moenarch-geo-io-geojson` with lib target `geo_io_geojson`
- Rust library crate `moenarch-geo-io-osm` with lib target `geo_io_osm`
- CLI, HTTP, WASM, npm wrapper, and Vite app adapters for each surface

## Examples

```toml
[dependencies]
geo-core = { package = "moenarch-geo-core", version = "0.1.0" }
geo-io-geojson = { package = "moenarch-geo-io-geojson", version = "0.1.0" }
geo-io-osm = { package = "moenarch-geo-io-osm", version = "0.1.0" }
```

```bash
cargo run -p moenarch-geo-io-geojson-cli -- run \
  --operation geoJson.bounds \
  --json '{"geoJson":{"type":"Point","coordinates":[8.0,49.0]}}'

cargo run -p moenarch-geo-io-osm-cli -- run \
  --operation osm.filterPbfBase64 \
  --json '{"pbfBase64":"","spec":{"filter":{"types":["node"]}}}'
```

## Checks

```bash
cargo test --workspace
bun install
bun run build
bun run test
cargo package -p moenarch-geo-core
cargo package -p moenarch-geo-io-geojson
```

`moenarch-geo-io-osm` depends on `moenarch-geo-io-geojson`, so registry
package verification for OSM should run after the GeoJSON crate is published.

Release workflow setup is documented in [docs/release.md](docs/release.md).
