import { useEffect, useMemo, useState } from "react";

import * as wasm from "@moenarch/geo-io-osm-wasm";

const operationInputs = {
  "osm.filterPbfBase64": {
    pbfBase64: "",
    spec: {
      filter: {
        types: ["node"],
      },
    },
  },
  "osm.filterSummary": {
    spec: {
      filter: {
        bbox: [8.5, 48.8, 9.3, 49.2],
      },
    },
  },
  "osm.validateSpec": {
    spec: {
      filter: {
        types: ["node", "way"],
      },
    },
  },
  describe: {
    includeOperations: true,
  },
} as const;

type OperationId = keyof typeof operationInputs;

const operationOrder: OperationId[] = ["osm.filterPbfBase64", "osm.filterSummary", "osm.validateSpec", "describe"];

function formatJson(value: unknown) {
  return JSON.stringify(value, null, 2);
}

export function App() {
  const [operation, setOperation] = useState<OperationId>("osm.filterPbfBase64");
  const [input, setInput] = useState(formatJson(operationInputs["osm.filterPbfBase64"]));
  const [result, setResult] = useState<string>("");
  const [error, setError] = useState<string>("");
  const [surface, setSurface] = useState<wasm.PackageSurface | null>(null);

  useEffect(() => {
    wasm.packageSurface().then(setSurface).catch((cause: unknown) => {
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
            <h1 className="text-2xl font-semibold">OSM I/O</h1>
            <p className="mt-1 text-sm text-slate-600">{surface?.library ?? "moenarch-geo-io-osm"}</p>
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

        <section className="grid gap-5">
          <div>
            <h2 className="section-title">{operationDetails?.name ?? operation}</h2>
            <p className="section-copy">{operationDetails?.description ?? "Run a geo-io-osm operation."}</p>
          </div>

          <label className="grid gap-2">
            <span className="text-sm font-medium text-slate-700">Input</span>
            <textarea
              className="code-input"
              spellCheck={false}
              value={input}
              onChange={(event) => setInput(event.target.value)}
            />
          </label>

          <div>
            <button className="button-primary" type="button" onClick={runSelectedOperation}>
              Run
            </button>
          </div>

          {error ? <pre className="error-text">{error}</pre> : null}
          {result ? <pre className="result-block">{result}</pre> : null}
        </section>
      </div>
    </main>
  );
}
