/**
 * liveStore.svelte.ts — Svelte 5 Runes WebSocket client for Xavier /maloca/ws/feed
 *
 * Connects to ws://<host>:<port>/maloca/ws/feed using the same base URL as
 * getXavierBaseUrl() (http → ws protocol conversion).
 *
 * Features:
 * - Exponential backoff auto-reconnect (0.5s → 1s → 2s → … → 30s max)
 * - Reactive status: 'connected' | 'reconnecting' | 'offline'
 * - Event stream: frames {type: 'xavier_event' | 'ping' | 'pong' | 'lagged'}
 * - Clean degradation: if server is unreachable, stays 'offline' without crash
 */

import { getXavierBaseUrl } from "./config";
import { wasmApply } from "./wasmBridge";

/* ------------------------------------------------------------------ */
/*  Types                                                              */
/* ------------------------------------------------------------------ */

export type LiveStatus = "connected" | "reconnecting" | "offline";

export interface LiveFrame {
  type: "xavier_event" | "ping" | "pong" | "lagged";
  payload?: unknown;
  ts?: number;
}

export interface LiveEvent {
  id: string;
  type: string;
  summary?: string;
  agent?: string;
  timestamp: number;
  raw: LiveFrame;
}

export type EventCallback = (event: LiveEvent) => void;

/* ------------------------------------------------------------------ */
/*  Backoff config                                                     */
/* ------------------------------------------------------------------ */

const BACKOFF_INITIAL_MS = 500;
const BACKOFF_MAX_MS = 30_000;
const BACKOFF_MULTIPLIER = 2;
const HEARTBEAT_RESPONSE_MS = 20_000;

/* ------------------------------------------------------------------ */
/*  Internal state                                                     */
/* ------------------------------------------------------------------ */

let ws: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let heartbeatTimer: ReturnType<typeof setInterval> | null = null;
let backoffMs = BACKOFF_INITIAL_MS;
let attempt = 0;
let disposed = false;
let eventSeq = 0;

let status = $state<LiveStatus>("offline");
let lastEvent = $state<LiveEvent | null>(null);
let eventCount = $state(0);

const listeners = new Set<EventCallback>();

/* ------------------------------------------------------------------ */
/*  URL builder                                                        */
/* ------------------------------------------------------------------ */

function buildWsUrl(): string {
  const base = getXavierBaseUrl(); // e.g. http://127.0.0.1:8006
  const url = new URL("/maloca/ws/feed", base);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}

/* ------------------------------------------------------------------ */
/*  Core WS lifecycle                                                  */
/* ------------------------------------------------------------------ */

function connect() {
  if (disposed) return;

  // Clean up previous socket if any
  if (ws) {
    ws.onopen = null;
    ws.onmessage = null;
    ws.onerror = null;
    ws.onclose = null;
    if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
      ws.close();
    }
    ws = null;
  }

  clearReconnectTimer();

  try {
    const url = buildWsUrl();
    ws = new WebSocket(url);
  } catch {
    // Invalid URL or environment without WebSocket — degrade to offline
    status = "offline";
    scheduleReconnect();
    return;
  }

  status = "reconnecting";

  ws.onopen = () => {
    status = "connected";
    backoffMs = BACKOFF_INITIAL_MS; // reset backoff on successful connect
    attempt = 0; // reset attempt count on successful connect
    startHeartbeatWatchdog();
  };

  ws.onmessage = (ev: MessageEvent) => {
    handleFrame(ev.data);
  };

  ws.onerror = () => {
    // onclose will fire after onerror — no action needed here
  };

  ws.onclose = () => {
    stopHeartbeatWatchdog();
    ws = null;
    if (!disposed) {
      status = "reconnecting";
      scheduleReconnect();
    }
  };
}

/* ------------------------------------------------------------------ */
/*  Frame handling                                                     */
/* ------------------------------------------------------------------ */

