import { describe, expect, it } from "vitest";
import type { GraphData } from "../src/types";
import {
  mergeFilteredGraphUpdate,
  normalizeGraphData,
} from "../src/utils/roadmapGraph";

const full: GraphData = {
  nodes: [
    {
      id: "org",
      label: "Org",
      type: "organization",
      description: "",
      date: "2026-01-01",
    },
    {
      id: "proj",
      label: "Proj",
      type: "project",
      description: "",
      date: "2026-06-01",
      parentId: "org",
    },
    {
      id: "old",
      label: "Old",
      type: "project",
      description: "",
      date: "2020-01-01",
      parentId: "org",
    },
  ],
  links: [
    { source: "org", target: "proj", relation: "owns" },
    { source: "org", target: "old", relation: "owns" },
  ],
};

describe("roadmapGraph", () => {
  it("normalizes object link endpoints to string ids", () => {
    const dirty = {
      nodes: full.nodes.slice(0, 1),
      links: [
        {
          source: { id: "org" } as unknown as string,
          target: { id: "proj" } as unknown as string,
          relation: "owns",
        },
      ],
    };
    const clean = normalizeGraphData(dirty as GraphData);
    expect(clean.links[0].source).toBe("org");
    expect(clean.links[0].target).toBe("proj");
  });

  it("merges filtered updates without dropping hidden nodes", () => {
    // Simulate date filter that only shows 2026 nodes (hides "old")
    const visibleIds = new Set(["org", "proj"]);
    const updated: GraphData = {
      nodes: [
        {
          id: "org",
          label: "Org Renamed",
          type: "organization",
          description: "updated",
          date: "2026-01-01",
        },
        {
          id: "proj",
          label: "Proj",
          type: "project",
          description: "",
          date: "2026-06-01",
          parentId: "org",
        },
      ],
      links: [{ source: "org", target: "proj", relation: "owns" }],
    };

    const merged = mergeFilteredGraphUpdate(full, visibleIds, updated);
    expect(merged.nodes.map((n) => n.id).sort()).toEqual([
      "old",
      "org",
      "proj",
    ]);
    expect(merged.nodes.find((n) => n.id === "org")?.label).toBe("Org Renamed");
    expect(merged.nodes.find((n) => n.id === "old")?.label).toBe("Old");
    expect(merged.links).toHaveLength(2);
  });
});
