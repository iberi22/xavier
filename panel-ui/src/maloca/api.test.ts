import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { malocaApi } from "./api";

const session = {
  id: "is_1",
  challenge_id: "hc_1",
  technique: "pre_mortem",
  turns: [{ role: "llm_guide", content: "Why?", timestamp: "2026-01-01T00:00:00Z" }],
  depth_score: 0,
  insights: [],
  status: "active",
  started_at: "2026-01-01T00:00:00Z",
  completed_at: null,
};

function mockFetch(body: unknown, status = 200) {
  const fn = vi.fn().mockImplementation(async () =>
    new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } }),
  );
  vi.stubGlobal("fetch", fn);
  return fn;
}

describe("malocaApi prefixes and introspection contract", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.unstubAllGlobals());

  it("serves legacy ops endpoints under /maloca (not /api/v1/maloca)", async () => {
    const fn = mockFetch({});
    await malocaApi.getPack();
    expect(fn.mock.calls[0][0]).toBe("/maloca/pack");
  });

  it("start posts the backend StartIntrospectionRequest to /v1/maloca", async () => {
    const fn = mockFetch({ session }, 201);
    const res = await malocaApi.startIntrospection({
      challenge_id: "hc_1",
      challenge_type: "decision",
      description: "d",
      technique: "pre_mortem",
    });
    expect(fn.mock.calls[0][0]).toBe("/v1/maloca/introspection/start");
    expect(fn.mock.calls[0][1].method).toBe("POST");
    expect(JSON.parse(fn.mock.calls[0][1].body)).toEqual({
      challenge_id: "hc_1",
      challenge_type: "decision",
      description: "d",
      technique: "pre_mortem",
    });
    expect(res.session.id).toBe("is_1");
  });

  it("turn, complete and get hit the session routes", async () => {
    const fn = mockFetch({ session });
    await malocaApi.introspectionTurn("is_1", { human_input: "x", challenge_description: "d" });
    await malocaApi.completeIntrospection("is_1");
    await malocaApi.getIntrospection("is_1");
    expect(fn.mock.calls.map((c) => c[0])).toEqual([
      "/v1/maloca/introspection/is_1/turn",
      "/v1/maloca/introspection/is_1/complete",
      "/v1/maloca/introspection/is_1",
    ]);
    expect(JSON.parse(fn.mock.calls[0][1].body)).toEqual({
      human_input: "x",
      challenge_description: "d",
    });
  });

  it("surfaces the backend { error } message", async () => {
    mockFetch({ error: "Introspection session not found" }, 404);
    await expect(malocaApi.getIntrospection("nope")).rejects.toThrow("Introspection session not found");
  });
});
