import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useTelecomSocket } from "../src/hooks/useTelecomSocket";

class FakeWebSocket {
  static OPEN = 1;
  static instances: FakeWebSocket[] = [];
  readyState = 0;
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((e: MessageEvent) => void) | null = null;
  constructor(public url: string) {
    FakeWebSocket.instances.push(this);
  }
  send() {}
  close() {}
}

describe("useTelecomSocket connection stability", () => {
  beforeEach(() => {
    FakeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeWebSocket);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("opens one socket across re-renders with the default ping payload", () => {
    const { rerender } = renderHook(() => useTelecomSocket("node-a"));
    rerender();
    rerender();
    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it("does not reconnect when the caller passes an inline ping payload", () => {
    const { rerender } = renderHook(() =>
      useTelecomSocket("node-a", { pingPayload: { event_type: "ping" } }),
    );
    rerender();
    rerender();
    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it("reconnects when the client id changes", () => {
    const { rerender } = renderHook(({ id }) => useTelecomSocket(id), {
      initialProps: { id: "node-a" },
    });
    rerender({ id: "node-b" });
    expect(FakeWebSocket.instances).toHaveLength(2);
    expect(FakeWebSocket.instances[1].url).toContain("/telecom/ws/node-b");
  });
});
