import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useNotificationStream } from "../src/hooks/useNotificationStream";

// Mock EventSource class
class MockEventSource {
  public url: string;
  public readyState: number = 0;
  public onopen: ((e: Event) => void) | null = null;
  public onmessage: ((e: MessageEvent) => void) | null = null;
  public onerror: ((e: Event) => void) | null = null;
  private listeners: Record<string, Array<(e: MessageEvent) => void>> = {};

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }

  static instances: MockEventSource[] = [];

  addEventListener(type: string, listener: (e: MessageEvent) => void) {
    if (!this.listeners[type]) {
      this.listeners[type] = [];
    }
    this.listeners[type].push(listener);
  }

  removeEventListener(type: string, listener: (e: MessageEvent) => void) {
    if (this.listeners[type]) {
      this.listeners[type] = this.listeners[type].filter((l) => l !== listener);
    }
  }

  close() {
    this.readyState = 2;
  }

  // Helper to simulate connection open
  simulateOpen() {
    this.readyState = 1;
    if (this.onopen) {
      this.onopen(new Event("open"));
    }
  }

  // Helper to simulate incoming SSE message
  simulateMessage(data: any, eventType?: string) {
    const event = new MessageEvent(eventType || "message", {
      data: JSON.stringify(data),
    });
    if (this.onmessage) {
      this.onmessage(event);
    }
    if (eventType && this.listeners[eventType]) {
      for (const listener of this.listeners[eventType]) {
        listener(event);
      }
    }
  }

  // Helper to simulate connection error
  simulateError() {
    if (this.onerror) {
      this.onerror(new Event("error"));
    }
  }
}

describe("useNotificationStream", () => {
  const originalEventSource = globalThis.EventSource;
  const originalFetch = globalThis.fetch;

  beforeEach(() => {
    MockEventSource.instances = [];
    // @ts-expect-error Mocking global EventSource
    globalThis.EventSource = MockEventSource;

    vi.stubGlobal("fetch", vi.fn().mockImplementation((url: string) => {
      if (url.includes("/notifications")) {
        return Promise.resolve({
          ok: true,
          status: 200,
          json: () =>
            Promise.resolve([
              {
                id: "notif-1",
                islandId: "system",
                title: "System Update",
                body: "System started successfully",
                timestamp: new Date().toISOString(),
                read: false,
                severity: "info",
              },
            ]),
        });
      }
      return Promise.resolve({ ok: true, json: () => Promise.resolve({}) });
    }));
  });

  afterEach(() => {
    globalThis.EventSource = originalEventSource;
    globalThis.fetch = originalFetch;
    vi.restoreAllMocks();
  });

  it("fetches initial notifications and connects EventSource stream", async () => {
    const { result } = renderHook(() => useNotificationStream());

    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
    });

    expect(result.current.notifications.length).toBe(1);
    expect(result.current.notifications[0].id).toBe("notif-1");
    expect(result.current.unreadCount).toBe(1);

    expect(MockEventSource.instances.length).toBeGreaterThan(0);
    const es = MockEventSource.instances[0];
    expect(es.url).toContain("/notifications/stream");

    act(() => {
      es.simulateOpen();
    });

    expect(result.current.isConnected).toBe(true);
  });

  it("receives real-time SSE notifications and updates unread count", async () => {
    const { result } = renderHook(() => useNotificationStream());

    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
    });

    const es = MockEventSource.instances[0];
    act(() => {
      es.simulateOpen();
      es.simulateMessage({
        id: "notif-2",
        islandId: "errors",
        title: "Database Alert",
        body: "Connection pool exhausted",
        timestamp: new Date().toISOString(),
        read: false,
        severity: "error",
      });
    });

    expect(result.current.notifications.length).toBe(2);
    expect(result.current.notifications[0].id).toBe("notif-2");
    expect(result.current.unreadCount).toBe(2);
  });

  it("marks notification as read and calls PATCH API", async () => {
    const { result } = renderHook(() => useNotificationStream());

    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
    });

    await act(async () => {
      await result.current.markRead("notif-1");
    });

    expect(result.current.notifications[0].read).toBe(true);
    expect(result.current.unreadCount).toBe(0);
    expect(globalThis.fetch).toHaveBeenCalledWith(
      expect.stringContaining("/notifications/notif-1/read"),
      expect.objectContaining({ method: "PATCH" }),
    );
  });

  it("marks all notifications as read and calls PATCH read-all API", async () => {
    const { result } = renderHook(() => useNotificationStream());

    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
    });

    await act(async () => {
      await result.current.markAllRead();
    });

    expect(result.current.notifications.every((n) => n.read)).toBe(true);
    expect(result.current.unreadCount).toBe(0);
    expect(globalThis.fetch).toHaveBeenCalledWith(
      expect.stringContaining("/notifications/read-all"),
      expect.objectContaining({ method: "PATCH" }),
    );
  });

  it("reconnects with exponential backoff on stream error", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });

    const { result } = renderHook(() => useNotificationStream());

    await waitFor(() => {
      expect(result.current.isLoading).toBe(false);
    });

    const initialInstancesCount = MockEventSource.instances.length;
    const es = MockEventSource.instances[0];

    act(() => {
      es.simulateError();
    });

    expect(result.current.isConnected).toBe(false);

    // Fast-forward time by 1000ms for first backoff reconnect
    act(() => {
      vi.advanceTimersByTime(1050);
    });

    expect(MockEventSource.instances.length).toBe(initialInstancesCount + 1);

    vi.useRealTimers();
  });
});
