import { useCallback, useEffect, useRef, useState } from "react";
import { getApiUrl } from "../api/client";
import { getApiTokenSync } from "./useApiToken";

/**
 * Telecom protocol frame envelope representing incoming/outgoing WebSocket messages.
 */
export interface TelecomFrame {
  event_type?: string;
  room_id?: string;
  sender?: string;
  recipient_id?: string;
  payload?: unknown;
  timestamp?: string | number;
  header?: {
    version?: number;
    channel_type?: number | string;
    sequence?: number;
    timestamp_ms?: number;
    sender_node_id?: string;
    recipient_id?: string;
    payload_checksum?: number;
  };
  [key: string]: unknown;
}

/**
 * Options for configuring the `useTelecomSocket` hook.
 */
export interface UseTelecomSocketOptions {
  /** Optional callback triggered on every valid received WebSocket frame. */
  onFrame?: (frame: TelecomFrame) => void;
  /** Whether to automatically connect on mount or when `clientId` changes (default: true). */
  autoConnect?: boolean;
  /** Heartbeat interval in milliseconds (default: 30000). Set to 0 to disable keepalive. */
  heartbeatIntervalMs?: number;
  /** Base delay in milliseconds for exponential reconnect backoff (default: 1000). */
  baseBackoffMs?: number;
  /** Maximum backoff delay cap in milliseconds (default: 30000). */
  maxBackoffMs?: number;
  /** Custom ping payload sent during heartbeat intervals. */
  pingPayload?: Record<string, unknown> | string;
}

/**
 * Hook return type offering connection state, frame helpers, and socket controls.
 */
export interface UseTelecomSocketReturn {
  isConnected: boolean;
  isConnecting: boolean;
  lastFrame: TelecomFrame | null;
  error: Error | null;
  reconnectCount: number;
  sendFrame: (frame: TelecomFrame | Record<string, unknown>) => boolean;
  sendRaw: (data: string | ArrayBuffer | Blob) => boolean;
  connect: () => void;
  disconnect: () => void;
}

/**
 * Calculates exponential backoff reconnect delay with upper cap.
 *
 * @param attempt Retry attempt index (0-based)
 * @param baseMs Base backoff delay in milliseconds (default: 1000)
 * @param maxMs Maximum backoff delay cap in milliseconds (default: 30000)
 * @returns Delay in milliseconds
 */
export function reconnectBackoff(
  attempt: number,
  baseMs = 1000,
  maxMs = 30000,
): number {
  const delay = baseMs * Math.pow(2, attempt);
  return Math.min(delay, maxMs);
}

/**
 * Converts an HTTP(S) API URL or path into a WebSocket WS(S) URL for telecom streaming.
 */