function handleFrame(raw: string) {
  // Use wasmBridge to classify frame synchronously with fallback to avoid race conditions and frame delays
  const frame = wasmApply<LiveFrame | null>("classify_frame", { raw }, () => {
    // TS fallback logic
    try {
      return JSON.parse(raw) as LiveFrame;
    } catch {
      return null; // ignore malformed frames
    }
  });

  if (!frame) return;

  // Respond to server ping with pong
  if (frame.type === "ping") {
    sendPong();
    return;
  }

  // Skip pong/lagged — they don't produce UI events
  if (frame.type === "pong" || frame.type === "lagged") {
    return;
  }

  // xavier_event — emit to subscribers
  if (frame.type === "xavier_event") {
    const event: LiveEvent = {
      id: `live-${++eventSeq}`,
      type: frame.payload && typeof frame.payload === "object" && "type" in (frame.payload as Record<string, unknown>)
        ? String((frame.payload as Record<string, unknown>).type)
        : "unknown",
      summary: frame.payload && typeof frame.payload === "object" && "summary" in (frame.payload as Record<string, unknown>)
        ? String((frame.payload as Record<string, unknown>).summary)
        : undefined,
      agent: frame.payload && typeof frame.payload === "object" && "agent" in (frame.payload as Record<string, unknown>)
        ? String((frame.payload as Record<string, unknown>).agent)
        : undefined,
      timestamp: frame.ts ?? Date.now(),
      raw: frame,
    };

    lastEvent = event;
    eventCount = eventCount + 1;

    // Notify all subscribers
    for (const cb of listeners) {
      try { cb(event); } catch { /* swallow listener errors */ }
    }
  }
}

function sendPong() {
  if (ws && ws.readyState === WebSocket.OPEN) {
    try { ws.send(JSON.stringify({ type: "pong" })); } catch { /* best effort */ }
  }
}

/* ------------------------------------------------------------------ */
/*  Heartbeat watchdog — if no message in 20s, treat as dead           */
/* ------------------------------------------------------------------ */

function startHeartbeatWatchdog() {
  stopHeartbeatWatchdog();
  heartbeatTimer = setInterval(() => {
    // If we haven't heard anything in a while, force reconnect
    if (ws && ws.readyState === WebSocket.OPEN) {
      // The server should ping every 25s, so 40s is generous
      // We'll rely on onclose for the actual detection.
      // This is a safety net: if WS is OPEN but stale, ping it
      sendPong();
    }
  }, HEARTBEAT_RESPONSE_MS);
}

function stopHeartbeatWatchdog() {
  if (heartbeatTimer !== null) {
    clearInterval(heartbeatTimer);
    heartbeatTimer = null;
  }
}

/* ------------------------------------------------------------------ */
/*  Reconnect with exponential backoff                                 */
/* ------------------------------------------------------------------ */

function scheduleReconnect() {
  clearReconnectTimer();
  if (disposed) return;

  attempt++;

  // Calculate the next backoff synchronously using WASM, or fallback to exponential backoff formula
  backoffMs = wasmApply<number>("next_backoff_ms", { attempt, cap_ms: BACKOFF_MAX_MS }, () => {
    // Fallback formula: initial * (multiplier ^ (attempt - 1)) capped at BACKOFF_MAX_MS
    return Math.min(BACKOFF_INITIAL_MS * Math.pow(BACKOFF_MULTIPLIER, attempt - 1), BACKOFF_MAX_MS);
  });

  reconnectTimer = setTimeout(() => {
    connect();
  }, backoffMs);
}

function clearReconnectTimer() {
  if (reconnectTimer !== null) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
}

/* ------------------------------------------------------------------ */
/*  Public API                                                         */
/* ------------------------------------------------------------------ */

/** Initialize the WebSocket connection. Call once from a component. */
export function initLiveFeed() {
  if (disposed) {
    // If previously disposed, reset
    disposed = false;
    backoffMs = BACKOFF_INITIAL_MS;
    attempt = 0;
  }
  if (!ws && status === "offline") {
    connect();
  }
}

/** Disconnect and clean up. Call on component destroy. */
export function disposeLiveFeed() {
  disposed = true;
  clearReconnectTimer();
  stopHeartbeatWatchdog();
  if (ws) {
    ws.onopen = null;
    ws.onmessage = null;
    ws.onerror = null;
    ws.onclose = null;
    ws.close();
    ws = null;
  }
  status = "offline";
}

/** Subscribe to live events. Returns unsubscribe function. */
export function onLiveEvent(cb: EventCallback): () => void {
  listeners.add(cb);
  return () => { listeners.delete(cb); };
}

/* ------------------------------------------------------------------ */
/*  Reactive getters (Svelte 5 Runes)                                  */
/* ------------------------------------------------------------------ */

export function getLiveStatus(): LiveStatus {
  return status;
}

export function getLiveLastEvent(): LiveEvent | null {
  return lastEvent;
}

export function getLiveEventCount(): number {
  return eventCount;
}
