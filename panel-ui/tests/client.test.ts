import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  ApiClient,
  RateLimitError,
  getApiUrl,
  parseRateLimitRemaining,
  parseRetryAfter,
} from "../src/api/client";

describe("Rate limiting helpers", () => {
  it("parses Retry-After values correctly", () => {
    expect(parseRetryAfter(null)).toBeNull();
    expect(parseRetryAfter("")).toBeNull();
    expect(parseRetryAfter("30")).toBe(30);
    expect(parseRetryAfter("12.4")).toBe(13);

    const futureDate = new Date(Date.now() + 10000).toUTCString();
    const parsedDateDiff = parseRetryAfter(futureDate);
    expect(parsedDateDiff).toBeGreaterThanOrEqual(9);
    expect(parsedDateDiff).toBeLessThanOrEqual(11);

    const pastDate = new Date(Date.now() - 5000).toUTCString();
    expect(parseRetryAfter(pastDate)).toBe(1);
  });

  it("parses X-RateLimit-Remaining values correctly", () => {
    expect(parseRateLimitRemaining(null)).toBeNull();
    expect(parseRateLimitRemaining("")).toBeNull();
    expect(parseRateLimitRemaining("invalid")).toBeNull();
    expect(parseRateLimitRemaining("42")).toBe(42);
    expect(parseRateLimitRemaining("0")).toBe(0);
  });
});

