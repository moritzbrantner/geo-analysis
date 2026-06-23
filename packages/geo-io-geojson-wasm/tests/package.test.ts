import { expect, test } from "bun:test";

test("geo-io-geojson-wasm package describes and runs the primary operation", async () => {
  const entry = await import("../index.js");
  expect(typeof entry.init).toBe("function");
  expect(typeof entry.packageSurface).toBe("function");
  expect(typeof entry.runOperation).toBe("function");

  const surface = await entry.packageSurface();
  expect(surface.library).toBe("moenarch-geo-io-geojson");

  const describe = await entry.runOperation({
    operation: "describe",
    input: { includeOperations: true },
  });
  expect(describe.operation).toBe("describe");

  const bounds = await entry.runOperation({
    operation: "geoJson.bounds",
    input: { geoJson: { type: "Point", coordinates: [8, 49] } },
  });
  expect(bounds.value.coordinateCount).toBe(1);
});
