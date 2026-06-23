import { expect, test } from "bun:test";

test("geo-clustering-wasm package exports stable entrypoints", async () => {
  const entry = await import("../index.js");
  expect(typeof entry.init).toBe("function");
  expect(typeof entry.packageSurface).toBe("function");
  expect(typeof entry.runOperation).toBe("function");

  const surface = await entry.packageSurface();
  expect(surface.library).toBe("moenarch-geo-clustering");

  const describe = await entry.runOperation({
    operation: "describe",
    input: {},
  });
  expect(describe.operation).toBe("describe");

  const bounds = await entry.runOperation({
    operation: "geoCluster.bounds",
    input: {
      points: [{ id: "a", longitude: 8, latitude: 49, properties: {} }],
    },
  });
  expect(bounds.value.bounds).toEqual([8, 49, 8, 49]);
});
