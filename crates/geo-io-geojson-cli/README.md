# GeoJSON CLI

Command-line adapter for `moenarch-geo-io-geojson`.

```bash
cargo run -p moenarch-geo-io-geojson-cli -- run --operation describe --json '{"includeOperations":true}'
cargo run -p moenarch-geo-io-geojson-cli -- run --operation geoJson.toGeoJson --json '{"geometry":{"type":"Point","coordinates":[8.0,49.0]}}'
```
