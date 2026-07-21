import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";

vi.mock("react-force-graph-3d", async () => {
  const React = await import("react");
  const MockForceGraph = React.forwardRef(function MockForceGraph({ graphData }, ref) {
    return React.createElement("canvas", {
      ref,
      "data-testid": "force-graph",
      "data-node-count": graphData.nodes.length,
      "data-edge-count": graphData.links.length
    });
  });
  return { default: MockForceGraph };
});

import { Graph3D } from "./Graph3D";

beforeAll(() => {
  Object.defineProperty(window, "WebGLRenderingContext", {
    configurable: true,
    value: class WebGLRenderingContext {}
  });
});

describe("Graph3D", () => {
  it("mounts a canonical payload with node, edge, and accessible fallback counts", () => {
    render(
      <Graph3D
        title="Fixture graph"
        graph={{
          nodes: [
            { id: "paper", label: "Study paper", kind: "Paper", weight: 2 },
            { id: "concept", label: "Retrieval practice", kind: "Concept", weight: 1 }
          ],
          edges: [{ source: "paper", target: "concept", relation: "studies", weight: 2 }]
        }}
      />
    );

    expect(screen.getByTestId("force-graph")).toHaveAttribute("data-node-count", "2");
    expect(screen.getByTestId("force-graph")).toHaveAttribute("data-edge-count", "1");
    expect(screen.getByText("2 nodes · 1 relationships")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Study paper Paper/i })).toBeInTheDocument();
  });

  it("shows evidence metadata in the deterministic non-WebGL fallback", () => {
    render(
      <Graph3D
        forceFallback
        graph={{
          nodes: [{
            id: "paper",
            label: "Source paper",
            kind: "Paper",
            properties: { citation: "Doe et al.", source_url: "https://example.edu/paper" }
          }],
          edges: []
        }}
      />
    );

    expect(screen.getByText(/WebGL is unavailable/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Source paper Paper/i }));
    expect(screen.getByTestId("graph-evidence-source")).toHaveTextContent("Doe et al.");
  });
});
