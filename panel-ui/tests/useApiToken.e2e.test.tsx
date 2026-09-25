import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getApiUrl } from "../src/api/client";
import { useAuthStore } from "../src/auth/AuthProvider";
import { getApiTokenSync, useApiToken } from "../src/hooks/useApiToken";

describe("useApiToken hook and getApiTokenSync helper", () => {
  const originalState = useAuthStore.getState();

  beforeEach(() => {
    vi.resetAllMocks();
    useAuthStore.setState({ token: null });
  });

  afterEach(() => {
    useAuthStore.setState(originalState);
    vi.unstubAllEnvs();
  });

  it("(a) returns useAuthStore token when store token is present", () => {
    useAuthStore.setState({ token: "custom-store-token-123" });
    const { result } = renderHook(() => useApiToken());
    expect(result.current).toBe("custom-store-token-123");
    expect(getApiTokenSync()).toBe("custom-store-token-123");
  });

  // SECURITY: useApiToken/getApiTokenSync must source the token exclusively from the /auth
  // session (useAuthStore's `token`, set from the operator's JWT on login/refresh) and must
  // NEVER fall back to a VITE_*-prefixed build-time env var — that gets inlined as plaintext
  // into the production JS bundle by Vite. A prior panel deploy shipped exactly that
  // (VITE_XAVIER_API_TOKEN baked into the built assets), a real credential-leak vector. This
  // asserts an env var of that shape has no effect whatsoever, even when set.
  it("(b) ignores a VITE_XAVIER_API_TOKEN env var entirely — never falls back to it", () => {
    vi.stubEnv("VITE_XAVIER_API_TOKEN", "env-token-should-be-ignored");
    useAuthStore.setState({ token: null });
    const { result } = renderHook(() => useApiToken());
    expect(result.current).toBe("");
    expect(getApiTokenSync()).toBe("");
  });

  it("(c) getApiTokenSync returns token synchronously outside React component lifecycle", () => {
    useAuthStore.setState({ token: "sync-token-456" });
    expect(getApiTokenSync()).toBe("sync-token-456");
  });

  it("(d) fetch attaching token header using getApiUrl resolves 200 without 401 loop or Tauri dependency", async () => {
    useAuthStore.setState({ token: "valid-test-token" });

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({ status: "ok", memories: [] }),
    });
    vi.stubGlobal("fetch", fetchMock);

    const tokenFromHook = getApiTokenSync();
    expect(tokenFromHook).toBe("valid-test-token");

    const response = await fetch(getApiUrl("/v1/memories?limit=1"), {
      headers: { "X-Xavier-Token": tokenFromHook },
    });

    expect(response.status).toBe(200);
    expect(fetchMock).toHaveBeenCalledWith(
      "/v1/memories?limit=1",
      expect.objectContaining({
        headers: { "X-Xavier-Token": "valid-test-token" },
      }),
    );
  });

  it("(e) falls back to empty string when no /auth session token is present", () => {
    useAuthStore.setState({ token: null });
    const { result } = renderHook(() => useApiToken());
    expect(result.current).toBe("");
    expect(getApiTokenSync()).toBe("");
  });
});
