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

export const getApiUrl = (path: string) => {
  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  return isTauri ? `http://127.0.0.1:8006${path}` : path;
};

/**
 * Parse Retry-After header into seconds.
 * Handles integer seconds, float seconds, and HTTP Date strings.
 */
export function parseRetryAfter(headerValue: string | null): number | null {
  if (!headerValue) return null;
  const trimmed = headerValue.trim();
  if (!trimmed) return null;

  const parsedNum = Number(trimmed);
  if (!isNaN(parsedNum) && parsedNum >= 0) {
    return Math.ceil(parsedNum);
  }

  const parsedDate = Date.parse(trimmed);
  if (!isNaN(parsedDate)) {
    const diffMs = parsedDate - Date.now();
    return Math.max(1, Math.ceil(diffMs / 1000));
  }

  return null;
}

/**
 * Parse X-RateLimit-Remaining header into integer count.
 */
export function parseRateLimitRemaining(headerValue: string | null): number | null {
  if (!headerValue) return null;
  const parsed = parseInt(headerValue.trim(), 10);
  return isNaN(parsed) ? null : parsed;
}

export class RateLimitError extends Error {
  public readonly status = 429;
  public readonly retryAfterSeconds: number;
  public readonly remaining: number | null;

  constructor(
    message: string,
    retryAfterSeconds: number,
    remaining: number | null = null,
  ) {
    super(message);
    this.name = "RateLimitError";
    this.retryAfterSeconds = retryAfterSeconds;
    this.remaining = remaining;
  }
}

export interface ApiRequestOptions extends RequestInit {
  isBackground?: boolean;
  autoRetry?: boolean;
  maxRetries?: number;
  baseDelayMs?: number;
}

export class ApiClient {
  private token: string;
  private rateLimitUntil = 0;
  private backoffAttempts = 0;

  constructor(token: string) {
    this.token = token;
  }

  public getRateLimitCooldown(): number {
    const remainingMs = this.rateLimitUntil - Date.now();
    return remainingMs > 0 ? Math.ceil(remainingMs / 1000) : 0;
  }

  private async fetch<T>(path: string, options?: ApiRequestOptions): Promise<T> {
    const isBackground = options?.isBackground ?? false;
    const autoRetry = options?.autoRetry ?? isBackground;
    const maxRetries = options?.maxRetries ?? (autoRetry ? 3 : 0);
    const baseDelayMs = options?.baseDelayMs ?? 1000;

    let attempt = 0;

    while (attempt <= maxRetries) {
      // Respect active rate-limit cooldown before making automated attempts
      if (attempt > 0 || autoRetry) {
        const cooldownLeft = this.rateLimitUntil - Date.now();
        if (cooldownLeft > 0) {
          await new Promise((resolve) => setTimeout(resolve, cooldownLeft));
        }
      }

      try {
        const response = await fetch(getApiUrl(path), {
          ...options,
          headers: {
            "Content-Type": "application/json",
            "X-Xavier-Token": this.token,
            ...(options?.headers ?? {}),
          },
        });

        if (response.status === 429) {
          const retryAfterHeader =
            response.headers?.get("Retry-After") ??
            response.headers?.get("retry-after") ??
            null;
          const remainingHeader =
            response.headers?.get("X-RateLimit-Remaining") ??
            response.headers?.get("x-ratelimit-remaining") ??
            null;

          const parsedRetryAfter = parseRetryAfter(retryAfterHeader);
          const remaining = parseRateLimitRemaining(remainingHeader);

          const exponentialDelaySec = Math.ceil(
            (baseDelayMs * Math.pow(2, this.backoffAttempts)) / 1000,
          );
          const retryAfterSeconds = parsedRetryAfter ?? Math.max(5, exponentialDelaySec);

          const cooldownMs = retryAfterSeconds * 1000;
          this.rateLimitUntil = Date.now() + cooldownMs;
          this.backoffAttempts += 1;

          const responseText = await response.text().catch(() => "Rate limit exceeded");
          const errorMessage =
            responseText || `Rate limit exceeded. Retry in ${retryAfterSeconds}s`;

          if (typeof window !== "undefined" && window.dispatchEvent) {
            window.dispatchEvent(
              new CustomEvent("xavier-rate-limit", {
                detail: {
                  message: errorMessage,
                  retryAfterSeconds,
                  remaining,
                  type: "rate-limit",
                },
              }),
            );
          }

          const rateLimitError = new RateLimitError(
            errorMessage,
            retryAfterSeconds,
            remaining,
          );

          if (attempt < maxRetries) {
            attempt += 1;
            await new Promise((resolve) => setTimeout(resolve, cooldownMs));
            continue;
          }

          if (isBackground) {
            return null as unknown as T;
          }

          throw rateLimitError;
        }

        if (!response.ok) {
          throw new Error(await response.text());
        }

        this.backoffAttempts = 0;
        return (await response.json()) as T;
      } catch (err) {
        if (err instanceof RateLimitError) {
          if (isBackground) {
            return null as unknown as T;
          }
          throw err;
        }

        if (attempt < maxRetries) {
          attempt += 1;
          const delayMs = baseDelayMs * Math.pow(2, attempt);
          await new Promise((resolve) => setTimeout(resolve, delayMs));
          continue;
        }

        if (isBackground) {
          return null as unknown as T;
        }
        throw err;
      }
    }

    return null as unknown as T;
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
