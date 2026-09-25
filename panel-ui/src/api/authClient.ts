import type { User } from "../types";
import { getApiUrl } from "./client";

export interface RegisterResponse {
  user: User;
  seed_phrase: string;
}

export interface LoginResponse {
  user: User;
  access_token: string;
  refresh_token: string;
  requires_2fa?: boolean;
}

export interface RefreshResponse {
  access_token: string;
  refresh_token: string;
}

export interface TwoFactorSetupResponse {
  qr_code: string;
  secret: string;
  backup_codes: string[];
}

/**
 * Structured auth error. Backend handlers (src/auth2/mod.rs, `json_err`) return
 * `{ "error": "<code>", "message": "<human text>" }` for known failure cases —
 * `code` lets callers react to specific conditions (e.g. `mfa_required`) instead
 * of pattern-matching an arbitrary message string. Some failures (auth_middleware
 * rejections, unhandled panics) still come back as a bare status with no body,
 * in which case `code` is left undefined.
 */
export class AuthApiError extends Error {
  code?: string;

  constructor(message: string, code?: string) {
    super(message);
    this.name = "AuthApiError";
    this.code = code;
  }
}

export class AuthClient {
  private async fetch<T>(path: string, options?: RequestInit): Promise<T> {
    const response = await fetch(getApiUrl(path), {
      ...options,
      headers: {
        "Content-Type": "application/json",
        ...(options?.headers ?? {}),
      },
      credentials: "include",
    });

    if (!response.ok) {
      const errorText = await response.text();
      let code: string | undefined;
      let message = errorText || response.statusText;
      try {
        const parsed = JSON.parse(errorText) as { error?: string; message?: string };
        if (parsed && typeof parsed === "object") {
          code = typeof parsed.error === "string" ? parsed.error : undefined;
          message =
            typeof parsed.message === "string"
              ? parsed.message
              : (code ?? message);
        }
      } catch {
        // Not JSON — some handlers (e.g. auth_middleware) return a bare status
        // with no body; keep the raw text/statusText as the message.
      }
      throw new AuthApiError(message, code);
    }
    // Some handlers return a bare 200/204 with no body at all (e.g. logout_handler,
    // src/auth2/mod.rs, just `Ok(StatusCode::OK)`). Calling response.json() directly
    // on an empty body throws "Unexpected end of JSON input" — which, uncaught, was
    // aborting AuthProvider's `logout()` before it ever reset `isAuthenticated`.
    const text = await response.text();
    return (text ? JSON.parse(text) : (undefined as unknown)) as T;
  }

  async login(
    email: string,
    password: string,
    totp_code?: string,
  ): Promise<LoginResponse> {
    return this.fetch<LoginResponse>("/auth/login", {
      method: "POST",
      body: JSON.stringify({ email, password, totp_code }),
    });
  }

  async register(
    email: string,
    name: string,
    password: string,
  ): Promise<RegisterResponse> {
    return this.fetch<RegisterResponse>("/auth/register", {
      method: "POST",
      body: JSON.stringify({ email, name, password }),
    });
  }

  // Backend `logout_handler` (src/auth2/mod.rs) expects a JSON body with the
  // refresh token to revoke — it is a plain `Json<RefreshRequest>` extractor,
  // not cookie-based, so a bodyless POST fails deserialization before it ever
  // reaches the revocation logic.
  async logout(refreshToken: string | null): Promise<void> {
    await this.fetch("/auth/logout", {
      method: "POST",
      body: JSON.stringify({ refresh_token: refreshToken ?? "" }),
    });
  }

  // Same as logout: `refresh_handler` requires `{ refresh_token }` in the body.
  async refresh(refreshToken: string | null): Promise<RefreshResponse> {
    return this.fetch<RefreshResponse>("/auth/refresh", {
      method: "POST",
      body: JSON.stringify({ refresh_token: refreshToken ?? "" }),
    });
  }

  // `/auth/2fa/setup` and `/auth/2fa/verify` are behind `auth_middleware`
  // (src/auth2/middleware.rs), which requires a strict `Authorization: Bearer
  // <jwt>` header — there is no cookie fallback, so the operator's access
  // token must be attached explicitly.
  async setup2FA(accessToken: string | null): Promise<TwoFactorSetupResponse> {
    return this.fetch<TwoFactorSetupResponse>("/auth/2fa/setup", {
      method: "POST",
      headers: accessToken ? { Authorization: `Bearer ${accessToken}` } : {},
    });
  }

  async verify2FA(
    accessToken: string | null,
    code: string,
  ): Promise<{ status: string }> {
    return this.fetch<{ status: string }>("/auth/2fa/verify", {
      method: "POST",
      headers: accessToken ? { Authorization: `Bearer ${accessToken}` } : {},
      body: JSON.stringify({ code }),
    });
  }

  async recover(
    email: string,
    seed_phrase: string,
    new_password: string,
  ): Promise<void> {
    await this.fetch("/auth/recovery", {
      method: "POST",
      body: JSON.stringify({ email, seed_phrase, new_password }),
    });
  }
}

export const authClient = new AuthClient();
