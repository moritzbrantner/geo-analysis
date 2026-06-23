import { expect, test } from "bun:test";

test("geo-io-osm-wasm package describes and runs the primary operation", async () => {
  const entry = await import("../index.js");
  expect(typeof entry.init).toBe("function");
  expect(typeof entry.packageSurface).toBe("function");
  expect(typeof entry.runOperation).toBe("function");

  const surface = await entry.packageSurface();
  expect(surface.library).toBe("moenarch-geo-io-osm");

  const describe = await entry.runOperation({
    operation: "describe",
    input: { includeOperations: true },
  });
  expect(describe.operation).toBe("describe");

  const filtered = await entry.runOperation({
    operation: "osm.filterPbfBase64",
    input: { pbfBase64: "", spec: { filter: { types: ["node"] } } },
  });
  expect(filtered.value.featureCount).toBe(0);
});
