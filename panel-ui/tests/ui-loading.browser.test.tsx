import { act, render, screen } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

import ChatHistory from "../src/components/ChatHistory";
import NotificationsDropdown from "../src/components/NotificationsDropdown";
import TopStatusBar from "../src/components/TopStatusBar";
import { ErrorToast } from "../src/components/ui/ErrorToast";
import { LoadingSpinner } from "../src/components/ui/LoadingSpinner";
import type { PanelMessage } from "../src/types";

// Mock Tauri invoke and listen
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd) => {
    if (cmd === "get_realtime_metrics") {
      return Promise.resolve({ cpu_percent: 25, ram_used_gb: 4, ram_total_gb: 16 });
    }
    if (cmd === "get_current_config_state") {
      return Promise.resolve({ has_openai: true, has_gemini: false, has_telegram: false });
    }
    if (cmd === "get_xavier_token") {
      return Promise.resolve("mock-token");
    }
    return Promise.resolve({});
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

// Mock fetch
global.fetch = vi.fn().mockImplementation((url: string) => {
  if (url.includes("/notifications")) {
    return Promise.resolve({
      ok: true,
      json: () => Promise.resolve([]),
    });
  }
  if (url.includes("/v1/memories")) {
    return Promise.resolve({
      ok: true,
      json: () => Promise.resolve({ pagination: { total: 5 } }),
    });
  }
  return Promise.resolve({
    ok: true,
    json: () => Promise.resolve({}),
  });
}) as any;

describe("UI Loading & Error States", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  test("(a) LoadingSpinner renderiza svg con size prop", () => {
    const { container } = render(<LoadingSpinner size={32} className="custom-spin" />);
    const svg = container.querySelector("svg");
    expect(svg).toBeInTheDocument();
    expect(svg).toHaveAttribute("width", "32");
    expect(svg).toHaveAttribute("height", "32");
    expect(svg).toHaveClass("animate-spin");
    expect(svg).toHaveClass("custom-spin");
  });

  test("(b) ErrorToast aparece con message y desaparece tras 4s", () => {
    vi.useFakeTimers();
    const onClose = vi.fn();

    render(<ErrorToast message="Connection error" onClose={onClose} autoDismissMs={4000} />);

    expect(screen.getByText("Connection error")).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(4000);
    });

    expect(screen.queryByText("Connection error")).not.toBeInTheDocument();
    expect(onClose).toHaveBeenCalled();
  });

  test("(c) NotificationsDropdown muestra 3 skeletons cuando isLoading", async () => {
    await act(async () => {
      render(<NotificationsDropdown onClose={vi.fn()} isLoading={true} />);
    });

    const skeletonContainer = screen.getByTestId("notification-skeletons");
    expect(skeletonContainer).toBeInTheDocument();

    const pulseElements = skeletonContainer.querySelectorAll(".animate-pulse");
    expect(pulseElements.length).toBe(3);
  });

  test("(d) TopStatusBar muestra spinner cuando fetching metrics", async () => {
    await act(async () => {
      render(<TopStatusBar isLoading={true} />);
    });

    const spinner = screen.getByTestId("loading-spinner");
    expect(spinner).toBeInTheDocument();
  });

  test("(e) ChatHistory spinner durante streaming", () => {
    const messages: PanelMessage[] = [
      {
        id: "msg_1",
        role: "user",
        plain_text: "Hello",
        created_at: new Date().toISOString(),
      },
      {
        id: "msg_2",
        role: "assistant",
        plain_text: "Responding to prompt...",
        created_at: new Date().toISOString(),
      },
    ];

    render(<ChatHistory messages={messages} streamingMessageId="msg_2" />);

    const spinners = screen.getAllByTestId("loading-spinner");
    expect(spinners.length).toBeGreaterThanOrEqual(1);
  });

  test("(f) ErrorToast con 2 mensajes en queue", () => {
    render(<ErrorToast queue={["First error", "Second error"]} />);

    expect(screen.getByText("First error")).toBeInTheDocument();
    expect(screen.getByText("Second error")).toBeInTheDocument();

    const toastItems = screen.getAllByTestId("error-toast-item");
    expect(toastItems.length).toBe(2);
  });

  test("(g) ErrorToast renders structured rate-limit toast with cooldown badge", () => {
    render(
      <ErrorToast
        structuredToasts={[
          {
            id: "rl-1",
            message: "Rate limit exceeded. Slow down.",
            type: "rate-limit",
            cooldownSeconds: 15,
            remaining: 0,
          },
        ]}
      />
    );

    expect(screen.getByText("Rate limit exceeded. Slow down.")).toBeInTheDocument();
    expect(screen.getByTestId("rate-limit-cooldown-badge")).toHaveTextContent("Cooldown: 15s");
    expect(screen.getByTestId("rate-limit-remaining-badge")).toHaveTextContent("Remaining: 0");
  });

  test("(h) ErrorToast captures xavier-rate-limit window event", () => {
    render(<ErrorToast />);

    act(() => {
      window.dispatchEvent(
        new CustomEvent("xavier-rate-limit", {
          detail: {
            message: "Cloudflare rate limit hit",
            retryAfterSeconds: 30,
            remaining: 0,
          },
        })
      );
    });

    expect(screen.getByText("Cloudflare rate limit hit")).toBeInTheDocument();
    expect(screen.getByTestId("rate-limit-cooldown-badge")).toHaveTextContent("Cooldown: 30s");
  });
});
