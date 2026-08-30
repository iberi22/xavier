import { getApiUrl } from "./client";

export interface MeshNetwork {
  id: string;
  name: string;
  template: "enterprise" | "dao" | "health";
  is_host: boolean;
  status: "active" | "syncing" | "idle";
  created_at: number;
}

export interface CreateNetworkPayload {
  name: string;
  template: "enterprise" | "dao" | "health";
  is_host: boolean;
}

export interface DaoProposal {
  id: string;
  title: string;
  description: string;
  category: string;
  for_votes: number;
  against_votes: number;
  abstain_votes: number;
  quorum_pct: number;
  status: "active" | "passed" | "rejected";
}

export interface MeshChatMessage {
  id: string;
  sender_node: string;
  recipient_or_room: string;
  ciphertext: string;
  timestamp: number;
  encrypted: boolean;
}

export class MeshApiClient {
  private token: string;

  constructor(token: string) {
    this.token = token;
  }

  private async fetch<T>(path: string, options?: RequestInit): Promise<T> {
    const response = await fetch(getApiUrl(path), {
      ...options,
      headers: {
        "Content-Type": "application/json",
        "X-Xavier-Token": this.token,
        ...(options?.headers ?? {}),
      },
    });

    if (!response.ok) {
      throw new Error(await response.text());
    }
    return (await response.json()) as T;
  }

  async listNetworks(): Promise<MeshNetwork[]> {
    return this.fetch<MeshNetwork[]>("/v1/mesh/networks");
  }

  async createNetwork(payload: CreateNetworkPayload): Promise<MeshNetwork> {
    return this.fetch<MeshNetwork>("/v1/mesh/networks", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async getInviteTicket(networkId: string): Promise<{ ticket: string; qr_data: string }> {
    return this.fetch<{ ticket: string; qr_data: string }>(`/v1/mesh/networks/${networkId}/invite`);
  }

  async revokePeer(networkId: string, peerId: string): Promise<{ purged: boolean; timestamp: number }> {
    return this.fetch<{ purged: boolean; timestamp: number }>(`/v1/mesh/networks/${networkId}/peers/${peerId}/revoke`, {
      method: "POST",
    });
  }

  async listDaoProposals(): Promise<DaoProposal[]> {
    return this.fetch<DaoProposal[]>("/v1/mesh/dao/proposals");
  }

  async submitDaoVote(proposalId: string, ballot: "for" | "against" | "abstain"): Promise<{ status: string }> {
    return this.fetch<{ status: string }>(`/v1/mesh/dao/proposals/${proposalId}/vote`, {
      method: "POST",
      body: JSON.stringify({ ballot }),
    });
  }

  async sendChatMessage(payload: { recipient_or_room: string; ciphertext: string }): Promise<MeshChatMessage> {
    return this.fetch<MeshChatMessage>("/v1/mesh/chat/send", {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }

  async getChatHistory(target: string): Promise<MeshChatMessage[]> {
    return this.fetch<MeshChatMessage[]>(`/v1/mesh/chat/history/${encodeURIComponent(target)}`);
  }

  async shareHealthRecord(recordId: string, payload: { ttl_seconds?: number; read_once?: boolean }): Promise<{ pass_token: string; expires_at: number }> {
    return this.fetch<{ pass_token: string; expires_at: number }>(`/v1/mesh/health/records/${recordId}/share-pass`, {
      method: "POST",
      body: JSON.stringify(payload),
    });
  }
}
