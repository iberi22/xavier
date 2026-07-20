import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import React from "react";
import OperationModeBadge from "../src/components/OperationModeBadge";

// Mock useAuthStore
vi.mock("../src/auth/AuthProvider", () => ({
  useAuthStore: vi.fn((selector) =>
    selector({
      token: "mock-token",
    }),
  ),
}));

describe("OperationModeBadge", () => {
  const originalFetch = global.fetch;

  beforeEach(() => {
    // No fake timers to avoid waitFor/promise issues
  });

  afterEach(() => {
    global.fetch = originalFetch;
    vi.restoreAllMocks();
  });

  it("handles offline state when fetches fail", async () => {
    global.fetch = vi.fn().mockRejectedValue(new Error("Network Error"));

    render(<OperationModeBadge />);

    await waitFor(() => {
      expect(screen.getAllByText(/Offline/i).length).toBeGreaterThan(0);
    });
  });

  it("handles local mode successfully", async () => {
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (url.endsWith("/health")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              status: "healthy",
              mode: "local-healthy",
              llm: {
                provider: "ollama",
                model: "qwen3-coder",
                reachable: true,
              },
            }),
        } as Response);
      }
      return Promise.resolve({ ok: false } as Response);
    });

    render(<OperationModeBadge />);

    await waitFor(() => {
      expect(screen.getAllByText(/Local/i).length).toBeGreaterThan(0);
      expect(screen.getAllByText(/Ollama/i).length).toBeGreaterThan(0);
      expect(screen.getAllByText(/\(qwen3-coder\)/i).length).toBeGreaterThan(0);
    });
  });

  it("handles cloud mode successfully", async () => {
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (url.endsWith("/health")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              status: "healthy",
              mode: "cloud-fallback",
              llm: {
                provider: "openai",
                model: "gpt-4o",
                reachable: true,
              },
            }),
        } as Response);
      }
      return Promise.resolve({ ok: false } as Response);
    });

    render(<OperationModeBadge />);

    await waitFor(() => {
      expect(screen.getAllByText(/Cloud/i).length).toBeGreaterThan(0);
      expect(screen.getAllByText(/Openai/i).length).toBeGreaterThan(0);
    });
  });

  it("handles degraded mode successfully", async () => {
    global.fetch = vi.fn().mockImplementation((url: string) => {
      if (url.endsWith("/health")) {
        return Promise.resolve({
          ok: true,
          json: () =>
            Promise.resolve({
              status: "degraded",
              mode: "local-degraded",
              llm: {
                provider: "ollama",
                model: "qwen3-coder",
                reachable: false,
              },
            }),
        } as Response);
      }
      return Promise.resolve({ ok: false } as Response);
    });

    render(<OperationModeBadge />);

    await waitFor(() => {
      expect(screen.getAllByText(/Degradado/i).length).toBeGreaterThan(0);
      expect(screen.getAllByText(/Ollama/i).length).toBeGreaterThan(0);
    });
  });
});
