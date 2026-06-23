export interface SurfaceRequest {
  operation: string;
  input: unknown;
}

export interface SurfaceOperation {
  id: string;
  name: string;
  description?: string;
  inputSchema: unknown;
  outputSchema: unknown;
  exampleRequest: unknown;
  wasmSupported: boolean;
  serverSupported: boolean;
}

export interface PackageSurface {
  library: string;
  version: string;
  operations: SurfaceOperation[];
  capabilities: unknown;
}

export interface SurfaceResponse {
  operation: string;
  value: unknown;
  diagnostics: unknown[];
  artifacts: unknown[];
}

export type GeoVizBounds = [
  west: number,
  south: number,
  east: number,
  north: number,
];
export type GeoVizMetricRecord = Record<string, number>;

export interface GeoVizPoint<TProperties = unknown> {
  id?: string;
  label?: string;
  longitude: number;
  latitude: number;
  metrics?: GeoVizMetricRecord;
  properties?: TProperties;
}

export interface GeoVizIndexedPoint<TProperties = unknown> {
  id: string;
  sourceIndex: number;
  label: string;
  longitude: number;
  latitude: number;
  metrics: GeoVizMetricRecord;
  properties: TProperties;
}

export interface GeoVizViewportQuery {
  bounds: GeoVizBounds;
  zoom: number;
}

export interface GeoVizAggregationOptions {
  radius?: number;
  extent?: number;
  minZoom?: number;
  maxZoom?: number;
}

export type GeoVizAggregationFeature<TProperties = unknown> =
  | {
      kind: "point";
      coordinates: [longitude: number, latitude: number];
      metrics: GeoVizMetricRecord;
      point: GeoVizIndexedPoint<TProperties>;
    }
  | {
      kind: "cluster";
      clusterId: string;
      coordinates: [longitude: number, latitude: number];
      expansionZoom: number;
      metrics: GeoVizMetricRecord;
      pointCount: number;
      pointCountAbbreviated: string;
    };

export interface GeoVizAggregationSummary {
  bounds: GeoVizBounds;
  zoom: number;
  metrics: GeoVizMetricRecord;
  visiblePointCount: number;
  visibleClusterCount: number;
  visibleUnclusteredCount: number;
}

export interface GeoVizAggregation<TProperties = unknown> {
  features: Array<GeoVizAggregationFeature<TProperties>>;
  summary: GeoVizAggregationSummary;
}

export interface GeoVizHeatOptions {
  radiusMeters?: number;
  weightMetric?: string;
}

export interface GeoVizHeatFeature<TProperties = unknown> {
  coordinates: [longitude: number, latitude: number];
  id: string;
  label: string;
  metrics: GeoVizMetricRecord;
  point: GeoVizIndexedPoint<TProperties>;
  pointCount: number;
  rawWeight: number;
  value: number;
}

export interface GeoVizHeatAggregation<TProperties = unknown> {
  features: Array<GeoVizHeatFeature<TProperties>>;
  summary: {
    bounds: GeoVizBounds;
    zoom: number;
    metrics: GeoVizMetricRecord;
    maxWeight: number;
    visiblePointCount: number;
  };
}

export interface GeoVizNearestPointQuery {
  longitude: number;
  latitude: number;
  maxDistance?: number;
}

export interface GeoVizFlow<TProperties = unknown> {
  id?: string;
  label?: string;
  from: [longitude: number, latitude: number];
  to: [longitude: number, latitude: number];
  metrics?: GeoVizMetricRecord;
  properties?: TProperties;
}

export interface GeoVizIndexedFlow<TProperties = unknown> {
  id: string;
  sourceIndex: number;
  label: string;
  from: [longitude: number, latitude: number];
  to: [longitude: number, latitude: number];
  metrics: GeoVizMetricRecord;
  properties: TProperties;
}

export interface GeoVizFlowOptions {
  aggregate?: "none" | "origin-destination" | "grid";
  minWeight?: number;
  weightMetric?: string;
}

export interface GeoVizFlowFeature<TProperties = unknown> {
  flow: GeoVizIndexedFlow<TProperties>;
  rawWeight: number;
  value: number;
}

