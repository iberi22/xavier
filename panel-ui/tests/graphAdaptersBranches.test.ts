import { describe, it, expect } from "vitest";
import {
  roadmapToCanvas,
  memoryViewToCanvas,
  codeViewToCanvas,
  canvasToForceData,
} from "../src/api/graphAdapters";

describe("graphAdapters branch unit tests", () => {
  it("handles null/undefined endpoint object in getEndpointId", () => {
    expect(roadmapToCanvas({
      nodes: [{ id: "n1", label: "Node 1" }],
      links: [{ source: null as any, target: { id: "n1" } }],
    })).toEqual({
      layer: "roadmap",
      nodes: [
        {
          id: "n1",
          label: "Node 1",
          kind: "unknown",
          description: undefined,
          meta: {
            parentId: undefined,
            date: undefined,
            milestone: undefined,
            reason: undefined,
            relatedFiles: undefined,
            decisions: undefined,
            commits: undefined,
            iterations: undefined,
          },
        },
      ],
      links: [{ source: "", target: "n1", relation: "" }],
    });
  });

  it("handles empty / non-object memoryViewToCanvas and codeViewToCanvas input", () => {
    expect(memoryViewToCanvas(null)).toEqual({ layer: "memory", nodes: [], links: [] });
    expect(codeViewToCanvas(undefined)).toEqual({ layer: "code", nodes: [], links: [] });
  });

  it("covers memoryViewToCanvas branch conditions", () => {
    const memoryData = {
      entities: [
        { id: "e1", name: "Entity 1", kind: "Topic", description: "Desc 1" },
        { id: "", name: "" }, // invalid node (skipped)
        { name: "UnnamedEntity" }, // uses name as id & label
      ],
      relations: [
        { from: "e1", to: "UnnamedEntity", relation_type: "connects", weight: 0.9 },
        { from: "e1", to: "placeholder_node", edge_type: "links" }, // creates placeholder_node
        { from: "", to: "e1" }, // skipped due to empty source
      ],
      total_relations: 5,
      max_depth: 3,
      direction: "outbound",
      truncated: true,
    };

    const canvas = memoryViewToCanvas(memoryData);
    expect(canvas.nodes.length).toBe(3); // e1, UnnamedEntity, placeholder_node
    expect(canvas.links.length).toBe(2);
    expect(canvas.stats).toEqual({ total_relations: 5, max_depth: 3, direction: "outbound" });
    expect(canvas.truncated).toBe(true);
  });

  it("covers codeViewToCanvas branch conditions", () => {
    const codeData = {
      symbols: [
        { stable_id: "s1", name: "funcA", signature: "fn funcA()", kind: "Function" },
        { id: "s2", label: "funcB", type: "Method" },
        { name: "s3" }, // uses name as id & label
      ],
      edges: [
        { from_symbol: "s1", to_symbol: "s2", edge_type: "calls", weight: 1.0 },
        { source: "s1", target: "s4", relation: "imports" }, // creates placeholder s4
      ],
      stats: { total_symbols: 10, repo: "main" },
      truncated: false,
    };

    const canvas = codeViewToCanvas(codeData);
    expect(canvas.nodes.length).toBe(4); // s1, s2, s3, s4
    expect(canvas.links.length).toBe(2);
    expect(canvas.stats).toEqual({ total_symbols: 10, repo: "main" });
    expect(canvas.truncated).toBe(false);
  });

  it("converts canvas to force data with default fallback kind", () => {
    const canvas = {
      layer: "code" as const,
      nodes: [{ id: "n1", label: "Node 1", kind: "" }],
      links: [{ source: "n1", target: "n2", relation: "rel" }],
    };

    const forceData = canvasToForceData(canvas);
    expect(forceData.nodes[0].type).toBe("session");
  });
});
