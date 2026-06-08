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
    return this.fetch<{ status: string; message: string }>("/v1/config/providers", {
      method: "PUT",
      body: JSON.stringify({ providers }),
    });
  }

  async testProvider(name: string) {
    return this.fetch<{ status: string; message: string }>(`/v1/providers/${name}/test`, {
      method: "POST",
    });
  }

  // System
  async systemScan() {
    return this.fetch<SystemScan>("/v1/system/scan");
  }

  // Quota
  async getProvidersQuota() {
    return this.fetch<ProviderQuota[]>("/v1/providers/quota");
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
