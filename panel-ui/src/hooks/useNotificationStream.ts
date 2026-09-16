import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getApiUrl } from "../api/client";
import type { Notification } from "../components/NotificationsDropdown";
import { getApiTokenSync } from "./useApiToken";

export type { Notification };

export interface UseNotificationStreamReturn {
  notifications: Notification[];
  unreadCount: number;
  isConnected: boolean;
  isLoading: boolean;
  error: Error | null;
  markRead: (id: string) => Promise<void>;
  markAllRead: () => Promise<void>;
  refetch: () => Promise<void>;
}

function getToken(): string {
  return getApiTokenSync();
}

/**
 * Custom hook to stream real-time notifications via EventSource (SSE)
 * with exponential backoff auto-reconnection and low-frequency fallback polling.
 */
export function useNotificationStream(): UseNotificationStreamReturn {
  const [notifications, setNotifications] = useState<Notification[]>([]);
  const [isConnected, setIsConnected] = useState<boolean>(false);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [error, setError] = useState<Error | null>(null);

  const eventSourceRef = useRef<EventSource | null>(null);
  const retryCountRef = useRef<number>(0);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fallbackPollTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const isMountedRef = useRef<boolean>(true);

  // Initial and refetch REST call to sync notifications state
  const fetchNotifications = useCallback(async () => {
    try {
      const token = getToken();
      const response = await fetch(getApiUrl("/notifications"), {
        headers: { "X-Xavier-Token": token },
      });

      if (!response.ok) {
        if (response.status === 401) {
          if (isMountedRef.current) setIsLoading(false);
          return;
        }
        throw new Error(`HTTP ${response.status}`);
      }

      const data = await response.json();
      if (isMountedRef.current && Array.isArray(data)) {
        setNotifications(data);
        setError(null);
      }
    } catch (err) {
      if (isMountedRef.current) {
        setError(err instanceof Error ? err : new Error("Failed to fetch notifications"));
      }
    } finally {
      if (isMountedRef.current) {
        setIsLoading(false);
      }
    }
  }, []);

  const handleNewNotification = useCallback((data: unknown) => {
    if (!isMountedRef.current || !data) return;

    let incoming: Notification[] = [];
    if (Array.isArray(data)) {
      incoming = data;
    } else if (typeof data === "object") {
      incoming = [data as Notification];
    }

    if (incoming.length === 0) return;

    setNotifications((prev) => {
      const existingIds = new Set(prev.map((n) => n.id));
      const newItems = incoming.filter((item) => item && item.id && !existingIds.has(item.id));
      if (newItems.length === 0) {
        // If updating an existing item (e.g. read status change from SSE)
        return prev.map((n) => {
          const match = incoming.find((item) => item && item.id === n.id);
          return match ? { ...n, ...match } : n;
        });
      }
      return [...newItems, ...prev];
    });
  }, []);

  // Connect to /notifications/stream via EventSource
  const connectStream = useCallback(() => {
    if (!isMountedRef.current) return;

    // Clean up existing EventSource if any
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }

    const token = getToken();
    const basePath = getApiUrl("/notifications/stream");
    const streamUrl = token
      ? `${basePath}${basePath.includes("?") ? "&" : "?"}token=${encodeURIComponent(token)}`
      : basePath;

    try {
      const es = new EventSource(streamUrl);
      eventSourceRef.current = es;

      es.onopen = () => {
        if (!isMountedRef.current) return;
        setIsConnected(true);
        retryCountRef.current = 0;
        setError(null);
      };

      const processEvent = (event: MessageEvent) => {
        try {
          const parsed = JSON.parse(event.data);
          handleNewNotification(parsed);
        } catch (err) {
          console.error("Failed to parse SSE notification payload:", err);
        }
      };

      es.onmessage = processEvent;
      es.addEventListener("notification", processEvent as EventListener);

      es.onerror = () => {
        if (!isMountedRef.current) return;
        setIsConnected(false);
        es.close();
        eventSourceRef.current = null;

        // Calculate exponential backoff delay: 1s, 2s, 4s, 8s, 16s, max 30s
        const backoffDelay = Math.min(1000 * Math.pow(2, retryCountRef.current), 30000);
        retryCountRef.current += 1;

        if (reconnectTimerRef.current) {
          clearTimeout(reconnectTimerRef.current);
        }

        reconnectTimerRef.current = setTimeout(() => {
          if (isMountedRef.current) {
            connectStream();
          }
        }, backoffDelay);
      };
    } catch (err) {
      if (isMountedRef.current) {
        setIsConnected(false);
        setError(err instanceof Error ? err : new Error("Failed to connect EventSource"));
      }
    }
  }, [handleNewNotification]);

  useEffect(() => {
    isMountedRef.current = true;

    // 1. Initial REST fetch
    fetchNotifications();

    // 2. Connect SSE stream
    connectStream();

    // 3. Fallback low-frequency poll (30s interval) for redundancy / reconnection offline sync
    fallbackPollTimerRef.current = setInterval(() => {
      if (isMountedRef.current) {
        fetchNotifications();
      }
    }, 30000);

    return () => {
      isMountedRef.current = false;
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
        eventSourceRef.current = null;
      }
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      if (fallbackPollTimerRef.current) {
        clearInterval(fallbackPollTimerRef.current);
        fallbackPollTimerRef.current = null;
      }
    };
  }, [connectStream, fetchNotifications]);

  // Synchronize state changes across multiple useNotificationStream hook instances
  useEffect(() => {
    const handleSync = (event: Event) => {
      const custom = event as CustomEvent<{ action: string; id?: string }>;
      if (!custom.detail) return;
      if (custom.detail.action === "mark-all-read") {
        setNotifications((prev) => prev.map((n) => ({ ...n, read: true })));
      } else if (custom.detail.action === "mark-read" && custom.detail.id) {
        const targetId = custom.detail.id;
        setNotifications((prev) =>
          prev.map((n) => (n.id === targetId ? { ...n, read: true } : n))
        );
      }
    };

    if (typeof window !== "undefined") {
      window.addEventListener("xavier:notifications-updated", handleSync);
      return () => {
        window.removeEventListener("xavier:notifications-updated", handleSync);
      };
    }
  }, []);

  const markRead = useCallback(async (id: string) => {
    setNotifications((prev) =>
      prev.map((n) => (n.id === id ? { ...n, read: true } : n))
    );

    if (typeof window !== "undefined") {
      window.dispatchEvent(
        new CustomEvent("xavier:notifications-updated", {
          detail: { action: "mark-read", id },
        })
      );
    }

    try {
      const token = getToken();
      await fetch(getApiUrl(`/notifications/${id}/read`), {
        method: "PATCH",
        headers: { "X-Xavier-Token": token },
      });
    } catch (err) {
      console.error("Failed to mark notification as read:", err);
    }
  }, []);

  const markAllRead = useCallback(async () => {
    setNotifications((prev) => prev.map((n) => ({ ...n, read: true })));

    if (typeof window !== "undefined") {
      window.dispatchEvent(
        new CustomEvent("xavier:notifications-updated", {
          detail: { action: "mark-all-read" },
        })
      );
    }

    try {
      const token = getToken();
      await fetch(getApiUrl("/notifications/read-all"), {
        method: "PATCH",
        headers: { "X-Xavier-Token": token },
      });
    } catch (err) {
      console.error("Failed to mark all notifications as read:", err);
    }
  }, []);

  const unreadCount = useMemo(() => {
    return notifications.reduce((acc, n) => acc + (n.read ? 0 : 1), 0);
  }, [notifications]);

  return {
    notifications,
    unreadCount,
    isConnected,
    isLoading,
    error,
    markRead,
    markAllRead,
    refetch: fetchNotifications,
  };
}
