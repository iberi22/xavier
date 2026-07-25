import { afterEach, describe, expect, it, vi } from "vitest";
import { malocaApi } from "../src/maloca/api";
import defaultUi from "../src/maloca/maloca.ui.json";
import type { VoteChoice } from "../src/maloca/types";

describe("maloca.ui.json", () => {
  it("includes decisions section and locked economic keys", () => {
    const ids = defaultUi.layout.sections.map((s) => s.id);
    expect(ids).toContain("council");
    expect(ids).toContain("decisions");
    expect(ids).toContain("nodes");
    expect(defaultUi.editMode.lockedKeys).toContain("manager_adds_vote_weight");
    expect(defaultUi.editMode.lockedKeys).toContain("synapse_unfrozen");
  });
});

describe("malocaApi vote/decisions", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("casts vote with node_id and choice", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        id: "v-1",
        proposal_id: "p-1",
        node_id: "lab_genesis",
        choice: "yes" satisfies VoteChoice,
        weight: 1,
        created_at: "2026-07-25T00:00:00Z",
      }),
    });
    vi.stubGlobal("fetch", fetchMock);

    const vote = await malocaApi.castVote("p-1", {
      node_id: "lab_genesis",
      choice: "yes",
    });
    expect(vote.node_id).toBe("lab_genesis");
    expect(fetchMock).toHaveBeenCalledWith(
      "/maloca/proposals/p-1/vote",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({ node_id: "lab_genesis", choice: "yes" }),
      }),
    );
  });

  it("surfaces karma rejection body", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        status: 403,
        text: async () => "karma 50 below vote_karma_min 500 (node local)",
      }),
    );

    await expect(
      malocaApi.castVote("p-1", { node_id: "local", choice: "yes" }),
    ).rejects.toThrow(/vote_karma_min/);
  });

  it("lists decisions and filtered votes", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({
        ok: true,
        json: async () => [
          {
            id: "d-1",
            kind: "voted",
            actor_node_id: "lab_genesis",
            genesis_node_id: "lab_genesis",
            payload: {},
            created_at: "t",
          },
        ],
      })
      .mockResolvedValueOnce({
        ok: true,
        json: async () => [],
      });
    vi.stubGlobal("fetch", fetchMock);

    const decisions = await malocaApi.listDecisions();
    expect(decisions[0].genesis_node_id).toBe("lab_genesis");

    await malocaApi.listVotes("p-genesis-params");
    expect(fetchMock.mock.calls[1][0]).toBe(
      "/maloca/votes?proposal_id=p-genesis-params",
    );
  });
});
