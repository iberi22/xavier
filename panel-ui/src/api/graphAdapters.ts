import type { GraphData, GraphNode, GraphLink } from "../types";

/**
 * Maps backend memory entity graph data to the local CanvasGraph structure.
 */
export function memoryViewToCanvas(backendData: any): GraphData {
  if (!backendData) return { nodes: [], links: [] };

  // Handle various wrapped payload shapes
  const rawNodes = backendData.nodes || backendData.entities || backendData.results || [];
  const rawLinks = backendData.links || backendData.relations || backendData.edges || [];

  const nodes: GraphNode[] = Array.isArray(rawNodes)
    ? rawNodes.map((n: any) => {
        const id = String(n.id || n.name || n.entity_id || "");
        return {
          id,
          label: n.label || n.name || n.title || id,
          type: n.type || n.kind || "entity",
          description: n.description || n.content || n.snippet || "",
          meta: n.meta || n.metadata || {
            kind: n.kind || n.type,
            trust: n.trust,
            memory_count: n.memory_count || n.count,
          },
        };
      })
    : [];

  const links: GraphLink[] = Array.isArray(rawLinks)
    ? rawLinks.map((l: any) => {
        const source = l.source || l.source_id || (typeof l.from === "object" ? l.from.id : l.from);
        const target = l.target || l.target_id || (typeof l.to === "object" ? l.to.id : l.to);
        return {
          source: String(source),
          target: String(target),
          relation: l.relation || l.relation_type || l.type || "related",
        };
      })
    : [];

  return { nodes, links };
}

/**
 * Maps backend code graph (overview/ego) data to the local CanvasGraph structure.
 */
export function codeViewToCanvas(backendData: any): GraphData {
  if (!backendData) return { nodes: [], links: [] };

  const rawNodes = backendData.nodes || backendData.symbols || backendData.results || [];
  const rawLinks = backendData.links || backendData.dependencies || backendData.edges || [];

  const nodes: GraphNode[] = Array.isArray(rawNodes)
    ? rawNodes.map((n: any) => {
        const id = String(n.id || n.stable_id || n.name || n.symbol_id || "");
        return {
          id,
          label: n.label || n.name || n.symbol || id,
          type: n.type || n.symbol_type || n.kind || "symbol",
          description: n.description || n.signature || n.path || n.file_path || "",
          meta: {
            path: n.path || n.file_path,
            language: n.language || n.lang,
            line: n.line || n.start_line,
            signature: n.signature,
            complexity: n.complexity,
            kind: n.type || n.symbol_type || n.kind,
          },
        };
      })
    : [];

  const links: GraphLink[] = Array.isArray(rawLinks)
    ? rawLinks.map((l: any) => {
        const source = l.source || l.source_id || (typeof l.from === "object" ? l.from.id : l.from);
        const target = l.target || l.target_id || (typeof l.to === "object" ? l.to.id : l.to);
        return {
          source: String(source),
          target: String(target),
          relation: l.relation || l.edge_type || l.type || "depends_on",
        };
      })
    : [];

  return { nodes, links };
}
