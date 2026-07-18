import { type GraphData } from "../types";
import { type CanvasGraph, type CanvasNode, type CanvasLink } from "../types/graphLayers";

function getEndpointId(endpoint: any): string {
  if (endpoint === null || endpoint === undefined) {
    return "";
  }
  if (typeof endpoint === "object") {
    return String(endpoint.id || "");
  }
  return String(endpoint);
}

export function roadmapToCanvas(data: GraphData): CanvasGraph {
  if (!data || typeof data !== "object") {
    return {
      layer: "roadmap",
      nodes: [],
      links: [],
    };
  }

  const nodes: CanvasNode[] = (data.nodes || []).map((node) => ({
    id: node.id,
    label: node.label || node.id,
    kind: node.type || "unknown",
    description: node.description,
    meta: {
      parentId: node.parentId,
      date: node.date,
      milestone: node.milestone,
      reason: node.reason,
      relatedFiles: node.relatedFiles,
      decisions: node.decisions,
      commits: node.commits,
      iterations: node.iterations,
    },
  }));

  const links: CanvasLink[] = (data.links || []).map((link) => ({
    source: getEndpointId(link.source),
    target: getEndpointId(link.target),
    relation: link.relation || "",
  }));

  return {
    layer: "roadmap",
    nodes,
    links,
  };
}

export function memoryViewToCanvas(json: any): CanvasGraph {
  if (!json || typeof json !== "object") {
    return { layer: "memory", nodes: [], links: [] };
  }

  const nodes: CanvasNode[] = [];
  const nodeIds = new Set<string>();

  // Extract explicit entities/nodes
  const rawNodes = Array.isArray(json.nodes)
    ? json.nodes
    : Array.isArray(json.entities)
    ? json.entities
    : [];

  for (const n of rawNodes) {
    if (n && typeof n === "object") {
      const id = String(n.id || n.name || "");
      if (!id) continue;
      const node: CanvasNode = {
        id,
        label: String(n.name || n.label || id),
        kind: String(n.entity_type || n.kind || n.type || "Concept"),
        description: n.description ? String(n.description) : undefined,
        meta: { ...n },
      };
      nodes.push(node);
      nodeIds.add(id);
    }
  }

  // Extract links/relations
  const rawLinks = Array.isArray(json.links)
    ? json.links
    : Array.isArray(json.relations)
    ? json.relations
    : [];

  const links: CanvasLink[] = [];
  for (const l of rawLinks) {
    if (l && typeof l === "object") {
      const source = getEndpointId(l.source || l.from || l.source_id);
      const target = getEndpointId(l.target || l.to || l.target_id);
      if (!source || !target) continue;

      const relation = String(l.relation || l.relation_type || l.edge_type || "");
      const weight = typeof l.weight === "number" ? l.weight : undefined;

      links.push({
        source,
        target,
        relation,
        weight,
      });

      // Ensure nodes has both source and target, creating placeholders if needed
      if (!nodeIds.has(source)) {
        nodes.push({
          id: source,
          label: source,
          kind: "Concept",
        });
        nodeIds.add(source);
      }
      if (!nodeIds.has(target)) {
        nodes.push({
          id: target,
          label: target,
          kind: "Concept",
        });
        nodeIds.add(target);
      }
    }
  }

  // Also collect general stats
  const stats: Record<string, number | string> = {};
  if (typeof json.total_relations === "number") {
    stats.total_relations = json.total_relations;
  }
  if (typeof json.max_depth === "number") {
    stats.max_depth = json.max_depth;
  }
  if (typeof json.direction === "string") {
    stats.direction = json.direction;
  }

  return {
    layer: "memory",
    nodes,
    links,
    truncated: typeof json.truncated === "boolean" ? json.truncated : undefined,
    stats: Object.keys(stats).length > 0 ? stats : undefined,
  };
}

export function codeViewToCanvas(json: any): CanvasGraph {
  if (!json || typeof json !== "object") {
    return { layer: "code", nodes: [], links: [] };
  }

  const nodes: CanvasNode[] = [];
  const nodeIds = new Set<string>();

  // Extract symbols/nodes/entities
  const rawNodes = Array.isArray(json.symbols)
    ? json.symbols
    : Array.isArray(json.nodes)
    ? json.nodes
    : Array.isArray(json.entities)
    ? json.entities
    : [];

  for (const n of rawNodes) {
    if (n && typeof n === "object") {
      // Prioritize stable_id or id, fall back to name
      const id = String(n.stable_id || n.id || n.name || "");
      if (!id) continue;
      const node: CanvasNode = {
        id,
        label: String(n.name || n.label || id),
        kind: String(n.kind || n.type || "Symbol"),
        description: n.signature || n.file_path || n.description,
        meta: { ...n },
      };
      nodes.push(node);
      nodeIds.add(id);
    }
  }

  // Extract edges/links/relations
  const rawLinks = Array.isArray(json.edges)
    ? json.edges
    : Array.isArray(json.links)
    ? json.links
    : Array.isArray(json.relations)
    ? json.relations
    : [];

  const links: CanvasLink[] = [];
  for (const l of rawLinks) {
    if (l && typeof l === "object") {
      const source = getEndpointId(l.from_symbol || l.source || l.from);
      const target = getEndpointId(l.to_symbol || l.target || l.to);
      if (!source || !target) continue;

      const relation = String(l.edge_type || l.relation || l.relation_type || "");
      const weight = typeof l.weight === "number" ? l.weight : undefined;

      links.push({
        source,
        target,
        relation,
        weight,
      });

      if (!nodeIds.has(source)) {
        nodes.push({
          id: source,
          label: source,
          kind: "Symbol",
        });
        nodeIds.add(source);
      }
      if (!nodeIds.has(target)) {
        nodes.push({
          id: target,
          label: target,
          kind: "Symbol",
        });
        nodeIds.add(target);
      }
    }
  }

  const stats: Record<string, number | string> = {};
  if (json.stats && typeof json.stats === "object") {
    Object.entries(json.stats).forEach(([key, val]) => {
      if (typeof val === "string" || typeof val === "number") {
        stats[key] = val;
      }
    });
  }

  return {
    layer: "code",
    nodes,
    links,
    truncated: typeof json.truncated === "boolean" ? json.truncated : undefined,
    stats: Object.keys(stats).length > 0 ? stats : undefined,
  };
}
