import { expect, test } from "bun:test";

test("geo-core-wasm package exports stable entrypoints", async () => {
  const entry = await import("../index.js");
  expect(typeof entry.init).toBe("function");
  expect(typeof entry.packageSurface).toBe("function");
  expect(typeof entry.runOperation).toBe("function");

  const surface = await entry.packageSurface();
  expect(surface.library).toBe("moenarch-geo-core");

  const describe = await entry.runOperation({
    operation: "describe",
    input: {},
  });
  expect(describe.operation).toBe("describe");

  const distance = await entry.runOperation({
    operation: "geo.distance",
    input: { from: [0, 0], to: [3, 4], mode: "planar" },
  });
  expect(distance.value.distanceUnits).toBe(5);
});