describe("ApiClient unit tests", () => {
  const globalFetch = global.fetch;

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    global.fetch = globalFetch;
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("resolves getApiUrl correctly in web and Tauri environments", () => {
    expect(getApiUrl("/v1/test")).toBe("/v1/test");

    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(getApiUrl("/v1/test")).toBe("http://127.0.0.1:8006/v1/test");
  });

  it("throws an error when HTTP fetch response is not ok", async () => {
    global.fetch = vi.fn().mockResolvedValue({
      ok: false,
      text: vi.fn().mockResolvedValue("Unauthorized access"),
    });

    const client = new ApiClient("bad-token");
    await expect(client.getProvidersConfig()).rejects.toThrow("Unauthorized access");
  });

  it("fetches and updates providers config", async () => {
    const mockProviders = [{ provider: "openai", model: "gpt-4o" }];
    global.fetch = vi.fn()
      .mockResolvedValueOnce({
        ok: true,
        json: vi.fn().mockResolvedValue({ providers: mockProviders }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: vi.fn().mockResolvedValue({ status: "ok", message: "updated" }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: vi.fn().mockResolvedValue({ status: "ok", message: "tested" }),
      });

    const client = new ApiClient("test-token");
    const getRes = await client.getProvidersConfig();
    expect(getRes).toEqual({ providers: mockProviders });

    const updateRes = await client.updateProvidersConfig(mockProviders);
    expect(updateRes).toEqual({ status: "ok", message: "updated" });

    const testRes = await client.testProvider("openai");
    expect(testRes).toEqual({ status: "ok", message: "tested" });
  });

  it("fetches system scan and quota", async () => {
    const mockScan = { version: "1.0", os: "linux", arch: "x64", providers: [], workspace_id: "w1", memory_backend: "sqlite" };
    const mockQuota = [{ provider: "openai", used_hourly: 10, used_today: 100, used_weekly: 500, used_monthly: 2000, weekly_quota: 5000, cache_hits: 5, rate_limited_until: null, last_update: "2026-08-28" }];

    global.fetch = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue(mockScan) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue(mockQuota) });

    const client = new ApiClient("test-token");
    expect(await client.systemScan()).toEqual(mockScan);
    expect(await client.getProvidersQuota()).toEqual(mockQuota);
  });

  it("performs memory search and memory addition", async () => {
    const mockMemories = [{ id: "1", content: "test memory", kind: "note" }];
    global.fetch = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue(mockMemories) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue(mockMemories[0]) });

    const client = new ApiClient("test-token");
    const searchRes = await client.searchMemories("query", "note", 10);
    expect(searchRes).toEqual(mockMemories);
    expect(global.fetch).toHaveBeenCalledWith(
      expect.stringContaining("/api/memory/search?q=query&limit=10&kind=note"),
      expect.anything()
    );

    const addRes = await client.addMemory("new content");
    expect(addRes).toEqual(mockMemories[0]);
  });

  it("fetches agents, cloud node, updates cloud node, and data commons", async () => {
    global.fetch = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue([{ id: "a1", name: "Agent1" }]) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ url: "http://cloud", token: "ctok", instance_id: "inst1" }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "ok" }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ enabled: true, consent_given: true }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "ok" }) });

    const client = new ApiClient("test-token");

    const agents = await client.getAgents();
    expect(agents).toEqual([{ id: "a1", name: "Agent1" }]);

    const cloudNode = await client.getCloudNode();
    expect(cloudNode.data.url).toBe("http://cloud");

    const updateCloud = await client.updateCloudNode({ url: "u", token: "t", instance_id: "i" });
    expect(updateCloud).toEqual({ status: "ok" });

    const dataCommons = await client.getDataCommons();
    expect(dataCommons.data).toEqual({ enabled: true, consent_given: true });

    const optIn = await client.optInDataCommons({ enabled: true, consent_given: true });
    expect(optIn).toEqual({ status: "ok" });
  });

  it("handles cloud node null fallback values", async () => {
    global.fetch = vi.fn().mockResolvedValueOnce({
      ok: true,
      json: vi.fn().mockResolvedValue({ url: null, token: null, instance_id: null }),
    });

    const client = new ApiClient("test-token");
    const cloudNode = await client.getCloudNode();
    expect(cloudNode.data).toEqual({
      url: "",
      token: "",
      instance_id: "",
      sync_interval_ms: 300000,
      auto_heartbeat: true,
    });
  });

  it("handles mesh status, peer pairing, pairing code, ACL update, and peer removal", async () => {
    global.fetch = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "online", peers: [] }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "ok", node_id: "n1" }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ code: "123456" }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "ok" }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "ok" }) });

    const client = new ApiClient("test-token");

    expect(await client.getMeshStatus()).toEqual({ status: "online", peers: [] });
    expect(await client.pairPeer("123456")).toEqual({ status: "ok", node_id: "n1" });
    expect(await client.generatePairingCode("http://ep")).toEqual({ code: "123456" });
    expect(await client.updatePeerAcl("n1", "peer" as any, "user" as any)).toEqual({ status: "ok" });
    expect(await client.removePeer("n1")).toEqual({ status: "ok" });
  });

  it("handles secret leases and audit history", async () => {
    global.fetch = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue([]) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "ok" }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue([]) });

    const client = new ApiClient("test-token");

    expect(await client.getLeases()).toEqual([]);
    expect(await client.revokeLease("lease-tok")).toEqual({ status: "ok" });
    expect(await client.getLeaseHistory()).toEqual([]);
  });

  it("handles Ollama models and active configuration", async () => {
    global.fetch = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ models: ["llama3"] }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "success" }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ llm: "llama3" }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ ok: true }) });

    const client = new ApiClient("test-token");

    expect(await client.getOllamaModels()).toEqual({ models: ["llama3"] });
    expect(await client.pullOllamaModel("llama3")).toEqual({ status: "success" });
    expect(await client.getOllamaActive()).toEqual({ llm: "llama3" });
    expect(await client.setOllamaActive("llama3", "llm")).toEqual({ ok: true });
  });

  it("handles offline models and config management", async () => {
    const mockOfflineConfig = { local_model_dirs: ["/models"], auto_start_last_model: true };
    const mockOfflineModels = { models: [{ name: "m1", path: "/models/m1", size_bytes: 1000, quantization: "q4" }] };
    const mockOfflineStatus = { gpu_detected: true, gpu_vendor: "nvidia", vram_mb: 8192, engine_status: "running", active_model: "m1", port: 8080 };

    global.fetch = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue(mockOfflineConfig) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "ok", message: "updated" }) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue(mockOfflineModels) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue(mockOfflineStatus) })
      .mockResolvedValueOnce({ ok: true, json: vi.fn().mockResolvedValue({ status: "ok", message: "done", filename: "m1", path: "/p" }) });

    const client = new ApiClient("test-token");

    expect(await client.getOfflineConfig()).toEqual(mockOfflineConfig);
    expect(await client.updateOfflineConfig(mockOfflineConfig)).toEqual({ status: "ok", message: "updated" });
    expect(await client.getOfflineModels()).toEqual(mockOfflineModels);
    expect(await client.getOfflineStatus()).toEqual(mockOfflineStatus);
    expect(await client.downloadOfflineModel("http://url/m1")).toEqual({ status: "ok", message: "done", filename: "m1", path: "/p" });
  });

  it("handles 429 rate limit responses and emits custom event", async () => {
    const mockHeaders = new Headers({
      "Retry-After": "10",
      "X-RateLimit-Remaining": "0",
    });

    global.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 429,
      headers: mockHeaders,
      text: vi.fn().mockResolvedValue("Rate limit exceeded for tests"),
    });

    const eventListener = vi.fn();
    window.addEventListener("xavier-rate-limit", eventListener);

    const client = new ApiClient("test-token");

    let thrownErr: unknown;
    try {
      await client.getProvidersConfig();
    } catch (e) {
      thrownErr = e;
    }

    expect(thrownErr).toBeInstanceOf(RateLimitError);
    const rlErr = thrownErr as RateLimitError;
    expect(rlErr.status).toBe(429);
    expect(rlErr.retryAfterSeconds).toBe(10);
    expect(rlErr.remaining).toBe(0);
    expect(rlErr.message).toBe("Rate limit exceeded for tests");

    expect(eventListener).toHaveBeenCalledTimes(1);
    const eventDetail = (eventListener.mock.calls[0][0] as CustomEvent).detail;
    expect(eventDetail).toEqual({
      message: "Rate limit exceeded for tests",
      retryAfterSeconds: 10,
      remaining: 0,
      type: "rate-limit",
    });

    window.removeEventListener("xavier-rate-limit", eventListener);
  });

  it("handles background polling requests on 429 without throwing unhandled rejections", async () => {
    const mockHeaders = new Headers({
      "Retry-After": "1",
      "X-RateLimit-Remaining": "0",
    });

    global.fetch = vi.fn().mockResolvedValue({
      ok: false,
      status: 429,
      headers: mockHeaders,
      text: vi.fn().mockResolvedValue("Background rate limit"),
    });

    const client = new ApiClient("test-token");
    // Explicitly call with isBackground: true option via private fetch or custom background options
    const result = await (client as any).fetch("/v1/system/scan", {
      isBackground: true,
      maxRetries: 0,
    });

    expect(result).toBeNull();
  });
});
