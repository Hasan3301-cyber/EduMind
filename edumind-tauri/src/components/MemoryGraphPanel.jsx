import { lazy, Suspense, useEffect, useState } from "react";

const Graph3D = lazy(() => import("./shared/Graph3D").then((module) => ({ default: module.Graph3D })));

export function MemoryGraphPanel({ client }) {
  const [graph, setGraph] = useState({ nodes: [], edges: [], communities: [] });
  const [error, setError] = useState(null);
  const [loading, setLoading] = useState(Boolean(client));

  useEffect(() => {
    let active = true;
    async function load() {
      if (!client) {
        setLoading(false);
        return;
      }
      setLoading(true);
      try {
        const result = await client.memoryGraph();
        if (active) {
          setGraph(result);
          setError(null);
        }
      } catch (reason) {
        if (active) {
          setError(reason.message);
        }
      } finally {
        if (active) {
          setLoading(false);
        }
      }
    }
    void load();
    return () => {
      active = false;
    };
  }, [client]);

  return (
    <div className="panel-stack">
      {loading && <p className="muted">Loading the local memory graph…</p>}
      {error && <p className="error-message">{error}</p>}
      {!client && <p className="muted">Browser preview uses the accessible empty graph state.</p>}
      <Suspense fallback={<p className="muted">Loading the local 3D renderer…</p>}>
        <Graph3D graph={graph} title="Memory knowledge graph" />
      </Suspense>
    </div>
  );
}
