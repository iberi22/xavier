import { getXavierBaseUrl } from "./config";

// ── Types matching Xavier GET /maloca/commits/graph ─────────────────────────

export interface CommitNode {
  type: "commit";
  id: string;          // full SHA
  short_hash: string;  // 8-char abbreviated hash
  message: string;
  author?: string;
  timestamp?: string;  // ISO date
  repo?: string;
  lines_added?: number;
  lines_deleted?: number;
  files_changed?: number;
}

export interface SymbolNode {
  type: "symbol";
  id: string;          // e.g. "file::path/to/file.ts::functionName"
  label: string;       // display name
  symbol_type?: string; // "function" | "class" | "type" | "module" | etc.
  file_path?: string;
  repo?: string;
  connections?: number;
}

export interface EntityNode {
  type: "entity";
  id: string;
  label: string;
  entity_type?: string;
  repo?: string;
}

export type GraphNodeData = CommitNode | SymbolNode | EntityNode;

export interface GraphLink {
  type: string;        // "commit_file" | "file_symbol" | "commit_symbol" | "symbol_symbol" etc.
  source: string;      // node id
  target: string;      // node id
  weight?: number;
}

export interface CommitGraphResponse {
  ok: boolean;
  repo?: string;
  commits?: any[];
  symbols?: any[];
  nodes: GraphNodeData[];
  links: GraphLink[];
  error?: string;
}

// ── Client ──────────────────────────────────────────────────────────────────

/**
 * Fetch commit network graph data from Xavier.
 *
 * NOTE ON WASM APPLICABILITY:
 * - We evaluated whether any WASM operations (e.g. `classify_frame`, `normalize_backlog`) could be rewired here.
 * - However, this client is a pure adapter fetching commit network graphs (nodes & links), which does not contain backlog
 *   items nor feed frames. Therefore, no WASM operations are applicable to this network graph fetch/normalization flow.
 * - Consequently, this client remains purely implemented in TypeScript as a direct network adapter.
 *
 * @param repos - optional list of repo identifiers to filter (app_id from apps-registry)
 * @param since - optional ISO date string for timeline scrubber lower bound
 * @param until - optional ISO date string for timeline scrubber upper bound
 */
export async function fetchCommitGraph(
  repos?: string[],
  since?: string,
  until?: string,
): Promise<CommitGraphResponse> {
  const base = getXavierBaseUrl();
  const params = new URLSearchParams();
  if (repos && repos.length > 0) {
    params.set("repos", repos.join(","));
  }
  if (since) params.set("since", since);
  if (until) params.set("until", until);

  const qs = params.toString();
  const url = `${base}/maloca/commits/graph${qs ? `?${qs}` : ""}`;

  try {
    const res = await fetch(url);
    if (!res.ok) {
      return emptyGraph(`HTTP ${res.status}: ${res.statusText}`);
    }
    const data: CommitGraphResponse = await res.json();
    if (!data.ok) {
      return emptyGraph(data.error ?? "Backend returned ok=false");
    }
    return data;
  } catch (e) {
    console.warn("Xavier Commit Graph API not reachable", e);
    return emptyGraph(e instanceof Error ? e.message : String(e));
  }
}

function emptyGraph(reason: string): CommitGraphResponse {
  return {
    ok: false,
    nodes: [],
    links: [],
    error: reason,
  };
}
