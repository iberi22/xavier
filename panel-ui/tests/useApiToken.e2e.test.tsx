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

  it("(b) falls back to VITE_XAVIER_API_TOKEN when store token is null", () => {
    vi.stubEnv("VITE_XAVIER_API_TOKEN", "env-token-xyz");
    useAuthStore.setState({ token: null });
    const { result } = renderHook(() => useApiToken());
    expect(result.current).toBe("env-token-xyz");
    expect(getApiTokenSync()).toBe("env-token-xyz");
  });

  it("(c) getApiTokenSync returns token synchronously outside React component lifecycle", () => {
    useAuthStore.setState({ token: "sync-token-456" });
    expect(getApiTokenSync()).toBe("sync-token-456");
  });

  it("(d) fetch attaching token header using getApiUrl resolves 200 without 401 loop or Tauri dependency", async () => {
    vi.stubEnv("VITE_XAVIER_API_TOKEN", "valid-test-token");
    useAuthStore.setState({ token: null });

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

  it("(e) falls back to empty string when neither store token nor env var is defined", () => {
    vi.stubEnv("VITE_XAVIER_API_TOKEN", "");
    useAuthStore.setState({ token: null });
    const { result } = renderHook(() => useApiToken());
    expect(result.current).toBe("");
    expect(getApiTokenSync()).toBe("");
  });
});
