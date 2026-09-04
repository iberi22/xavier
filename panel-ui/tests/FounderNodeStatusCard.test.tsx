import { render, screen, fireEvent, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import FounderNodeStatusCard from "../src/components/FounderNodeStatusCard";

describe("FounderNodeStatusCard Component", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal(
      "fetch",
      vi.fn().mockImplementation((url: string) => {
        if (url.includes("/health")) {
          return Promise.resolve({
            ok: true,
            json: () => Promise.resolve({ status: "healthy", version: "1.0.0" }),
          });
        }
        if (url.includes("/xavier/sync/check")) {
          return Promise.resolve({
            ok: true,
            json: () =>
              Promise.resolve({
                status: "ok",
                lag_ms: 12,
                save_ok_rate: 0.99,
                active_agents: 5,
              }),
          });
        }
        if (url.includes("/v1/mesh/public/nodes")) {
          return Promise.resolve({
            ok: true,
            json: () =>
              Promise.resolve([
                { node_id: "node_alpha", provider: "vps", status: "active" },
                { node_id: "node_beta", provider: "supabase", status: "active" },
              ]),
          });
        }
        return Promise.resolve({ ok: false });
      }),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("renders correctly with Genesis Founder role and cryptographic identity", async () => {
    await act(async () => {
      render(<FounderNodeStatusCard />);
    });

    expect(screen.getByText("SWAL Founder Node")).toBeInTheDocument();
    expect(screen.getByText("GENESIS")).toBeInTheDocument();
    expect(screen.getByText(/Role: Genesis Founder/i)).toBeInTheDocument();
    expect(screen.getByText("Cryptographic Identity")).toBeInTheDocument();
    expect(screen.getByText(/ed25519:xavier_founder_01/i)).toBeInTheDocument();
  });

  it("displays sync state and connected mesh peers metrics", async () => {
    await act(async () => {
      render(<FounderNodeStatusCard />);
    });

    expect(screen.getByText("Mesh Synchronization State")).toBeInTheDocument();
    expect(screen.getByText(/Connected Mesh Peers/i)).toBeInTheDocument();
    expect(screen.getByText("node_alpha")).toBeInTheDocument();
    expect(screen.getByText("node_beta")).toBeInTheDocument();
  });

  it("polls telemetry every 15 seconds", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");

    await act(async () => {
      render(<FounderNodeStatusCard />);
    });

    const initialFetchCount = fetchSpy.mock.calls.length;

    await act(async () => {
      vi.advanceTimersByTime(15000);
    });

    expect(fetchSpy.mock.calls.length).toBeGreaterThan(initialFetchCount);
  });

  it("handles dark and light mode styling classes correctly", async () => {
    let container: HTMLElement | null = null;
    await act(async () => {
      const res = render(<FounderNodeStatusCard />);
      container = res.container;
    });

    const cardElement = container?.querySelector('[data-testid="founder-node-status-card"]');
    expect(cardElement).not.toBeNull();
    expect(cardElement?.className).toContain("bg-white");
    expect(cardElement?.className).toContain("dark:bg-[#0a0a0a]");
  });

  it("calls onClose when close button is clicked", async () => {
    const handleClose = vi.fn();
    await act(async () => {
      render(<FounderNodeStatusCard onClose={handleClose} />);
    });

    const closeButton = screen.getByLabelText("Close");
    fireEvent.click(closeButton);

    expect(handleClose).toHaveBeenCalledTimes(1);
  });
});
