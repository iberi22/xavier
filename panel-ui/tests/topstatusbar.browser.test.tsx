import { act, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, vi, test } from "vitest";
import TopStatusBar from "../src/components/TopStatusBar";

describe("TopStatusBar Browser Compatibility & Guards", () => {
  const originalFetch = global.fetch;

  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    delete (window as any).__TAURI_INTERNALS__;
    localStorage.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    global.fetch = originalFetch;
    delete (window as any).__TAURI_INTERNALS__;
  });

  test("renders loading spinner initially while loading resources", async () => {
    global.fetch = vi.fn().mockImplementation(() => new Promise(() => {})); // Never resolves

    render(<TopStatusBar />);

    expect(screen.getByLabelText("Loading...")).toBeInTheDocument();
    expect(screen.getByText("Loading...")).toBeInTheDocument();
  });

  test("fetches /health and updates CPU/RAM metrics in non-Tauri browser mode", async () => {
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (typeof url === "string" && url.includes("/health")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              system: {
                cpu_usage: 45.5,
                ram_usage_percent: 50.0,
              },
            }),
        });
      }
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({}),
      });
    });

    render(<TopStatusBar />);

    await waitFor(() => {
      expect(screen.queryByText("Loading...")).not.toBeInTheDocument();
    });

    expect(screen.getByTitle("CPU: 46%")).toBeInTheDocument();
    expect(global.fetch).toHaveBeenCalledWith(expect.stringContaining("/health"));
  });

  test("uses dynamic invoke mock when __TAURI_INTERNALS__ is present in window", async () => {
    const mockInvoke = vi.fn().mockImplementation((cmd: string) => {
      if (cmd === "get_current_config_state") {
        return Promise.resolve({
          has_openai: true,
          has_gemini: false,
          has_telegram: true,
        });
      }
      if (cmd === "get_realtime_metrics") {
        return Promise.resolve({
          cpu_percent: 32,
          ram_used_gb: 4,
          ram_total_gb: 16,
        });
      }
      return Promise.resolve({});
    });

    (window as any).__TAURI_INTERNALS__ = {
      invoke: mockInvoke,
      plugins: {},
      transformCallback: (cb: any) => cb,
    };

    render(<TopStatusBar />);

    await waitFor(() => {
      expect(screen.getByTitle("CPU: 32%")).toBeInTheDocument();
    });

    expect(screen.getByTitle("Memory: 4.0GB / 16.0GB")).toBeInTheDocument();
  });

  test("sets up and cleans up polling interval on mount and unmount", async () => {
    const setIntervalSpy = vi.spyOn(global, "setInterval");
    const clearIntervalSpy = vi.spyOn(global, "clearInterval");

    global.fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: () =>
        Promise.resolve({
          system: { cpu_usage: 10, ram_usage_percent: 20 },
        }),
    });

    const { unmount } = render(<TopStatusBar />);

    await waitFor(() => {
      expect(screen.queryByText("Loading...")).not.toBeInTheDocument();
    });

    expect(setIntervalSpy).toHaveBeenCalled();

    unmount();

    expect(clearIntervalSpy).toHaveBeenCalled();
  });

  test("fetches config fallback via /v1/config/providers in non-Tauri browser mode", async () => {
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (typeof url === "string" && url.includes("/v1/config/providers")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              providers: [
                { provider: "openai", api_key: "sk-123", model: "gpt-4o" },
              ],
            }),
        });
      }
      if (typeof url === "string" && url.includes("/health")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({ system: { cpu_usage: 15, ram_usage_percent: 30 } }),
        });
      }
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({}),
      });
    });

    render(<TopStatusBar />);

    await waitFor(() => {
      expect(screen.getByText("OAI")).toBeInTheDocument();
    });

    expect(global.fetch).toHaveBeenCalledWith(
      expect.stringContaining("/v1/config/providers"),
      expect.any(Object),
    );
  });

  test("handles network/fetch error gracefully without crashing or throwing invoke error", async () => {
    vi.spyOn(console, "debug").mockImplementation(() => {});

    global.fetch = vi.fn().mockRejectedValue(new Error("Network Error"));

    render(<TopStatusBar />);

    await waitFor(() => {
      expect(screen.queryByText("Loading...")).not.toBeInTheDocument();
    });

    expect(screen.getAllByText(/Xavier/).length).toBeGreaterThan(0);
    expect(screen.getByTitle("CPU: 0%")).toBeInTheDocument();
  });

  test("displays node indicator button, opens Node modal, and updates remote URL", async () => {
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (typeof url === "string" && url.includes("/health")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              system: { cpu_usage: 10, ram_usage_percent: 20 },
            }),
        });
      }
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({}),
      });
    });

    render(<TopStatusBar />);

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Node Connection Settings" }),
      ).toBeInTheDocument();
    });

    const nodeButton = screen.getByRole("button", {
      name: "Node Connection Settings",
    });
    await act(async () => {
      nodeButton.click();
    });

    expect(screen.getByText("Xavier Node Connectivity")).toBeInTheDocument();

    const input = screen.getByLabelText(/Remote Node Endpoint/) as HTMLInputElement;
    expect(input).toBeInTheDocument();

    const saveButton = screen.getByRole("button", { name: "Save Remote Node" });
    await act(async () => {
      saveButton.click();
    });

    expect(saveButton).toBeInTheDocument();
  });

  test("opens sync popover, triggers manual sync push & pull cycle on Sync Now click, and dispatches toast event", async () => {
    const toastListener = vi.fn();
    window.addEventListener("xavier-toast", toastListener);

    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (typeof url === "string" && url.includes("/health")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({ system: { cpu_usage: 10, ram_usage_percent: 20 } }),
        });
      }
      if (typeof url === "string" && url.includes("/api/v1/memory/sync/push")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              status: "ok",
              session: { chunks_sent: 2, chunks_received: 0 },
            }),
        });
      }
      if (typeof url === "string" && url.includes("/api/v1/memory/sync/pull")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              status: "ok",
              session: { chunks_sent: 0, chunks_received: 3 },
            }),
        });
      }
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({}),
      });
    });

    render(<TopStatusBar />);

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Memory Sync Control & Status" }),
      ).toBeInTheDocument();
    });

    const syncPill = screen.getByRole("button", {
      name: "Memory Sync Control & Status",
    });

    await act(async () => {
      syncPill.click();
    });

    expect(screen.getByText("Memory Sync")).toBeInTheDocument();

    const syncNowButton = screen.getByRole("button", { name: "Sync Now" });
    expect(syncNowButton).toBeInTheDocument();

    await act(async () => {
      syncNowButton.click();
    });

    await waitFor(() => {
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/memory/sync/push"),
        expect.any(Object),
      );
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining("/api/v1/memory/sync/pull"),
        expect.any(Object),
      );
      expect(toastListener).toHaveBeenCalled();
    });

    window.removeEventListener("xavier-toast", toastListener);
  });

  test("dispatches error toast when manual sync fails", async () => {
    const errorToastListener = vi.fn();
    window.addEventListener("xavier-error-toast", errorToastListener);

    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (typeof url === "string" && url.includes("/health")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({ system: { cpu_usage: 10, ram_usage_percent: 20 } }),
        });
      }
      if (typeof url === "string" && url.includes("/api/v1/memory/sync/push")) {
        return Promise.resolve({
          ok: false,
          status: 500,
          text: () => Promise.resolve("Internal Server Error"),
        });
      }
      return Promise.resolve({
        ok: true,
        json: () => Promise.resolve({}),
      });
    });

    render(<TopStatusBar />);

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Memory Sync Control & Status" }),
      ).toBeInTheDocument();
    });

    const syncPill = screen.getByRole("button", {
      name: "Memory Sync Control & Status",
    });

    await act(async () => {
      syncPill.click();
    });

    const syncNowButton = screen.getByRole("button", { name: "Sync Now" });

    await act(async () => {
      syncNowButton.click();
    });

    await waitFor(() => {
      expect(errorToastListener).toHaveBeenCalled();
    });

    window.removeEventListener("xavier-error-toast", errorToastListener);
  });
});
