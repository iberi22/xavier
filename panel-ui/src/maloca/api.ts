import { getApiUrl } from "../api/client";

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

// --- API Client Fetch Helpers ---

async function fetchMaloca<T>(endpoint: string, options?: RequestInit): Promise<T> {
  // Using the `/api/v1/maloca/*` prefix we mounted in Rust
  const url = getApiUrl(`/api/v1/maloca${endpoint}`);
  const activeWorkspace = typeof localStorage !== "undefined"
    ? localStorage.getItem("xavier_active_workspace") || "default"
    : "default";

  const token = typeof localStorage !== "undefined" ? localStorage.getItem("auth_token") : null;

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    "X-Workspace-Id": activeWorkspace,
  };

  if (token) {
    headers["X-Xavier-Token"] = token;
  }

  const response = await fetch(url, {
    ...options,
    headers: {
      ...headers,
      ...(options?.headers || {})
    }
  });

  if (!response.ok) {
    let errorMsg = `HTTP Error ${response.status}`;
    try {
      const errorJson = await response.json();
      errorMsg = errorJson.message || errorMsg;
    } catch {
      // Fallback to text if JSON parsing fails
      const errorText = await response.text();
      if (errorText) errorMsg = errorText;
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
};
