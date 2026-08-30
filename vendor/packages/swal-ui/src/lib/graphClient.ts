import { getXavierBaseUrl } from "./config";

export interface GraphNode {
  id: string;
  label: string;
  entity_type: "entity" | "code_symbol" | "commit" | "decision" | string;
  app_id?: string;
  trust_score?: number;
  memory_count?: number;
  kind?: string; // Sometimes kind is used instead of entity_type
  [key: string]: any;
}

export interface GraphEdge {
  id: string;
  source: string;
  target: string;
  weight?: number;
  relation?: string;
}

export interface GraphData {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/**
 * Fetch a graph view from Xavier APIs.
 *
 * NOTE ON WASM APPLICABILITY:
 * - We evaluated whether any WASM operations (e.g. `normalize_backlog`) could be rewired here.
 * - However, this client is a pure adapter fetching different layer graphs (memory, code, ecosystem) and managing local
 *   deterministic seed mock data. It has no backlog or frame payload structure.
 * - Therefore, no WASM operations are applicable to this network graph fetch flow, and it remains purely in TypeScript.
 */
export async function fetchGraph(
  layer: "memory" | "code" | "ecosystem",
  entityId?: string
): Promise<GraphData> {
  const base = getXavierBaseUrl();
  let url = "";

  switch (layer) {
    case "memory":
      url = `${base}/memory/graph/view`;
      break;
    case "code":
      url = `${base}/code/graph/view`;
      break;
    case "ecosystem":
      url = `${base}/maloca/graph/ecosystem`;
      break;
  }

  if (entityId) {
    url += `?entity_id=${encodeURIComponent(entityId)}`;
  }

  try {
    const res = await fetch(url);
    if (!res.ok) {
      // Mock fallback if API not ready
      return getMockGraphData(layer, entityId);
    }
    return await res.json();
  } catch (e) {
    console.warn("Xavier Graph API not reachable, using mock data", e);
    return getMockGraphData(layer, entityId);
  }
}

function getMockGraphData(layer: string, entityId?: string): GraphData {
  const isEgo = !!entityId;
  if (isEgo) {
    return {
      nodes: [
        { id: entityId, label: `Ego ${entityId}`, entity_type: "entity", app_id: "core", trust_score: 95 },
        { id: `${entityId}-child1`, label: `Child 1`, entity_type: "decision", app_id: "swal-backoffice", memory_count: 5 },
        { id: `${entityId}-child2`, label: `Child 2`, entity_type: "commit", app_id: "swal-node" }
      ],
      edges: [
        { id: `e1-${entityId}`, source: entityId, target: `${entityId}-child1`, weight: 0.8 },
        { id: `e2-${entityId}`, source: entityId, target: `${entityId}-child2`, weight: 0.5 }
      ]
    };
  }

  if (layer === "memory") {
    return {
      nodes: [
        { id: "mem1", label: "Core Mem", entity_type: "entity", app_id: "core", trust_score: 90, memory_count: 10 },
        { id: "mem2", label: "Auth Concept", entity_type: "decision", app_id: "swal-ui", trust_score: 80, memory_count: 2 },
        { id: "mem3", label: "Mesh Setup", entity_type: "commit", app_id: "swal-node", trust_score: 99 }
      ],
      edges: [
        { id: "e1", source: "mem1", target: "mem2", weight: 0.7 },
        { id: "e2", source: "mem1", target: "mem3", weight: 0.9 }
      ]
    };
  }

  if (layer === "code") {
    return {
      nodes: [
        { id: "code1", label: "App.svelte", entity_type: "code_symbol", app_id: "swal-backoffice", trust_score: 100 },
        { id: "code2", label: "GraphExplorer", entity_type: "code_symbol", app_id: "swal-backoffice" },
        { id: "code3", label: "update()", entity_type: "code_symbol", app_id: "core" }
      ],
      edges: [
        { id: "ec1", source: "code1", target: "code2", weight: 0.8 },
        { id: "ec2", source: "code2", target: "code3", weight: 0.6 }
      ]
    };
  }

  return {
    nodes: [
      { id: "eco1", label: "SWAL Ops", entity_type: "entity", app_id: "swal-backoffice", trust_score: 100 },
      { id: "eco2", label: "SWAL Node", entity_type: "entity", app_id: "swal-node", trust_score: 100 },
      { id: "eco3", label: "Xavier Instance", entity_type: "decision", app_id: "xavier", trust_score: 90 }
    ],
    edges: [
      { id: "ee1", source: "eco1", target: "eco2", weight: 1.0 },
      { id: "ee2", source: "eco2", target: "eco3", weight: 0.9 }
    ]
  };
}
