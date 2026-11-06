import { describe, expect, it } from "vitest";
import { getLatencyColor } from "../src/components/Telecom/PeerDiscoveryCard";
import {
  isPeerRecentlySeen,
  PEER_ONLINE_WINDOW_S,
} from "../src/components/Telecom/TelecomHubView";

describe("peer presence from last_seen_at (unix seconds)", () => {
  const nowMs = Date.UTC(2026, 10, 6, 18, 0, 0);
  const nowS = Math.floor(nowMs / 1000);

  it("treats a peer seen within the window as online", () => {
    expect(isPeerRecentlySeen(nowS, nowMs)).toBe(true);
    expect(isPeerRecentlySeen(nowS - PEER_ONLINE_WINDOW_S, nowMs)).toBe(true);
  });

  it("treats an older or missing last_seen_at as offline", () => {
    expect(isPeerRecentlySeen(nowS - PEER_ONLINE_WINDOW_S - 1, nowMs)).toBe(false);
    expect(isPeerRecentlySeen(null, nowMs)).toBe(false);
    expect(isPeerRecentlySeen(undefined, nowMs)).toBe(false);
  });

  it("does not mistake a seconds timestamp for milliseconds", () => {
    // A millisecond value would be ~1000x larger; seconds must still be read as recent.
    expect(isPeerRecentlySeen(nowS - 10, nowMs)).toBe(true);
  });

  it("labels an online peer without measured latency as Online", () => {
    expect(getLatencyColor(null, "online").label).toBe("Online");
    expect(getLatencyColor(null, "offline").label).toBe("Offline");
    expect(getLatencyColor(null).label).toBe("Offline");
  });
});
