import { getApiUrl } from "../api/client";
import { AUTH_TOKEN_KEY, requestMalocaToken } from "./authPrompt";

// --- Maloca Domain Types ---

export interface SupportTicket {
  id: string;
  title: string;
  body: string;
  status: string;
  created_at: string;
  feature_id?: string;
}

export interface ReviewRequest {
  id: string;
  target: string;
  kind: string;
  notes: string;
  status: string;
  created_at: string;
}

export interface MicroTask {
  id: string;
  parent_feature: string;
  kind: string;
  title: string;
  acceptance: string;
  evidence_paths?: string[];
  reward_hint: number;
  difficulty: number;
  status: string;
}

export interface MeshTicketOffer {
  id: string;
  microtask: MicroTask;
  offered_at: string;
  expires_at: string;
  claimed_by?: string;
}

export interface RewardReceipt {
  ticket_id: string;
  xp: number;
  karma_delta: number;
  recorded_at: string;
}

export interface MalocaPack {
  generated_at: string;
  codegraph_indexed_at?: string;
  codegraph_head?: string;
  features_total: number;
  features_draft: number;
  gaps_zero_symbol_modules: string[];
  decisions_count: number;
  support_open: number;
  inbox_open: number;
}

export type ProposalStatus = "open" | "closed" | "reconsidering" | "analyzing";

export interface Proposal {
  id: string;
  type: string;
  title: string;
  body: string;
  status: ProposalStatus;
  created_at: string;
  locked_param?: boolean;
}

export type ManagerActionType = "request_reconsideration" | "request_scenario_analysis";

export interface ManagerAction {
  id: string;
  type: ManagerActionType;
  proposalId: string;
  reason: string;
  created_at: string;
}

export interface NetworkParam {
  key: string;
  default: string;
  locked_until_quorum: boolean;
  notes: string;
}

export interface CreateSupportBody {
  title: string;
  body: string;
  feature_id?: string;
}

export interface CreateProposalBody {
  type: string;
  title: string;
  body: string;
  locked_param?: boolean;
}

export interface MeshNodeInfo {
  node_id: string;
  role: string;
  note: string;
  karma: number;
  active: boolean;
}

export interface MeshInfo {
  id: string;
  kind: string;
  description: string;
}

export interface MeshSnapshot {
  mode: string;
  genesis_node_id: string;
  parent_nodes_enabled: boolean;
  manager_adds_vote_weight: boolean;
  wallet_multi_node_anchor: boolean;
  nodes: MeshNodeInfo[];
  meshes: MeshInfo[];
}

export type VoteChoice = "yes" | "no" | "abstain";

export interface Vote {
  id: string;
  proposal_id: string;
  node_id: string;
  choice: VoteChoice;
  weight: number;
  created_at: string;
}

export interface CastVoteBody {
  node_id?: string;
  choice: VoteChoice;
}

export interface DecisionEvent {
  id: string;
  kind: string;
  proposal_id?: string;
  actor_node_id: string;
  genesis_node_id: string;
  payload: any;
  created_at: string;
}

export interface NodeRecord {
  node_id: string;
  role: string;
  karma: number;
  active: boolean;
  note: string;
}

export interface BacklogItem {
  id: string;
  title: string;
  status: string;
  progress_pct: number;
  notes: string;
  repo_name: string;
}

export interface BacklogResponse {
  source: string;
  items: BacklogItem[];
}

// --- Introspection Types (mirror src/server/maloca/introspection_routes.rs + src/humanchallenge/types.rs) ---

export type ChallengeType =
  | "contradiction"
  | "decision"
  | "execution"
  | "assumption"
  | "clarification";

export type IntrospectionTechnique =
  | "socratic_questioning"
  | "five_whys"
  | "pre_mortem"
  | "steel_manning"
  | "first_principles"
  | "pattern_recognition";

export type IntrospectionStatus = "active" | "completed" | "abandoned";
export type TurnRole = "llm_guide" | "human";

export interface IntrospectionTurn {
  role: TurnRole;
  content: string;
  timestamp: string;
}

export interface IntrospectionSession {
  id: string;
  challenge_id: string;
  technique: IntrospectionTechnique;
  turns: IntrospectionTurn[];
  depth_score: number;
  insights: string[];
  status: IntrospectionStatus;
  started_at: string;
  completed_at: string | null;
}

/** Candidate challenge as returned by GET /v1/maloca/introspection/available. */
export interface HumanChallengeEvent {
  id: string;
  session_id: string;
  challenge_type: ChallengeType;
  description: string;
  raw_content: string;
  confidence_score: number;
  status: string;
  created_at: string;
  answered_at?: string | null;
  response?: string | null;
  points_awarded?: number;
  privacy_p4_local_only?: boolean;
}

export interface IntrospectionAvailableResponse {
  techniques: string[];
  challenges?: HumanChallengeEvent[];
}

export interface StartIntrospectionRequest {
  challenge_id: string;
  challenge_type: ChallengeType;
  description: string;
  technique?: IntrospectionTechnique | null;
}

export interface ProcessTurnRequest {
  human_input: string;
  challenge_description: string;
}

/** start/turn/complete/get all answer `{ session }`. */
export interface IntrospectionSessionResponse {
  session: IntrospectionSession;
}

// --- API Client Fetch Helpers ---

