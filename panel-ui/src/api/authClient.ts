import { getApiUrl } from "./client";
import type { User } from "../types";

export interface RegisterResponse {
  user: User;
  seed_phrase: string;
}

export interface LoginResponse {
  user: User;
  token: string;
  refresh_token: string;
  requires_2fa?: boolean;
}

export interface TwoFactorSetupResponse {
  qr_code: string;
  secret: string;
  backup_codes: string[];
}

export class AuthClient {
  private async fetch<T>(path: string, options?: RequestInit): Promise<T> {
    const response = await fetch(getApiUrl(path), {
      ...options,
      headers: {
        "Content-Type": "application/json",
        ...(options?.headers ?? {}),
      },
      credentials: 'include',
    });

    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(errorText || response.statusText);
    }
    return (await response.json()) as T;
  }

  async login(email: string, password: string, totp_code?: string): Promise<LoginResponse> {
    return this.fetch<LoginResponse>("/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password, totp_code }),
    });
  }

  async register(email: string, name: string, password: string): Promise<RegisterResponse> {
    return this.fetch<RegisterResponse>("/auth/register", {
      method: "POST",
      body: JSON.stringify({ email, name, password }),
    });
  }

  async logout(): Promise<void> {
    await this.fetch("/auth/logout", { method: "POST" });
  }

  async refresh(): Promise<LoginResponse> {
    return this.fetch<LoginResponse>("/auth/refresh", { method: "POST" });
  }

  async setup2FA(): Promise<TwoFactorSetupResponse> {
    return this.fetch<TwoFactorSetupResponse>("/auth/2fa/setup", { method: "POST" });
  }

  async verify2FA(code: string): Promise<{ status: string }> {
    return this.fetch<{ status: string }>("/auth/2fa/verify", {
      method: "POST",
      body: JSON.stringify({ code }),
    });
  }

  async recover(email: string, seed_phrase: string, new_password: string): Promise<void> {
    await this.fetch("/auth/recovery", {
      method: "POST",
      body: JSON.stringify({ email, seed_phrase, new_password }),
    });
  }
}

export const authClient = new AuthClient();
