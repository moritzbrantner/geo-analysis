import initWasm, {
  GeoFlowIndex as WasmGeoFlowIndex,
  GeoJsonIndex as WasmGeoJsonIndex,
  GeoPointIndex as WasmGeoPointIndex,
  ScalarFieldIndex as WasmScalarFieldIndex,
  createScalarFieldGrid as wasmCreateScalarFieldGrid,
  initSync,
  packageSurface as wasmPackageSurface,
  runOperation as wasmRunOperation,
} from "./pkg/geo_viz_wasm.js";
import * as wasmModule from "./pkg/geo_viz_wasm.js";

let initialized = false;

export async function init() {
  if (!initialized) {
    if (isNodeLikeRuntime()) {
      initializeNodeSync();
    } else {
      await initWasm();
      initialized = true;
    }
  }

  return wasmModule;
}

if (!isNodeLikeRuntime()) {
  await init();
}

export class GeoPointIndex {
  constructor(points, options) {
    initializeNodeSync();
    this.inner = new WasmGeoPointIndex(points, options ?? null);
  }

  getBounds() {
    return this.inner.getBounds();
  }

  getPointById(pointId) {
    return this.inner.getPointById(pointId);
  }

  getViewportAggregation(query) {
    return this.inner.getViewportAggregation(query);
  }

  getClusterExpansionZoom(clusterId) {
    return this.inner.getClusterExpansionZoom(clusterId);
  }

  getClusterLeaves(clusterId, limit, offset) {
    return this.inner.getClusterLeaves(clusterId, limit, offset);
  }

  getHeatFeatures(query, options) {
    return this.inner.getHeatFeatures(query, options ?? null);
  }

  nearestPoint(query) {
    return this.inner.nearestPoint(query);
  }

  free() {
    this.inner.free();
  }
}

export class GeoFlowIndex {
  constructor(flows) {
    initializeNodeSync();
    this.inner = new WasmGeoFlowIndex(flows);
  }

  getBounds() {
    return this.inner.getBounds();
  }

  getViewportFlows(query, options) {
    return this.inner.getViewportFlows(query, options ?? null);
  }

  free() {
    this.inner.free();
  }
}

export class GeoJsonIndex {
  constructor(geoJson) {
    initializeNodeSync();
    this.inner = new WasmGeoJsonIndex(geoJson);
  }

  getBounds() {
    return this.inner.getBounds();
  }

  getViewportFeatures(query, options) {
    return this.inner.getViewportFeatures(query, options ?? null);
  }

  free() {
    this.inner.free();
  }
}

export class ScalarFieldIndex {
  constructor(points, options) {
    initializeNodeSync();
    this.inner = new WasmScalarFieldIndex(points, options ?? null);
  }

  getBounds() {
    return this.inner.getBounds();
  }

  getPointCount() {
    return this.inner.getPointCount();
  }

  getValueDomain() {
    return this.inner.getValueDomain();
  }

  getValueAtCoordinate(coordinate) {
    return this.inner.getValueAtCoordinate(coordinate);
  }

  createGrid() {
    return this.inner.createGrid();
  }

  free() {
    this.inner.free();
  }
}

export function createScalarFieldGrid(points, options) {
  initializeNodeSync();
  return wasmCreateScalarFieldGrid(points, options ?? null);
}

export function packageSurface() {
  initializeNodeSync();
  return wasmPackageSurface();
}

export function runOperation(request) {
  initializeNodeSync();
  return wasmRunOperation(request);
}

function initializeNodeSync() {
  if (initialized || !isNodeLikeRuntime()) {
    return;
  }

  const wasmFile = "./pkg/" + "geo_viz_wasm_bg.wasm";
  const wasmPath = new URL(wasmFile, import.meta.url);
  const bytes = readNodeFileSync(wasmPath);

  initSync({ module: bytes });
  initialized = true;
}

function isNodeLikeRuntime() {
  return typeof process !== "undefined" && Boolean(process.versions?.node);
}

function readNodeFileSync(wasmPath) {
  const fs = process.getBuiltinModule?.("fs");

  if (!fs?.readFileSync) {
    throw new Error(
      `geo-viz-wasm could not synchronously read ${wasmPath.toString()}.`,
    );
  }

  const path =
    wasmPath.protocol === "file:"
      ? wasmPath
      : decodeURIComponent(wasmPath.pathname.replace(/^\/@fs/, ""));

  return fs.readFileSync(path);
}
