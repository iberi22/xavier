import type { Agent, MemoryEntry } from "../types";

const getApiUrl = (path: string) => {
  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  return isTauri ? `http://127.0.0.1:8006${path}` : path;
};

export class ApiClient {
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

  // Provider Config
  async getProvidersConfig() {
    return this.fetch<{ providers: ProviderConfig[] }>("/v1/config/providers");
  }

  async updateProvidersConfig(providers: ProviderConfig[]) {
    return this.fetch<{ status: string; message: string }>(
      "/v1/config/providers",
      {
        method: "PUT",
        body: JSON.stringify({ providers }),
      },
    );
  }

  async testProvider(name: string) {
    return this.fetch<{ status: string; message: string }>(
      `/v1/providers/${name}/test`,
      {
        method: "POST",
      },
    );
  }

  // System
  async systemScan() {
    return this.fetch<SystemScan>("/v1/system/scan");
  }

  // Quota
  async getProvidersQuota() {
    return this.fetch<ProviderQuota[]>("/v1/providers/quota");
  }

  // Memory
  async searchMemories(query: string, kind?: string, limit = 20) {
    const params = new URLSearchParams({ q: query, limit: String(limit) });
    if (kind) params.set("kind", kind);
    return this.fetch<MemoryEntry[]>(`/api/memory/search?${params}`);
  }

  async addMemory(
    content: string,
    kind = "note",
    priority = "medium",
    source = "panel-ui",
  ) {
    return this.fetch<MemoryEntry>("/api/memory/add", {
      method: "POST",
      body: JSON.stringify({ content, kind, priority, source }),
    });
  }

  // Agents
  async getAgents() {
    return this.fetch<Agent[]>("/api/agents");
  }

  // Mesh
  async getMeshIdentity() {
    return this.fetch<MeshIdentity>("/v1/mesh/identity");
  }

  async getPeers() {
    return this.fetch<PeerInfo[]>("/v1/mesh/peers");
  }

  async addPeer(peer: PeerInfo) {
    return this.fetch<{ status: string }>("/v1/mesh/peers", {
      method: "POST",
      body: JSON.stringify(peer),
    });
  }

  async removePeer(nodeId: string) {
    return this.fetch<{ status: string }>(`/v1/mesh/peers/${nodeId}`, {
      method: "DELETE",
    });
  }

  async generatePairingCode(endpoint?: string) {
    return this.fetch<{ code: string; secret: string }>(
      "/v1/mesh/pairing/generate",
      {
        method: "POST",
        body: JSON.stringify({ endpoint }),
      },
    );
  }

  async joinMesh(code: string) {
    return this.fetch<{ status: string; node_id: string }>(
      "/v1/mesh/pairing/join",
      {
        method: "POST",
        body: JSON.stringify({ code }),
      },
    );
  }

  // Data Commons
  async getDataCommons() {
    return this.fetch<{ status: string; data: DataCommonsConfig }>(
      "/v1/mesh/data_commons/opt_in",
    );
  }

  async optInDataCommons(config: DataCommonsConfig) {
    return this.fetch<{ status: string }>("/v1/mesh/data_commons/opt_in", {
      method: "POST",
      body: JSON.stringify(config),
    });
  }

  // Cloud Node
  async getCloudNode() {
    return this.fetch<{ status: string; data: CloudNodeConfig }>(
      "/api/settings/cloud-node",
    );
  }

  async updateCloudNode(config: CloudNodeConfig) {
    return this.fetch<{ status: string }>("/api/settings/cloud-node", {
      method: "POST",
      body: JSON.stringify(config),
    });
  }
}

export interface ProviderConfig {
  provider: string;
  model: string;
  api_key?: string;
  base_url?: string;
}

export interface SystemScan {
  version: string;
  os: string;
  arch: string;
  providers: {
    name: string;
    configured: boolean;
    model: string;
  }[];
  workspace_id: string;
  memory_backend: string;
}

export interface ProviderQuota {
  provider: string;
  used_hourly: number;
  used_today: number;
  used_weekly: number;
  used_monthly: number;
  weekly_quota: number;
  cache_hits: number;
  rate_limited_until: string | null;
  last_update: string;
}

export interface MeshIdentity {
  node_id: string;
  public_key_hex: string;
}

export interface PeerInfo {
  node_id: string;
  alias?: string;
  endpoint_url: string;
  public_key_hex: string;
  added_at: number;
  last_seen_at?: number;
  sync_enabled: boolean;
  is_cloud: boolean;
}

export interface DataCommonsConfig {
  enabled: boolean;
  consent_given: boolean;
  wallet_address?: string;
}

export interface CloudNodeConfig {
  url: string;
  token: string;
  instance_id: string;
}
