import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { malocaApi, probeMarketplaceAvailable } from "./api";
import { AUTH_TOKEN_KEY } from "./authPrompt";

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

  it("interacts with /v2/marketplace listings, buy, receipts and consent endpoints", async () => {
    const fn = mockFetch([
      {
        id: "list_1",
        seller_id: "seller_node",
        title: "Test Biology",
        description: "Test bio data",
        price_credits: 10,
        license: "cc-by-4.0",
        terms_version: "v1.0",
        created_at_ms: 1000,
        revoked_at_ms: null,
      },
    ]);

    const listings = await malocaApi.getMarketplaceListings();
    expect(fn.mock.calls[0][0]).toBe("/v2/marketplace/listings");
    expect(listings.length).toBe(1);
    expect(listings[0].title).toBe("Test Biology");

    // Create listing with signed headers
    const fnCreate = mockFetch({
      id: "list_2",
      seller_id: "seller_node",
      title: "New Dataset",
      description: "Description",
      price_credits: 15,
      license: "mit",
      terms_version: "v1.0",
      created_at_ms: 1500,
    }, 201);
    const createdListing = await malocaApi.createMarketplaceListing(
      {
        seller_id: "seller_node",
        title: "New Dataset",
        description: "Description",
        price_credits: 15,
        license: "mit",
        terms_version: "v1.0",
      },
      {
        "X-Xavier-Actor-Key": "actor_key_hex",
        "X-Xavier-Actor-Timestamp": "1500",
        "X-Xavier-Actor-Signature": "sig_hex",
      },
    );
    expect(fnCreate.mock.calls[0][0]).toBe("/v2/marketplace/listings");
    expect(fnCreate.mock.calls[0][1].method).toBe("POST");
    expect(fnCreate.mock.calls[0][1].headers["X-Xavier-Actor-Key"]).toBe("actor_key_hex");
    expect(createdListing.id).toBe("list_2");

    // Revoke listing with signed headers
    const fnRevoke = mockFetch({
      id: "list_2",
      seller_id: "seller_node",
      title: "New Dataset",
      revoked_at_ms: 2000,
    });
    await malocaApi.revokeMarketplaceListing("list_2", {
      "X-Xavier-Actor-Key": "actor_key_hex",
      "X-Xavier-Actor-Timestamp": "2000",
      "X-Xavier-Actor-Signature": "sig_hex_revoke",
    });
    expect(fnRevoke.mock.calls[0][0]).toBe("/v2/marketplace/listings/list_2/revoke");
    expect(fnRevoke.mock.calls[0][1].method).toBe("POST");
    expect(fnRevoke.mock.calls[0][1].headers["X-Xavier-Actor-Key"]).toBe("actor_key_hex");

    // Buy listing
    mockFetch({
      id: "rcpt_1",
      listing_id: "list_1",
      buyer_id: "buyer_node",
      amount_credits: 10,
      currency: "internal_credits",
      created_at_ms: 2000,
    }, 201);
    const receipt = await malocaApi.buyMarketplaceListing({
      listing_id: "list_1",
      buyer_id: "buyer_node",
    });
    expect(receipt.id).toBe("rcpt_1");
    expect(receipt.amount_credits).toBe(10);

    // Get receipts
    mockFetch([{ id: "rcpt_1", listing_id: "list_1", buyer_id: "buyer_node", amount_credits: 10, currency: "internal_credits", created_at_ms: 2000 }]);
    const myReceipts = await malocaApi.getMarketplaceReceipts("buyer_node");
    expect(myReceipts.length).toBe(1);

    // Set and get consent
    const fnConsent = mockFetch({
      node_id: "node_1",
      state: "shared",
      terms_version: "v1.0",
      consent_version: "v1.0",
      updated_at_ms: 3000,
    });
    const consent = await malocaApi.setMarketplaceConsent(
      {
        node_id: "node_1",
        state: "shared",
        terms_version: "v1.0",
        consent_version: "v1.0",
      },
      {
        "X-Xavier-Actor-Key": "actor_key_hex",
        "X-Xavier-Actor-Timestamp": "3000",
        "X-Xavier-Actor-Signature": "sig_consent_hex",
      },
    );
    expect(fnConsent.mock.calls[0][0]).toBe("/v2/marketplace/consent");
    expect(fnConsent.mock.calls[0][1].method).toBe("POST");
    expect(JSON.parse(fnConsent.mock.calls[0][1].body)).toEqual({
      node_id: "node_1",
      state: "shared",
      terms_version: "v1.0",
      consent_version: "v1.0",
    });
    expect(fnConsent.mock.calls[0][1].headers["X-Xavier-Actor-Key"]).toBe("actor_key_hex");
    expect(consent.state).toBe("shared");

    mockFetch({
      node_id: "node_1",
      state: "shared",
      terms_version: "v1.0",
      consent_version: "v1.0",
      updated_at_ms: 3000,
    });
    const retrievedConsent = await malocaApi.getMarketplaceConsent("node_1");
    expect((retrievedConsent as any).state).toBe("shared");
  });
});

describe("marketplace availability probe and consent scope", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.unstubAllGlobals());

  it("sends the stored X-Xavier-Token when checking the marketplace routes", async () => {
    localStorage.setItem(AUTH_TOKEN_KEY, "tok_probe");
    const fn = mockFetch([]);
    expect(await probeMarketplaceAvailable()).toBe(true);
    expect(fn.mock.calls[0][0]).toBe("/v2/marketplace/listings");
    expect(fn.mock.calls[0][1].headers["X-Xavier-Token"]).toBe("tok_probe");
  });

  it("omits the token header when no token is stored", async () => {
    const fn = mockFetch([]);
    expect(await probeMarketplaceAvailable()).toBe(true);
    expect(fn.mock.calls[0][1].headers["X-Xavier-Token"]).toBeUndefined();
  });

  it("reports the marketplace unavailable on 404 or 401", async () => {
    mockFetch({ error: "Not Found" }, 404);
    expect(await probeMarketplaceAvailable()).toBe(false);

    mockFetch({ error: "Unauthorized" }, 401);
    expect(await probeMarketplaceAvailable()).toBe(false);
  });

  it("reports the marketplace unavailable when the request fails", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
    expect(await probeMarketplaceAvailable()).toBe(false);
  });

  it("reads consent for one node only", async () => {
    const fn = mockFetch({
      node_id: "node_1",
      state: "off",
      terms_version: "",
      consent_version: "",
      updated_at_ms: 0,
    });
    await malocaApi.getMarketplaceConsent("node_1");
    expect(fn.mock.calls[0][0]).toBe("/v2/marketplace/consent?node_id=node_1");
  });
});