// The server mounts the legacy ops tree at `/maloca/*` and the v1 tree
// (registry, backlog/unified, challenges, introspection, models) at `/v1/maloca/*`.
// Nothing is served under `/api/v1/maloca` (src/cli/server.rs, src/maloca/mod.rs).
export const MALOCA_OPS_PREFIX = "/maloca";
export const MALOCA_V1_PREFIX = "/v1/maloca";

async function fetchMalocaOnce(url: string, options?: RequestInit): Promise<Response> {
  const activeWorkspace = typeof localStorage !== "undefined"
    ? localStorage.getItem("xavier_active_workspace") || "default"
    : "default";

  const token = typeof localStorage !== "undefined" ? localStorage.getItem(AUTH_TOKEN_KEY) : null;

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    "X-Workspace-Id": activeWorkspace,
  };

  if (token) {
    headers["X-Xavier-Token"] = token;
  }

  return fetch(url, {
    ...options,
    headers: {
      ...headers,
      ...(options?.headers || {})
    }
  });
}

async function fetchMaloca<T>(
  endpoint: string,
  options?: RequestInit,
  prefix: string = MALOCA_OPS_PREFIX,
): Promise<T> {
  const url = getApiUrl(`${prefix}${endpoint}`);

  let response = await fetchMalocaOnce(url, options);

  // 401: ask the user for a token (stored on this device only) and retry once.
  if (response.status === 401 && typeof localStorage !== "undefined") {
    const token = await requestMalocaToken();
    if (token) {
      response = await fetchMalocaOnce(url, options);
    }
  }

  if (!response.ok) {
    let errorMsg = `HTTP Error ${response.status}`;
    const errorText = await response.text().catch(() => "");
    if (errorText) {
      try {
        const errorJson = JSON.parse(errorText);
        errorMsg = errorJson.message || errorJson.error || errorMsg;
      } catch {
        errorMsg = errorText;
      }
    }
    throw new Error(errorMsg);
  }

  return response.json() as Promise<T>;
}

// --- Specific Endpoints ---

export const malocaApi = {
  getPack: () => fetchMaloca<MalocaPack>("/pack"),

  getBacklog: (appId?: string) => {
    const query = appId ? `?app_id=${encodeURIComponent(appId)}` : "";
    return fetchMaloca<BacklogResponse>(`/backlog${query}`);
  },

  getSupportTickets: () => fetchMaloca<SupportTicket[]>("/support"),
  createSupportTicket: (body: CreateSupportBody) => fetchMaloca<SupportTicket>("/support", {
    method: "POST",
    body: JSON.stringify(body)
  }),

  getProposals: () => fetchMaloca<Proposal[]>("/proposals"),
  createProposal: (body: CreateProposalBody) => fetchMaloca<Proposal>("/proposals", {
    method: "POST",
    body: JSON.stringify(body)
  }),

  getVotes: (proposalId?: string) => {
    const query = proposalId ? `?proposal_id=${encodeURIComponent(proposalId)}` : "";
    return fetchMaloca<Vote[]>(`/votes${query}`);
  },
  castVote: (proposalId: string, body: CastVoteBody) => fetchMaloca<Vote>(`/proposals/${encodeURIComponent(proposalId)}/vote`, {
    method: "POST",
    body: JSON.stringify(body)
  }),

  getMesh: () => fetchMaloca<MeshSnapshot>("/mesh"),
  getNodes: () => fetchMaloca<NodeRecord[]>("/nodes"),

  getDecisions: () => fetchMaloca<DecisionEvent[]>("/decisions"),
  getParams: () => fetchMaloca<NetworkParam[]>("/params"),

  getReviews: () => fetchMaloca<ReviewRequest[]>("/reviews"),

  getInbox: () => fetchMaloca<MeshTicketOffer[]>("/inbox"),
  claimTicket: (ticketId: string, nodeId?: string) => fetchMaloca<MeshTicketOffer>(`/inbox/${encodeURIComponent(ticketId)}/claim`, {
    method: "POST",
    body: JSON.stringify({ node_id: nodeId || "local" })
  }),
  completeTicket: (ticketId: string) => fetchMaloca<RewardReceipt>(`/inbox/${encodeURIComponent(ticketId)}/complete`, {
    method: "POST"
  }),

  getRewards: () => fetchMaloca<RewardReceipt[]>("/rewards"),

  // --- Introspection (v1 tree) ---
  getIntrospectionAvailable: () =>
    fetchMaloca<IntrospectionAvailableResponse>("/introspection/available", undefined, MALOCA_V1_PREFIX),
  startIntrospection: (body: StartIntrospectionRequest) =>
    fetchMaloca<IntrospectionSessionResponse>(
      "/introspection/start",
      { method: "POST", body: JSON.stringify(body) },
      MALOCA_V1_PREFIX,
    ),
  introspectionTurn: (sessionId: string, body: ProcessTurnRequest) =>
    fetchMaloca<IntrospectionSessionResponse>(
      `/introspection/${encodeURIComponent(sessionId)}/turn`,
      { method: "POST", body: JSON.stringify(body) },
      MALOCA_V1_PREFIX,
    ),
  completeIntrospection: (sessionId: string) =>
    fetchMaloca<IntrospectionSessionResponse>(
      `/introspection/${encodeURIComponent(sessionId)}/complete`,
      { method: "POST" },
      MALOCA_V1_PREFIX,
    ),
  getIntrospection: (sessionId: string) =>
    fetchMaloca<IntrospectionSessionResponse>(
      `/introspection/${encodeURIComponent(sessionId)}`,
      undefined,
      MALOCA_V1_PREFIX,
    ),
};
