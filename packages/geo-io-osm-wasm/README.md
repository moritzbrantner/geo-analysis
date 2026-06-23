# OSM WASM

WASM package for `moenarch-geo-io-osm`.

```ts
import { runOperation } from "@moenarch/geo-io-osm-wasm";

await runOperation({
  operation: "osm.filterPbfBase64",
  input: { pbfBase64: "", spec: { filter: { types: ["node"] } } },
});
```
