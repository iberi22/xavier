import { describe, it, expect, beforeEach, vi } from "vitest";
import { useRagProfileStore, RAG_PROFILES } from "../src/lib/ragProfileStore.svelte";

describe("Visual RAG Onboarding Wizard Store", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("initializes with default profiles and storage quota", () => {
    const store = useRagProfileStore();

    expect(store.selectedProfiles).toEqual(["legal", "code"]);
    expect(store.storageGb).toBe(50);
    expect(store.zeroCloud).toBe(true);
    expect(store.isSaving).toBe(false);
    expect(store.saveStatus).toBeNull();
  });

  it("toggles profile selection correctly", () => {
    const store = useRagProfileStore();

    // Add video profile
    store.toggleProfile("video");
    expect(store.selectedProfiles).toContain("video");
    expect(store.selectedProfiles.length).toBe(3);

    // Remove legal profile
    store.toggleProfile("legal");
    expect(store.selectedProfiles).not.toContain("legal");
    expect(store.selectedProfiles.length).toBe(2);
  });

  it("calculates estimated RAM and node specs dynamically", () => {
    const store = useRagProfileStore();

    // Default 50 GB with legal (8 MB/GB) and code (12 MB/GB), base RAM 1024 MB
    // RAM = 1024 + 50 * (8 + 12) = 2024 MB
    expect(store.estimatedRamMb).toBe(2024);
    expect(store.formattedRamEstimate).toBe("2.0 GB");
    expect(store.recommendedNodeSpecs.minRamGb).toBe(2);

    // Increase storage quota to 200 GB
    store.setStorageGb(200);
    expect(store.storageGb).toBe(200);
    // RAM = 1024 + 200 * 20 = 5024 MB -> 4.9 GB
    expect(store.estimatedRamMb).toBe(5024);
    expect(store.formattedRamEstimate).toBe("4.9 GB");
  });

  it("clamps storage allocation within valid range (10 GB to 500 GB)", () => {
    const store = useRagProfileStore();

    store.setStorageGb(5);
    expect(store.storageGb).toBe(10);

    store.setStorageGb(1000);
    expect(store.storageGb).toBe(500);
  });

  it("handles successful API payload saving to /v1/maloca/config/profiles", async () => {
    const store = useRagProfileStore();

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ success: true, message: "OK", id: "node-123" }),
    });
    vi.stubGlobal("fetch", fetchMock);

    const result = await store.saveProfiles("http://localhost:8006");

    expect(result.success).toBe(true);
    expect(result.nodeConfigId).toBe("node-123");
    expect(store.saveStatus?.success).toBe(true);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, options] = fetchMock.mock.calls[0];
    expect(url).toBe("http://localhost:8006/v1/maloca/config/profiles");
    expect(options.method).toBe("POST");

    const body = JSON.parse(options.body as string);
    expect(body.selectedProfiles).toEqual(["legal", "code"]);
    expect(body.storageGb).toBe(50);
    expect(body.zeroCloud).toBe(true);
  });

  it("handles network error gracefully when saving configuration", async () => {
    const store = useRagProfileStore();

    const fetchMock = vi.fn().mockRejectedValue(new Error("Conexión rechazada"));
    vi.stubGlobal("fetch", fetchMock);

    const result = await store.saveProfiles();

    expect(result.success).toBe(false);
    expect(result.message).toContain("Conexión rechazada");
    expect(store.saveStatus?.success).toBe(false);
  });

  it("resets selection to default values", () => {
    const store = useRagProfileStore();

    store.toggleProfile("photos");
    store.setStorageGb(250);
    store.setZeroCloud(false);

    store.resetSelection();

    expect(store.selectedProfiles).toEqual(["legal", "code"]);
    expect(store.storageGb).toBe(50);
    expect(store.zeroCloud).toBe(true);
  });
});
