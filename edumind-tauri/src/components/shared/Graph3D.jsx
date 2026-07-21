import { useEffect, useMemo, useRef, useState } from "react";
import ForceGraph3D from "react-force-graph-3d";

const NODE_COLORS = {
  paper: "#38bdf8",
  author: "#a78bfa",
  concept: "#34d399",
  venue: "#fb923c",
  source: "#f472b6"
};

export function Graph3D({ graph, title = "Knowledge graph", onNodeSelect, forceFallback = false }) {
  const graphRef = useRef();
  const reducedMotion = useReducedMotion();
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState("all");
  const [colorMode, setColorMode] = useState("type");
  const [showLabels, setShowLabels] = useState(true);
  const [frozen, setFrozen] = useState(reducedMotion);
  const [selectedId, setSelectedId] = useState(null);
  const webglAvailable = useMemo(() => !forceFallback && canUseWebGl(), [forceFallback]);
  const normalized = useMemo(() => normalizeGraph(graph), [graph]);
  const kinds = useMemo(
    () => [...new Set(normalized.nodes.map((node) => node.nodeType))].sort(),
    [normalized.nodes]
  );
  const graphData = useMemo(
    () => filterGraph(normalized, query, kind),
    [normalized, query, kind]
  );
  const selected = graphData.nodes.find((node) => node.id === selectedId) ?? null;
  const neighborIds = useMemo(() => {
    if (!selected) {
      return new Set();
    }
    const neighbors = new Set([selected.id]);
    for (const link of graphData.links) {
      if (linkEndpointId(link.source) === selected.id) {
        neighbors.add(linkEndpointId(link.target));
      }
      if (linkEndpointId(link.target) === selected.id) {
        neighbors.add(linkEndpointId(link.source));
      }
    }
    return neighbors;
  }, [graphData.links, selected]);

  useEffect(() => {
    if (reducedMotion) {
      setFrozen(true);
    }
  }, [reducedMotion]);

  function selectNode(node) {
    setSelectedId(node.id);
    onNodeSelect?.(node);
    if (!graphRef.current || !Number.isFinite(node.x) || !Number.isFinite(node.y)) {
      return;
    }
    const distance = 110;
    const length = Math.hypot(node.x, node.y, node.z ?? 0) || 1;
    graphRef.current.cameraPosition(
      {
        x: node.x + (node.x / length) * distance,
        y: node.y + (node.y / length) * distance,
        z: (node.z ?? 0) + ((node.z ?? 0) / length) * distance
      },
      node,
      reducedMotion ? 0 : 850
    );
  }

  return (
    <section className="graph3d" aria-labelledby="graph3d-title">
      <div className="panel-heading graph-heading">
        <div>
          <p className="eyebrow">Interactive 3D graph</p>
          <h2 id="graph3d-title">{title}</h2>
        </div>
        <p className="muted" data-testid="graph-summary">
          {graphData.nodes.length} nodes · {graphData.links.length} relationships
        </p>
      </div>
      <div className="graph-controls" aria-label="Graph controls">
        <label>
          <span>Search nodes</span>
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filter labels"
          />
        </label>
        <label>
          <span>Node type</span>
          <select value={kind} onChange={(event) => setKind(event.target.value)}>
            <option value="all">All types</option>
            {kinds.map((entry) => (
              <option key={entry} value={entry}>
                {entry}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span>Color nodes</span>
          <select value={colorMode} onChange={(event) => setColorMode(event.target.value)}>
            <option value="type">By node type</option>
            <option value="community">By community</option>
          </select>
        </label>
        <button
          type="button"
          className="secondary-button"
          aria-pressed={showLabels}
          onClick={() => setShowLabels((current) => !current)}
        >
          {showLabels ? "Hide hover labels" : "Show hover labels"}
        </button>
        <button
          type="button"
          className="secondary-button"
          aria-pressed={frozen}
          onClick={() => setFrozen((current) => !current)}
        >
          {frozen ? "Reheat layout" : "Freeze layout"}
        </button>
      </div>
      <div className="graph-stage" data-testid="graph3d-canvas">
        {webglAvailable ? (
          <ForceGraph3D
            ref={graphRef}
            graphData={graphData}
            backgroundColor="#101827"
            nodeLabel={showLabels ? nodeTooltip : () => ""}
            nodeColor={(node) => nodeColor(node, neighborIds, colorMode)}
            nodeVal={(node) => Math.max(1, Math.min(12, Number(node.weight) || 1))}
            linkColor={(link) => (selected && linkTouches(link, selected.id) ? "#f8fafc" : "#64748b")}
            linkWidth={(link) => Math.max(0.5, Math.min(5, Number(link.weight) || 1))}
            linkLabel={(link) => link.relation || "related"}
            linkDirectionalParticles={selected ? 2 : 0}
            linkDirectionalParticleWidth={1.5}
            onNodeClick={selectNode}
            enableNodeDrag
            cooldownTicks={frozen ? 0 : reducedMotion ? 1 : 140}
            warmupTicks={reducedMotion ? 0 : 20}
            showNavInfo={!reducedMotion}
          />
        ) : (
          <div className="graph-webgl-fallback" role="img" aria-label="3D graph unavailable">
            WebGL is unavailable. Use the keyboard-navigable node list below.
          </div>
        )}
      </div>
      <div className="graph-accessibility">
        <div>
          <h3>Accessible node list</h3>
          <p className="muted">
            Select a node to inspect its neighborhood and metadata without relying on WebGL.
          </p>
          <ul className="graph-node-list" aria-label={`${title} nodes`}>
            {graphData.nodes.slice(0, 120).map((node) => (
              <li key={node.id}>
                <button
                  type="button"
                  className={node.id === selectedId ? "node-list-button selected" : "node-list-button"}
                  onClick={() => selectNode(node)}
                >
                  <span className="node-color" style={{ backgroundColor: nodeColor(node, new Set(), colorMode) }} />
                  <span>{node.label}</span>
                  <small>{node.nodeType}</small>
                </button>
              </li>
            ))}
          </ul>
        </div>
        <aside className="graph-detail" aria-live="polite">
          <h3>{selected ? selected.label : "Selected node"}</h3>
          {selected ? (
            <>
              <p className="muted">{selected.nodeType} · weight {formatWeight(selected.weight)}</p>
              <p>{selected.properties?.summary ?? selected.properties?.abstract ?? "No additional source metadata."}</p>
              <EvidenceSource node={selected} />
              <p className="muted">Connected to {Math.max(0, neighborIds.size - 1)} visible node(s).</p>
            </>
          ) : (
            <p>Choose a node from the canvas or the list to focus it.</p>
          )}
        </aside>
      </div>
    </section>
  );
}

function normalizeGraph(graph) {
  const communities = new Map();
  for (const community of graph?.communities ?? []) {
    for (const nodeId of community.node_ids ?? community.nodeIds ?? []) {
      communities.set(String(nodeId), community.id);
    }
  }
  const nodes = (graph?.nodes ?? []).map((node) => ({
    id: String(node.id),
    label: String(node.label ?? node.id),
    nodeType: String(node.node_type ?? node.nodeType ?? node.kind ?? "Concept"),
    weight: Number(node.weight ?? 1),
    properties: node.properties ?? node.metadata ?? {},
    community: communities.get(String(node.id)) ?? node.community ?? null
  }));
  const known = new Set(nodes.map((node) => node.id));
  const links = (graph?.edges ?? graph?.links ?? [])
    .map((edge) => ({
      source: String(edge.source?.id ?? edge.source),
      target: String(edge.target?.id ?? edge.target),
      relation: String(edge.relation ?? "related"),
      weight: Number(edge.weight ?? 1)
    }))
    .filter((edge) => known.has(edge.source) && known.has(edge.target));
  return { nodes, links };
}

function filterGraph(graph, query, kind) {
  const normalizedQuery = query.trim().toLowerCase();
  const hardware = typeof navigator === "undefined" ? 4 : navigator.hardwareConcurrency ?? 4;
  const limit = hardware <= 4 ? 280 : 900;
  const nodes = graph.nodes
    .filter((node) => kind === "all" || node.nodeType === kind)
    .filter((node) => !normalizedQuery || node.label.toLowerCase().includes(normalizedQuery))
    .sort((left, right) => right.weight - left.weight || left.label.localeCompare(right.label))
    .slice(0, limit);
  const visible = new Set(nodes.map((node) => node.id));
  return {
    nodes,
    links: graph.links.filter((link) => visible.has(link.source) && visible.has(link.target))
  };
}

function canUseWebGl() {
  return typeof window !== "undefined" && !window.__EDUMIND_DISABLE_WEBGL && typeof window.WebGLRenderingContext !== "undefined";
}

function useReducedMotion() {
  const [reduced, setReduced] = useState(() => {
    return typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  });
  useEffect(() => {
    const media = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    if (!media) {
      return undefined;
    }
    const update = () => setReduced(media.matches);
    media.addEventListener?.("change", update);
    return () => media.removeEventListener?.("change", update);
  }, []);
  return reduced;
}

function nodeColor(node, neighbors = new Set(), colorMode = "type") {
  if (neighbors.size && !neighbors.has(node.id)) {
    return "#334155";
  }
  if (colorMode === "community" && node.community) {
    return communityColor(node.community);
  }
  return NODE_COLORS[node.nodeType.toLowerCase()] ?? communityColor(node.community);
}

function communityColor(community) {
  if (!community) {
    return "#94a3b8";
  }
  const palette = ["#22c55e", "#f59e0b", "#ec4899", "#818cf8", "#14b8a6"];
  const index = [...String(community)].reduce((total, character) => total + character.charCodeAt(0), 0);
  return palette[index % palette.length];
}

function nodeTooltip(node) {
  const source = evidenceSource(node);
  const sourceLine = source ? `<br/>Source: ${escapeMarkup(source.label)}` : "";
  return `<strong>${escapeMarkup(node.label)}</strong><br/>${escapeMarkup(node.nodeType)}${sourceLine}`;
}

function EvidenceSource({ node }) {
  const source = evidenceSource(node);
  if (!source) {
    return null;
  }
  return (
    <p className="graph-evidence-source" data-testid="graph-evidence-source">
      <strong>Evidence source:</strong>{" "}
      {source.href ? (
        <a href={source.href} target="_blank" rel="noreferrer">{source.label}</a>
      ) : (
        <span>{source.label}</span>
      )}
    </p>
  );
}

function evidenceSource(node) {
  const properties = node.properties ?? {};
  const raw = properties.source_url ?? properties.sourceUrl ?? properties.url ?? properties.doi_url ?? properties.doi;
  const sourceId = properties.source_id ?? properties.sourceId ?? properties.paper_id ?? properties.paperId;
  const label = properties.citation ?? properties.title ?? sourceId ?? raw;
  if (!label) {
    return null;
  }
  const href = safeHttpUrl(raw);
  return { label: String(label), href };
}

function safeHttpUrl(value) {
  if (!value) {
    return null;
  }
  try {
    const url = new URL(String(value));
    return url.protocol === "https:" || url.protocol === "http:" ? url.toString() : null;
  } catch {
    return null;
  }
}

function escapeMarkup(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

function linkTouches(link, nodeId) {
  return linkEndpointId(link.source) === nodeId || linkEndpointId(link.target) === nodeId;
}

function linkEndpointId(endpoint) {
  return typeof endpoint === "object" ? endpoint.id : endpoint;
}

function formatWeight(value) {
  return Number(value).toFixed(1);
}
