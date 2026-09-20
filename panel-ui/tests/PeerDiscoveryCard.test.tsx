import { act, fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PeerDiscoveryCard, {
  getLatencyColor,
  truncatePublicKey,
} from "../src/components/Telecom/PeerDiscoveryCard";

describe("PeerDiscoveryCard Component", () => {
  const samplePublicKey = "0x7a8f3b2c9d1e4f5a6b7c8d9e0f1a2b3c4d5e6f7a";
  const sampleAlias = "Primary Gateway Alpha";
  const sampleNodeId = "node_alpha_8f";

  beforeEach(() => {
    vi.useFakeTimers();
    // Mock navigator.clipboard
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn().mockImplementation(() => Promise.resolve()),
      },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("truncates long public keys correctly", () => {
    expect(truncatePublicKey(samplePublicKey)).toBe("0x7a8f3b...5e6f7a");
    expect(truncatePublicKey("short_key")).toBe("short_key");
    expect(truncatePublicKey("")).toBe("");
  });

  it("computes latency color badges according to round-trip thresholds", () => {
    // Optimal (<50ms)
    const green = getLatencyColor(24, "online");
    expect(green.label).toContain("24ms (Optimal)");
    expect(green.bg).toContain("emerald");

    // Moderate (50ms-150ms)
    const yellow = getLatencyColor(110, "online");
    expect(yellow.label).toContain("110ms (Moderate)");
    expect(yellow.bg).toContain("amber");

    // High (>150ms)
    const red = getLatencyColor(220, "online");
    expect(red.label).toContain("220ms (High Latency)");
    expect(red.bg).toContain("rose");

    // Offline / null
    const offline = getLatencyColor(null, "offline");
    expect(offline.label).toBe("Offline");
    expect(offline.bg).toContain("rose");
  });

  it("renders peer info, alias, node ID, and public key snippet", () => {
    render(
      <PeerDiscoveryCard
        publicKey={samplePublicKey}
        alias={sampleAlias}
        nodeId={sampleNodeId}
        latencyMs={18}
        lastSeen="10s ago"
      />,
    );

    expect(screen.getByText(sampleAlias)).toBeInTheDocument();
    expect(screen.getByText(`Node ID: ${sampleNodeId}`)).toBeInTheDocument();
    expect(screen.getByText("10s ago", { exact: false })).toBeInTheDocument();
    expect(screen.getByTestId("public-key-snippet")).toHaveTextContent(
      "0x7a8f3b...5e6f7a",
    );
    expect(screen.getByText("18ms (Optimal)")).toBeInTheDocument();
  });

  it("copies public key to clipboard when Copy button is clicked", async () => {
    render(
      <PeerDiscoveryCard
        publicKey={samplePublicKey}
        alias={sampleAlias}
        latencyMs={42}
      />,
    );

    const copyBtn = screen.getByLabelText("Copy public key to clipboard");
    await act(async () => {
      fireEvent.click(copyBtn);
    });

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(samplePublicKey);
    expect(screen.getByText("Copied")).toBeInTheDocument();

    // Fast-forward 2s to test reset state
    await act(async () => {
      vi.advanceTimersByTime(2000);
    });
    expect(screen.queryByText("Copied")).not.toBeInTheDocument();
  });

  it("invokes onPing callback when Ping button is pressed", async () => {
    const handlePing = vi.fn();
    render(
      <PeerDiscoveryCard
        publicKey={samplePublicKey}
        alias={sampleAlias}
        latencyMs={35}
        onPing={handlePing}
      />,
    );

    const pingBtn = screen.getByLabelText(`Ping node ${sampleAlias}`);
    await act(async () => {
      fireEvent.click(pingBtn);
    });

    expect(handlePing).toHaveBeenCalledWith(samplePublicKey);
  });

  it("invokes onInitiateDirectChat callback when Direct Chat button is clicked", () => {
    const handleStartChat = vi.fn();
    render(
      <PeerDiscoveryCard
        publicKey={samplePublicKey}
        alias={sampleAlias}
        nodeId={sampleNodeId}
        latencyMs={30}
        onInitiateDirectChat={handleStartChat}
      />,
    );

    const chatBtn = screen.getByLabelText(`Start Direct Chat with ${sampleAlias}`);
    fireEvent.click(chatBtn);

    expect(handleStartChat).toHaveBeenCalledWith({
      publicKey: samplePublicKey,
      alias: sampleAlias,
      nodeId: sampleNodeId,
    });
  });
});
