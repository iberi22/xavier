import type {
  DecisionEvent,
  MalocaPack,
  ManagerAction,
  MeshSnapshot,
  MeshTicketOffer,
  NetworkParam,
  NodeRecord,
  Proposal,
  SupportTicket,
  Vote,
  VoteChoice,
} from "./types";

const getApiUrl = (path: string) => {
  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  return isTauri ? `http://127.0.0.1:8006${path}` : path;
};

async function json<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(getApiUrl(path), {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
  });
  if (!r.ok) {
    const detail = await r.text().catch(() => "");
    throw new Error(
      detail.trim()
        ? `${r.status} ${path}: ${detail.trim()}`
        : `${r.status} ${path}`,
    );
  }
  return r.json() as Promise<T>;
}

export const malocaApi = {
  pack: () => json<MalocaPack>("/maloca/pack"),
  backlog: () => json<unknown>("/maloca/backlog"),
  listSupport: () => json<SupportTicket[]>("/maloca/support"),
  createSupport: (body: { title: string; body: string }) =>
    json<SupportTicket>("/maloca/support", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  listInbox: () => json<MeshTicketOffer[]>("/maloca/inbox"),
  claim: (id: string, node_id = "local") =>
    json<MeshTicketOffer>(`/maloca/inbox/${id}/claim`, {
      method: "POST",
      body: JSON.stringify({ node_id }),
    }),
  complete: (id: string) =>
    json<unknown>(`/maloca/inbox/${id}/complete`, { method: "POST" }),
  mesh: () => json<MeshSnapshot>("/maloca/mesh"),
  listNodes: () => json<NodeRecord[]>("/maloca/nodes"),
  params: () => json<NetworkParam[]>("/maloca/params"),
  listProposals: () => json<Proposal[]>("/maloca/proposals"),
  createProposal: (body: {
    type: string;
    title: string;
    body: string;
    locked_param?: boolean;
  }) =>
    json<Proposal>("/maloca/proposals", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  listVotes: (proposalId?: string) =>
    json<Vote[]>(
      proposalId
        ? `/maloca/votes?proposal_id=${encodeURIComponent(proposalId)}`
        : "/maloca/votes",
    ),
  castVote: (proposalId: string, body: { node_id: string; choice: VoteChoice }) =>
    json<Vote>(`/maloca/proposals/${proposalId}/vote`, {
      method: "POST",
      body: JSON.stringify(body),
    }),
  listDecisions: () => json<DecisionEvent[]>("/maloca/decisions"),
  listManagerActions: () => json<ManagerAction[]>("/maloca/manager-actions"),
  managerAction: (body: {
    type: ManagerAction["type"];
    proposalId: string;
    reason: string;
  }) =>
    json<ManagerAction>("/maloca/manager-actions", {
      method: "POST",
      body: JSON.stringify(body),
    }),
};
