import { useEffect, useRef } from "react";

interface XUIEvent {
  type: "action" | "submit" | "change";
  componentId: string;
  componentType: string;
  payload: Record<string, unknown>;
  timestamp: number;
}

export function useXavierWebSocket(threadId: string | null) {
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!threadId) return;

    const wsUrl = `ws://localhost:8006/ws/panel/${threadId}`;
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => {
      console.log("[XavierWS] Connected to thread:", threadId);
    };

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        console.log("[XavierWS] Received:", data);
      } catch (e) {
        console.error("[XavierWS] Invalid message:", event.data);
      }
    };

    ws.onerror = (error) => {
      console.error("[XavierWS] Error:", error);
    };

    ws.onclose = () => {
      console.log("[XavierWS] Disconnected");
    };

    return () => {
      ws.close();
      wsRef.current = null;
    };
  }, [threadId]);

  const sendXUIEvent = (event: XUIEvent) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      const { type, ...rest } = event;
      wsRef.current.send(
        JSON.stringify({
          type: "xui_event",
          event_type: type,
          ...rest,
          thread_id: threadId,
        }),
      );
      console.log("[XavierWS] Sent XUI event:", event);
    } else {
      console.warn("[XavierWS] Not connected, queuing event");
      // Could queue events for retry
    }
  };

  return { sendXUIEvent, ws: wsRef };
}

export default useXavierWebSocket;
