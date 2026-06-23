import { useEffect, useMemo, useState } from "react";

import * as wasm from "@moenarch/geo-viz-wasm";

const viewport = { bounds: [7.0, 48.0, 10.0, 51.0], zoom: 8 };

const operationInputs = {
  "geoViz.aggregateViewport": {
    points: [{ id: "a", longitude: 8.0, latitude: 49.0, metrics: { weight: 2 } }],
    query: viewport,
  },
  "geoViz.heatViewport": {
    points: [{ id: "a", longitude: 8.0, latitude: 49.0, metrics: { weight: 2 } }],
    query: viewport,
  },
  "geoViz.geoJsonViewport": {
    geoJson: {
      type: "FeatureCollection",
      features: [{ type: "Feature", properties: { name: "A" }, geometry: { type: "Point", coordinates: [8.0, 49.0] } }],
    },
    query: viewport,
  },
  "geoViz.flowViewport": {
    flows: [{ from: [8.0, 49.0], to: [9.0, 50.0], metrics: { weight: 2 } }],
    query: viewport,
  },
  "geoViz.scalarFieldGrid": {
    points: [{ id: "a", longitude: 8.0, latitude: 49.0, metrics: { value: 2 } }],
    options: { domainBounds: [7.0, 48.0, 9.0, 50.0], fieldColumns: 4, fieldRows: 2 },
  },
  "geoViz.resampleGeometry": {
    coordinates: [0.0, 0.0, 10.0, 0.0],
    coordinateCount: 3,
    closed: false,
  },
  describe: {
    includeOperations: true,
  },
} as const;

type OperationId = keyof typeof operationInputs;

const operationOrder: OperationId[] = [
  "geoViz.aggregateViewport",
  "geoViz.heatViewport",
  "geoViz.geoJsonViewport",
  "geoViz.flowViewport",
  "geoViz.scalarFieldGrid",
  "geoViz.resampleGeometry",
  "describe",
];

function formatJson(value: unknown) {
  return JSON.stringify(value, null, 2);
}

export function App() {
  const [operation, setOperation] = useState<OperationId>("geoViz.aggregateViewport");
  const [input, setInput] = useState(formatJson(operationInputs["geoViz.aggregateViewport"]));
  const [result, setResult] = useState<string>("");
  const [error, setError] = useState<string>("");
  const [surface, setSurface] = useState<wasm.PackageSurface | null>(null);

  useEffect(() => {
    Promise.resolve(wasm.packageSurface()).then(setSurface).catch((cause: unknown) => {
      setError(cause instanceof Error ? cause.message : String(cause));
    });
  }, []);

  const operationDetails = useMemo(() => {
    return surface?.operations.find((candidate) => candidate.id === operation);
  }, [operation, surface]);

  async function runSelectedOperation() {
    setError("");
    try {
      const response = await wasm.runOperation({
        operation,
        input: JSON.parse(input),
      });
      setResult(formatJson(response));
    } catch (cause) {
      setResult("");
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  function selectOperation(nextOperation: OperationId) {
    setOperation(nextOperation);
    setInput(formatJson(operationInputs[nextOperation]));
    setResult("");
    setError("");
  }

  return (
    <main className="min-h-screen bg-slate-50 text-slate-950">
      <div className="mx-auto grid max-w-6xl gap-6 px-4 py-6 md:grid-cols-[18rem_1fr]">
        <aside className="space-y-4 border-b border-slate-200 pb-4 md:border-b-0 md:border-r md:pr-6">
          <div>
            <h1 className="text-2xl font-semibold">Geo Visualization</h1>
            <p className="mt-1 text-sm text-slate-600">{surface?.library ?? "moenarch-geo-viz"}</p>
          </div>
          <div className="grid gap-2">
            {operationOrder.map((id) => (
              <button
                key={id}
                className={id === operation ? "mode-button mode-button-active" : "mode-button"}
                type="button"
                onClick={() => selectOperation(id)}
              >
                {id}
              </button>
            ))}
          </div>
          <dl className="detail-list">
            <div>
              <dt>Version</dt>
              <dd>{surface?.version ?? "loading"}</dd>
            </div>
            <div>
              <dt>Operations</dt>
              <dd>{surface?.operations.length ?? 0}</dd>
            </div>
          </dl>
        </aside>
        <section className="grid gap-4">
          <div className="panel">
            <h2 className="section-title">{operationDetails?.name ?? operation}</h2>
            <p className="section-copy">{operationDetails?.description ?? "Run a package-surface operation."}</p>
            <textarea className="code-input mt-4" value={input} onChange={(event) => setInput(event.target.value)} />
            <button className="button-primary mt-4" type="button" onClick={runSelectedOperation}>
              Run
            </button>
            {error ? <div className="error-text">{error}</div> : null}
            {result ? <pre className="result-block">{result}</pre> : null}
          </div>
        </section>
      </div>
    </main>
  );
}
