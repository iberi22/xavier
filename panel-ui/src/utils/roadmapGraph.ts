import { type GraphData, type GraphNode, type GraphLink } from "../types";

export const EMPTY_ROADMAP_GRAPH: GraphData = {
  nodes: [],
  links: [],
};

export function normalizeGraphData(data: any): GraphData {
  if (!data || typeof data !== "object") {
    return { nodes: [], links: [] };
  }

  const rawNodes = Array.isArray(data.nodes) ? data.nodes : [];
  const rawLinks = Array.isArray(data.links) ? data.links : [];

  const nodes: GraphNode[] = rawNodes
    .filter((n: any) => n && typeof n === "object" && typeof n.id === "string")
    .map((n: any) => ({
      id: n.id,
      label: typeof n.label === "string" ? n.label : n.id,
      type: ["organization", "project", "subproject", "session"].includes(n.type)
        ? n.type
        : "session",
      description: typeof n.description === "string" ? n.description : "",
      parentId: typeof n.parentId === "string" ? n.parentId : undefined,
      date: typeof n.date === "string" ? n.date : undefined,
      milestone: typeof n.milestone === "string" ? n.milestone : undefined,
      reason: typeof n.reason === "string" ? n.reason : undefined,
      relatedFiles: Array.isArray(n.relatedFiles) ? n.relatedFiles.map(String) : undefined,
      decisions: Array.isArray(n.decisions) ? n.decisions.map(String) : undefined,
      commits: Array.isArray(n.commits) ? n.commits.map(String) : undefined,
      iterations: Array.isArray(n.iterations) ? n.iterations.map(String) : undefined,
    }));

  const links: GraphLink[] = rawLinks
    .filter(
      (l: any) =>
        l &&
        typeof l === "object" &&
        (typeof l.source === "string" || (l.source && typeof l.source === "object") || typeof l.source === "number") &&
        (typeof l.target === "string" || (l.target && typeof l.target === "object") || typeof l.target === "number")
    )
    .map((l: any) => {
      const getEndpointId = (endpoint: any): string => {
        if (endpoint && typeof endpoint === "object") {
          return String(endpoint.id || "");
        }
        return String(endpoint);
      };

      return {
        source: getEndpointId(l.source),
        target: getEndpointId(l.target),
        relation: typeof l.relation === "string" ? l.relation : "",
      };
    });

  return { nodes, links };
}

export function mergeFilteredGraphUpdate(
  current: GraphData,
  update: GraphData
): GraphData {
  const normalizedCurrent = normalizeGraphData(current);
  const normalizedUpdate = normalizeGraphData(update);

  const nodeMap = new Map<string, GraphNode>();
  for (const node of normalizedCurrent.nodes) {
    nodeMap.set(node.id, node);
  }
  for (const node of normalizedUpdate.nodes) {
    nodeMap.set(node.id, node);
  }

  const linksMap = new Map<string, GraphLink>();
  const getLinkKey = (l: GraphLink) => `${l.source}:::${l.target}:::${l.relation}`;

  for (const link of normalizedCurrent.links) {
    linksMap.set(getLinkKey(link), link);
  }
  for (const link of normalizedUpdate.links) {
    linksMap.set(getLinkKey(link), link);
  }

  return {
    nodes: Array.from(nodeMap.values()),
    links: Array.from(linksMap.values()),
  };
}
