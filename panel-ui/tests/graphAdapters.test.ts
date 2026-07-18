import { describe, it, expect } from "vitest";
import {
  roadmapToCanvas,
  memoryViewToCanvas,
  codeViewToCanvas,
  canvasToForceData,
} from "../src/api/graphAdapters";
import { EMPTY_ROADMAP_GRAPH } from "../src/utils/roadmapGraph";
import { type GraphData } from "../src/types";

describe("graphAdapters", () => {
  it("roadmap mapping preserves ids", () => {
    const roadmapData: GraphData = {
      nodes: [
        {
          id: "node-1",
          label: "My Project",
          type: "project",
          description: "This is a project",
          parentId: "org-1",
        },
      ],
      links: [
        {
          source: "node-1",
          target: "node-2",
          relation: "contains",
        },
      ],
    };

    const canvas = roadmapToCanvas(roadmapData);

    expect(canvas.layer).toBe("roadmap");
    expect(canvas.nodes).toHaveLength(1);
    expect(canvas.nodes[0].id).toBe("node-1");
    expect(canvas.nodes[0].label).toBe("My Project");
    expect(canvas.nodes[0].kind).toBe("project");
    expect(canvas.nodes[0].description).toBe("This is a project");
    expect(canvas.nodes[0].meta?.parentId).toBe("org-1");

    expect(canvas.links).toHaveLength(1);
    expect(canvas.links[0].source).toBe("node-1");
    expect(canvas.links[0].target).toBe("node-2");
    expect(canvas.links[0].relation).toBe("contains");
  });

  it("memory view sample JSON ÔåÆ nodes/links", () => {
    const memoryJson = {
      entities: [
        {
          id: "entity-1",
          name: "My Entity",
          entity_type: "Concept",
          description: "Memory concept description",
        },
      ],
      relations: [
        {
          source: "entity-1",
          target: "entity-2",
          relation_type: "relates_to",
          weight: 0.85,
        },
      ],
      total_relations: 1,
      max_depth: 2,
      direction: "both",
      truncated: false,
    };

    const canvas = memoryViewToCanvas(memoryJson);

    expect(canvas.layer).toBe("memory");
    // Should have 2 nodes: explicit entity-1, and implicit placeholder entity-2
    expect(canvas.nodes).toHaveLength(2);
    const n1 = canvas.nodes.find((n) => n.id === "entity-1");
    const n2 = canvas.nodes.find((n) => n.id === "entity-2");

    expect(n1).toBeDefined();
    expect(n1?.label).toBe("My Entity");
    expect(n1?.kind).toBe("Concept");
    expect(n1?.description).toBe("Memory concept description");

    expect(n2).toBeDefined();
    expect(n2?.label).toBe("entity-2");
    expect(n2?.kind).toBe("Concept");

    expect(canvas.links).toHaveLength(1);
    expect(canvas.links[0].source).toBe("entity-1");
    expect(canvas.links[0].target).toBe("entity-2");
    expect(canvas.links[0].relation).toBe("relates_to");
    expect(canvas.links[0].weight).toBe(0.85);

    expect(canvas.truncated).toBe(false);
    expect(canvas.stats).toEqual({
      total_relations: 1,
      max_depth: 2,
      direction: "both",
    });
  });

  it("code view sample JSON ÔåÆ nodes/links", () => {
    const codeJson = {
      symbols: [
        {
          stable_id: "sym-1",
          name: "my_function",
          kind: "Function",
          signature: "fn my_function()",
          file_path: "src/lib.rs",
        },
      ],
      edges: [
        {
          from_symbol: "sym-1",
          to_symbol: "sym-2",
          edge_type: "Calls",
        },
      ],
      stats: {
        total_files: 5,
        total_symbols: 12,
      },
      truncated: true,
    };

    const canvas = codeViewToCanvas(codeJson);

    expect(canvas.layer).toBe("code");
    expect(canvas.nodes).toHaveLength(2);
    const n1 = canvas.nodes.find((n) => n.id === "sym-1");
    const n2 = canvas.nodes.find((n) => n.id === "sym-2");

    expect(n1).toBeDefined();
    expect(n1?.label).toBe("my_function");
    expect(n1?.kind).toBe("Function");
    expect(n1?.description).toBe("fn my_function()");

    expect(n2).toBeDefined();
    expect(n2?.label).toBe("sym-2");
    expect(n2?.kind).toBe("Symbol");

    expect(canvas.links).toHaveLength(1);
    expect(canvas.links[0].source).toBe("sym-1");
    expect(canvas.links[0].target).toBe("sym-2");
    expect(canvas.links[0].relation).toBe("Calls");

    expect(canvas.truncated).toBe(true);
    expect(canvas.stats).toEqual({
      total_files: 5,
      total_symbols: 12,
    });
  });

  it("object {id} link endpoints coerced to string", () => {
    const roadmapData: GraphData = {
      nodes: [],
      links: [
        {
          source: { id: "source-obj-id" } as any,
          target: { id: "target-obj-id" } as any,
          relation: "points_to",
        },
      ],
    };

    const canvas = roadmapToCanvas(roadmapData);
    expect(canvas.links).toHaveLength(1);
    expect(canvas.links[0].source).toBe("source-obj-id");
    expect(canvas.links[0].target).toBe("target-obj-id");
  });

  it("canvasToForceData maps kind onto type for GraphCanvas paint", () => {
    const force = canvasToForceData({
      layer: "memory",
      nodes: [
        { id: "e1", label: "Alice", kind: "person", description: "dev" },
      ],
      links: [{ source: "e1", target: "e2", relation: "knows" }],
    });
    expect(force.nodes[0].type).toBe("person");
    expect(force.nodes[0].label).toBe("Alice");
    expect(force.links[0].source).toBe("e1");
  });

  it("EMPTY_ROADMAP_GRAPH is empty", () => {
    expect(EMPTY_ROADMAP_GRAPH.nodes).toEqual([]);
    expect(EMPTY_ROADMAP_GRAPH.links).toEqual([]);
  });
});
