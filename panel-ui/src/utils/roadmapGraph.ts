import type { GraphData, GraphLink } from "../types";

function linkEndpointId(end: GraphLink["source"] | GraphLink["target"]): string {
  return typeof end === "object" && end !== null && "id" in end
    ? String((end as { id: string }).id)
    : String(end);
}

/** Normalize force-graph mutations so source/target stay string ids. */
export function normalizeGraphData(data: GraphData): GraphData {
  return {
    nodes: data.nodes.map((n) => ({ ...n })),
    links: data.links.map((l) => ({
      source: linkEndpointId(l.source),
      target: linkEndpointId(l.target),
      relation: l.relation,
    })),
  };
}

/**
 * Merge an update from a filtered GraphView back into the full roadmap.
 * Nodes/links outside the visible filter set are preserved.
 */
export function mergeFilteredGraphUpdate(
  full: GraphData,
  visibleIds: Set<string>,
  updated: GraphData,
): GraphData {
  const normalizedUpdated = normalizeGraphData(updated);
  const hiddenNodes = full.nodes.filter((n) => !visibleIds.has(n.id));
  const hiddenLinks = full.links.filter((l) => {
    const source = linkEndpointId(l.source);
    const target = linkEndpointId(l.target);
    return !visibleIds.has(source) || !visibleIds.has(target);
  });

  return normalizeGraphData({
    nodes: [...hiddenNodes, ...normalizedUpdated.nodes],
    links: [...hiddenLinks, ...normalizedUpdated.links],
  });
}

export const EMPTY_ROADMAP_GRAPH: GraphData = { nodes: [], links: [] };
