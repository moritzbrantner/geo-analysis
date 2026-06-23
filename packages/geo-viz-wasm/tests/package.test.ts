import { expect, test } from "bun:test";

test("geo-viz-wasm package exports stable entrypoints", async () => {
  const entry = await import("../index.js");
  expect(typeof entry.init).toBe("function");
  expect(typeof entry.packageSurface).toBe("function");
  expect(typeof entry.runOperation).toBe("function");
  expect(typeof entry.GeoPointIndex).toBe("function");
  expect(typeof entry.ScalarFieldIndex).toBe("function");
  expect(typeof entry.createScalarFieldGrid).toBe("function");

  const surface = await entry.packageSurface();
  expect(surface.library).toBe("moenarch-geo-viz");

  const describe = await entry.runOperation({
    operation: "describe",
    input: {},
  });
  expect(describe.operation).toBe("describe");
});

test("GeoPointIndex aggregates viewport features", async () => {
  const { GeoPointIndex } = await import("../index.js");
  const index = new GeoPointIndex([
    { id: "a", longitude: 13, latitude: 52, metrics: { value: 2 } },
    { id: "b", longitude: 13.0001, latitude: 52.0001, metrics: { value: 3 } },
  ]);
  const aggregation = index.getViewportAggregation({
    bounds: [12.9, 51.9, 13.1, 52.1],
    zoom: 1,
  });

  expect(aggregation.summary.visiblePointCount).toBe(2);
  expect(aggregation.summary.metrics.value).toBe(5);
});

test("runOperation resamples geometry", async () => {
  const { runOperation } = await import("../index.js");
  const response = await runOperation({
    operation: "geoViz.resampleGeometry",
    input: {
      coordinates: [0, 0, 10, 0],
      coordinateCount: 3,
      closed: false,
    },
  });

  expect(response.value.coordinates).toEqual([0, 0, 5, 0, 10, 0]);
});

test("ScalarFieldIndex samples IDW values and creates grids", async () => {
  const { ScalarFieldIndex, createScalarFieldGrid } = await import("../index.js");
  const points = [
    { id: "cold", longitude: 0, latitude: 0, metrics: { temperature: 10 } },
    { id: "warm", longitude: 2, latitude: 0, metrics: { temperature: 20 } },
  ];
  const options = {
    domainBounds: [0, -1, 2, 1],
    fieldColumns: 2,
    fieldRows: 1,
    interpolationK: 2,
    valueMetric: "temperature",
  };
  const index = new ScalarFieldIndex(points, options);

  expect(index.getPointCount()).toBe(2);
  expect(index.getValueAtCoordinate([1, 0])).toBeCloseTo(15, 8);
  expect(index.createGrid().values.length).toBe(2);
  expect(createScalarFieldGrid(points, options).valueDomain).toEqual(
    index.createGrid().valueDomain,
  );
});

test("runOperation creates scalar field grids", async () => {
  const { runOperation } = await import("../index.js");
  const response = await runOperation({
    operation: "geoViz.scalarFieldGrid",
    input: {
      points: [{ id: "a", longitude: 13, latitude: 52, metrics: { value: 3 } }],
      options: {
        domainBounds: [12, 51, 14, 53],
        fieldColumns: 1,
        fieldRows: 1,
      },
    },
  });

  expect(response.value.values).toEqual([3]);
});
