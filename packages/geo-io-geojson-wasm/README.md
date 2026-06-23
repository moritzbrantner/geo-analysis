# GeoJSON WASM

WASM package for `moenarch-geo-io-geojson`.

```ts
import { runOperation } from "@moenarch/geo-io-geojson-wasm";

await runOperation({
  operation: "geoJson.bounds",
  input: { geoJson: { type: "Point", coordinates: [8, 49] } },
});
```