export interface GeoVizFlowAggregation<TProperties = unknown> {
  features: Array<GeoVizFlowFeature<TProperties>>;
  summary: {
    bounds: GeoVizBounds | null;
    viewportBounds: GeoVizBounds;
    zoom: number;
    metrics: GeoVizMetricRecord;
    maxWeight: number;
    visibleFlowCount: number;
  };
}

export interface GeoVizGeoJsonOptions {
  clipToViewport?: boolean;
  simplifyTolerance?: number;
}

export interface GeoVizGeoJsonViewport<TProperties = unknown> {
  bounds: GeoVizBounds | null;
  featureCollection: {
    type: "FeatureCollection";
    features: Array<{
      geometry: unknown;
      id?: string | number;
      properties?: TProperties;
      type: "Feature";
    }>;
  };
  featureCount: number;
  viewportBounds: GeoVizBounds;
  zoom: number;
}

export interface GeoVizScalarFieldOptions {
  domainBounds?: GeoVizBounds;
  domainPaddingRatio?: number;
  fieldCellSizeMeters?: number;
  fieldColumns?: number;
  fieldRows?: number;
  interpolationEpsilonMeters?: number;
  interpolationExtrapolate?: boolean;
  interpolationK?: number;
  interpolationMaxDistanceMeters?: number;
  interpolationPower?: number;
  valueDomain?: [min: number, max: number];
  valueMetric?: string;
}

export interface GeoVizScalarFieldGrid {
  bounds: GeoVizBounds;
  columns: number;
  rows: number;
  valueDomain: [min: number, max: number] | null;
  values: Array<number | null>;
}

export class GeoPointIndex<TProperties = unknown> {
  constructor(
    points: Array<GeoVizPoint<TProperties>>,
    options?: GeoVizAggregationOptions,
  );
  getBounds(): GeoVizBounds | null;
  getPointById(pointId: string): GeoVizIndexedPoint<TProperties> | null;
  getViewportAggregation(
    query: GeoVizViewportQuery,
  ): GeoVizAggregation<TProperties>;
  getClusterExpansionZoom(clusterId: string): number;
  getClusterLeaves(
    clusterId: string,
    limit?: number,
    offset?: number,
  ): Array<GeoVizIndexedPoint<TProperties>>;
  getHeatFeatures(
    query: GeoVizViewportQuery,
    options?: GeoVizHeatOptions,
  ): GeoVizHeatAggregation<TProperties>;
  nearestPoint(
    query: GeoVizNearestPointQuery,
  ): GeoVizIndexedPoint<TProperties> | null;
  free(): void;
}

export class GeoFlowIndex<TProperties = unknown> {
  constructor(flows: Array<GeoVizFlow<TProperties>>);
  getBounds(): GeoVizBounds | null;
  getViewportFlows(
    query: GeoVizViewportQuery,
    options?: GeoVizFlowOptions,
  ): GeoVizFlowAggregation<TProperties>;
  free(): void;
}

export class GeoJsonIndex<TProperties = unknown> {
  constructor(geoJson: unknown);
  getBounds(): GeoVizBounds | null;
  getViewportFeatures(
    query: GeoVizViewportQuery,
    options?: GeoVizGeoJsonOptions,
  ): GeoVizGeoJsonViewport<TProperties>;
  free(): void;
}

export class ScalarFieldIndex<TProperties = unknown> {
  constructor(
    points: Array<GeoVizPoint<TProperties>>,
    options?: GeoVizScalarFieldOptions,
  );
  getBounds(): GeoVizBounds | null;
  getPointCount(): number;
  getValueDomain(): [min: number, max: number] | null;
  getValueAtCoordinate(
    coordinate: [longitude: number, latitude: number],
  ): number | null;
  createGrid(): GeoVizScalarFieldGrid;
  free(): void;
}

export function createScalarFieldGrid<TProperties = unknown>(
  points: Array<GeoVizPoint<TProperties>>,
  options?: GeoVizScalarFieldOptions,
): GeoVizScalarFieldGrid;

export function init(): Promise<unknown>;
export function packageSurface(): PackageSurface;
export function runOperation(request: SurfaceRequest): SurfaceResponse;