function buildTelecomWsUrl(clientId: string): string {
  const relativeOrAbsolute = getApiUrl(`/telecom/ws/${encodeURIComponent(clientId)}`);
  const token = getApiTokenSync();

  let wsUrl = relativeOrAbsolute;
  if (typeof window !== "undefined") {
    if (relativeOrAbsolute.startsWith("/")) {
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      wsUrl = `${protocol}//${window.location.host}${relativeOrAbsolute}`;
    } else if (relativeOrAbsolute.startsWith("http://")) {
      wsUrl = relativeOrAbsolute.replace(/^http:\/\//i, "ws://");
    } else if (relativeOrAbsolute.startsWith("https://")) {
      wsUrl = relativeOrAbsolute.replace(/^https:\/\//i, "wss://");
    }
  }

  if (token) {
    const separator = wsUrl.includes("?") ? "&" : "?";
    wsUrl = `${wsUrl}${separator}token=${encodeURIComponent(token)}`;
  }

  return wsUrl;
}

/**
 * React custom hook for managing resilient WebSocket connections to the telecom gateway
 * at `/telecom/ws/:client_id`, handling heartbeat ping/pong keepalives, exponential
 * backoff auto-reconnection, and frame dispatching.
 *
 * @param clientId Target client or node identifier
 * @param options Configuration options for heartbeats, callbacks, and reconnection logic
 */
export function useTelecomSocket(
  clientId: string | null,
  options: UseTelecomSocketOptions = {},
): UseTelecomSocketReturn {
  const {
    onFrame,
    autoConnect = true,
    heartbeatIntervalMs = 30000,
    baseBackoffMs = 1000,
    maxBackoffMs = 30000,
    pingPayload = { event_type: "ping" },
  } = options;

  const [isConnected, setIsConnected] = useState<boolean>(false);
  const [isConnecting, setIsConnecting] = useState<boolean>(false);
  const [lastFrame, setLastFrame] = useState<TelecomFrame | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const [reconnectCount, setReconnectCount] = useState<number>(0);

  const socketRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const heartbeatTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const isMountedRef = useRef<boolean>(true);
  const isManualDisconnectRef = useRef<boolean>(false);
  const reconnectCountRef = useRef<number>(0);

  // Keep callback refs stable across renders to avoid tearing or unnecessary re-connections
  const onFrameRef = useRef(onFrame);
  useEffect(() => {
    onFrameRef.current = onFrame;
  }, [onFrame]);

  const clearTimers = useCallback(() => {
    if (reconnectTimerRef.current) {
      clearTimeout(reconnectTimerRef.current);
      reconnectTimerRef.current = null;
    }
    if (heartbeatTimerRef.current) {
      clearInterval(heartbeatTimerRef.current);
      heartbeatTimerRef.current = null;
    }
  }, []);

  const disconnect = useCallback(() => {
    isManualDisconnectRef.current = true;
    clearTimers();

    if (socketRef.current) {
      // Unbind handlers to avoid triggering auto-reconnect on deliberate close
      socketRef.current.onopen = null;
      socketRef.current.onclose = null;
      socketRef.current.onerror = null;
      socketRef.current.onmessage = null;
      socketRef.current.close();
      socketRef.current = null;
    }

    if (isMountedRef.current) {
      setIsConnected(false);
      setIsConnecting(false);
    }
  }, [clearTimers]);

  const connect = useCallback(() => {
    if (!clientId || !isMountedRef.current) return;

    // Close any prior socket instance
    disconnect();
    isManualDisconnectRef.current = false;

    if (isMountedRef.current) {
      setIsConnecting(true);
      setError(null);
    }

    try {
      const wsUrl = buildTelecomWsUrl(clientId);
      const socket = new WebSocket(wsUrl);
      socketRef.current = socket;

      socket.onopen = () => {
        if (!isMountedRef.current) return;

        setIsConnected(true);
        setIsConnecting(false);
        setError(null);
        reconnectCountRef.current = 0;
        setReconnectCount(0);

        // Start periodic heartbeat keepalive if configured
        if (heartbeatIntervalMs > 0) {
          heartbeatTimerRef.current = setInterval(() => {
            if (socketRef.current?.readyState === WebSocket.OPEN) {
              const pingMsg =
                typeof pingPayload === "string"
                  ? pingPayload
                  : JSON.stringify(pingPayload);
              socketRef.current.send(pingMsg);
            }
          }, heartbeatIntervalMs);
        }
      };

      socket.onmessage = (event: MessageEvent) => {
        if (!isMountedRef.current) return;

        try {
          let parsed: TelecomFrame;
          if (typeof event.data === "string") {
            // Ignore pong responses or plain text acknowledgments if appropriate
            if (event.data === "pong" || event.data === "ack") return;
            parsed = JSON.parse(event.data);
          } else {
            // Non-string or binary frame
            parsed = { payload: event.data };
          }

          // Filter out pong frames from state if received as JSON object
          if (parsed.event_type === "pong") return;

          setLastFrame(parsed);
          if (onFrameRef.current) {
            onFrameRef.current(parsed);
          }
        } catch (_err) {
          // If frame is raw string or non-JSON, frame as generic text payload
          const rawFrame: TelecomFrame = { payload: event.data };
          setLastFrame(rawFrame);
          if (onFrameRef.current) {
            onFrameRef.current(rawFrame);
          }
        }
      };

      socket.onerror = (_evt: Event) => {
        if (!isMountedRef.current) return;
        const err = new Error("Telecom WebSocket connection error");
        setError(err);
      };

      socket.onclose = () => {
        if (!isMountedRef.current) return;

        setIsConnected(false);
        setIsConnecting(false);
        clearTimers();

        // Trigger exponential backoff reconnection if connection fell unexpectedly
        if (!isManualDisconnectRef.current) {
          const currentAttempt = reconnectCountRef.current;
          const delay = reconnectBackoff(currentAttempt, baseBackoffMs, maxBackoffMs);

          reconnectCountRef.current += 1;
          setReconnectCount(reconnectCountRef.current);

          reconnectTimerRef.current = setTimeout(() => {
            if (isMountedRef.current && !isManualDisconnectRef.current) {
              connect();
            }
          }, delay);
        }
      };
    } catch (err) {
      if (isMountedRef.current) {
        setIsConnected(false);
        setIsConnecting(false);
        setError(
          err instanceof Error
            ? err
            : new Error("Failed to initialize Telecom WebSocket"),
        );
      }
    }
  }, [
    clientId,
    disconnect,
    clearTimers,
    heartbeatIntervalMs,
    pingPayload,
    baseBackoffMs,
    maxBackoffMs,
  ]);

  useEffect(() => {
    isMountedRef.current = true;

    if (autoConnect && clientId) {
      connect();
    }

    return () => {
      isMountedRef.current = false;
      disconnect();
    };
  }, [clientId, autoConnect, connect, disconnect]);

  const sendFrame = useCallback(
    (frame: TelecomFrame | Record<string, unknown>): boolean => {
      if (socketRef.current?.readyState === WebSocket.OPEN) {
        socketRef.current.send(JSON.stringify(frame));
        return true;
      }
      return false;
    },
    [],
  );

  const sendRaw = useCallback((data: string | ArrayBuffer | Blob): boolean => {
    if (socketRef.current?.readyState === WebSocket.OPEN) {
      socketRef.current.send(data);
      return true;
    }
    return false;
  }, []);

  return {
    isConnected,
    isConnecting,
    lastFrame,
    error,
    reconnectCount,
    sendFrame,
    sendRaw,
    connect,
    disconnect,
  };
}

export default useTelecomSocket;
