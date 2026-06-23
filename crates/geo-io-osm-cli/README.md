# OSM CLI

Command-line adapter for `moenarch-geo-io-osm`.

```bash
cargo run -p moenarch-geo-io-osm-cli -- run --operation describe --json '{"includeOperations":true}'
cargo run -p moenarch-geo-io-osm-cli -- run --operation osm.filterSummary --json '{"spec":{"filter":{"bbox":[8.5,48.8,9.3,49.2]}}}'
```
