import type { GraphData, GraphLink } from "../types";

/** Resolve force-graph link endpoint to a stable string id. */
export function linkEndpointId(
  end: GraphLink["source"] | GraphLink["target"] | { id: string },
): string {
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
 * Nodes/links outside the visible filter set are preserved, then dangling
 * links (endpoint missing after delete) are pruned.
 */
export function mergeFilteredGraphUpdate(
  full: GraphData,
  visibleIds: Set<string>,
  updated: GraphData,
): GraphData {
  const normalizedUpdated = normalizeGraphData(updated);
  const hiddenNodes = full.nodes.filter((n) => !visibleIds.has(n.id));
  const nodes = [...hiddenNodes, ...normalizedUpdated.nodes];
  const nodeIds = new Set(nodes.map((n) => n.id));

  const hiddenLinks = full.links.filter((l) => {
    const source = linkEndpointId(l.source);
    const target = linkEndpointId(l.target);
    // Keep links that touch at least one non-visible node (were outside the filter view).
    return !visibleIds.has(source) || !visibleIds.has(target);
  });

  const links = [...hiddenLinks, ...normalizedUpdated.links].filter((l) => {
    const source = linkEndpointId(l.source);
    const target = linkEndpointId(l.target);
    return nodeIds.has(source) && nodeIds.has(target);
  });

  return normalizeGraphData({ nodes, links });
}

export const EMPTY_ROADMAP_GRAPH: GraphData = { nodes: [], links: [] };
