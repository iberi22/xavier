import type {
  Agent,
  ClearanceLevel,
  MemoryEntry,
  MeshRole,
  MeshStatus,
  PairingCodeResponse,
  SecretAuditLog,
  SecretLease,
} from "../types";

export const REMOTE_URL_KEY = "xavier_remote_url";

export const getRemoteUrl = (): string => {
  if (typeof window === "undefined") return "";
  return localStorage.getItem(REMOTE_URL_KEY) || "";
};

export const setRemoteUrl = (url: string | null): void => {
  if (typeof window === "undefined") return;
  if (url && url.trim().length > 0) {
    localStorage.setItem(REMOTE_URL_KEY, url.trim());
  } else {
    localStorage.removeItem(REMOTE_URL_KEY);
  }
};

export const getApiUrl = (path: string) => {
  if (typeof window !== "undefined") {
    const remoteUrl = localStorage.getItem(REMOTE_URL_KEY);
    if (remoteUrl && remoteUrl.trim().length > 0) {
      const cleanRemote = remoteUrl.trim().replace(/\/+$/, "");
      const cleanPath = path.startsWith("/") ? path : `/${path}`;
      return `${cleanRemote}${cleanPath}`;
    }
  }
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

  // Cloud relay
  async getCloudNode() {
    const data = await this.fetch<PgHeartSettings>("/v1/mesh/cloud");
    return {
      status: "ok",
      data: {
        url: data.url ?? "",
        token: data.token ?? "",
        instance_id: data.instance_id ?? "",
        sync_interval_ms: data.sync_interval_ms ?? 300000,
        auto_heartbeat: data.auto_heartbeat ?? true,
      },
    };
  }

  async updateCloudNode(config: CloudNodeConfig) {
    return this.fetch<{ status: string }>("/v1/mesh/cloud", {
      method: "PUT",
      body: JSON.stringify(config),
    });
  }

  // Data Commons
  async getDataCommons() {
    const data = await this.fetch<DataCommonsConfig>(
      "/v1/mesh/data_commons/opt_in",
    );
    return { status: "ok", data };
  }

  async optInDataCommons(config: DataCommonsConfig) {
    return this.fetch<{ status: string }>("/v1/mesh/data_commons/opt_in", {
      method: "POST",
      body: JSON.stringify(config),
    });
  }

  // Mesh
  async getMeshStatus() {
    return this.fetch<MeshStatus>("/v1/mesh/peers");
  }

  async pairPeer(code: string) {
    return this.fetch<{ status: string; node_id: string }>(
      "/v1/mesh/peers/pair",
      {
        method: "POST",
        body: JSON.stringify({ code }),
      },
    );
  }

  async generatePairingCode(endpoint?: string) {
    return this.fetch<PairingCodeResponse>("/v1/mesh/peers/generate-code", {
      method: "POST",
      body: JSON.stringify({ endpoint }),
    });
  }

  async updatePeerAcl(
    nodeId: string,
    role: MeshRole,
    clearance: ClearanceLevel,
  ) {
    return this.fetch<{ status: string }>(`/v1/mesh/peers/${nodeId}/acl`, {
      method: "PUT",
      body: JSON.stringify({ role, clearance }),
    });
  }

  async removePeer(nodeId: string) {
    return this.fetch<{ status: string }>(`/v1/mesh/peers/${nodeId}`, {
      method: "DELETE",
    });
  }

  // Secrets & Leases
  async getLeases() {
    return this.fetch<SecretLease[]>("/secrets/leases");
  }

  async revokeLease(token: string) {
    return this.fetch<{ status: string }>("/secrets/revoke", {
      method: "POST",
      body: JSON.stringify({ token }),
    });
  }

  async getLeaseHistory() {
    return this.fetch<SecretAuditLog[]>("/secrets/history");
  }

  // Ollama Model Manager (Ola 4 · 02/04) — routes from ollama_models handlers
  async getOllamaModels() {
    // Backend may return raw Ollama /api/tags shape: { models: [{ name, ... }] }
    return this.fetch<{ models?: Array<string | { name?: string }> }>(
      "/v1/ollama/models",
    );
  }

  async pullOllamaModel(name: string) {
    return this.fetch<Record<string, unknown>>("/v1/ollama/pull", {
      method: "POST",
      body: JSON.stringify({ name }),
    });
  }

  async getOllamaActive() {
    return this.fetch<{
      llm?: string;
      embedding?: string | null;
      model?: string;
    }>("/v1/ollama/active");
  }

  async setOllamaActive(model: string, kind: "llm" | "embedding") {
    return this.fetch<{
      ok?: boolean;
      success?: boolean;
      model?: string;
      kind?: string;
      error?: string;
    }>("/v1/ollama/active", {
      method: "POST",
      body: JSON.stringify({ model, kind }),
    });
  }

  // Offline Models
  async getOfflineConfig() {
    return this.fetch<{
      local_model_dirs: string[];
      auto_start_last_model: boolean;
    }>("/v1/offline/config");
  }

  async updateOfflineConfig(config: {
    local_model_dirs: string[];
    auto_start_last_model: boolean;
  }) {
    return this.fetch<{ status: string; message: string }>(
      "/v1/offline/config",
      {
        method: "POST",
        body: JSON.stringify(config),
      },
    );
  }

  async getOfflineModels() {
    return this.fetch<{
      models: Array<{
        name: string;
        path: string;
        size_bytes: number;
        quantization: string | null;
      }>;
    }>("/v1/offline/models");
  }

  async getOfflineStatus() {
    return this.fetch<{
      gpu_detected: boolean;
      gpu_vendor: string;
      vram_mb: number;
      engine_status: string;
      active_model: string;
      port: number;
    }>("/v1/offline/status");
  }

  async downloadOfflineModel(url: string) {
    return this.fetch<{
      status: string;
      message: string;
      filename: string;
      path: string;
    }>("/v1/offline/download", {
      method: "POST",
      body: JSON.stringify({ url }),
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

interface PgHeartSettings {
  url?: string | null;
  token?: string | null;
  instance_id?: string | null;
  sync_interval_ms?: number;
  auto_heartbeat?: boolean;
}

export interface CloudNodeConfig {
  url: string;
  token: string;
  instance_id: string;
  sync_interval_ms?: number;
  auto_heartbeat?: boolean;
}

export interface DataCommonsConfig {
  enabled: boolean;
  consent_given: boolean;
  wallet_address?: string | null;
}
