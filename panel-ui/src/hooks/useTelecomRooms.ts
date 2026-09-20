import { useCallback, useEffect, useMemo, useState } from "react";

/**
 * Interface representing a Telecom Chat Room or Direct Peer Channel.
 */
export interface TelecomRoom {
  id: string;
  name: string;
  type: "room" | "direct";
  unreadCount: number;
  lastActiveTimestamp: number;
  description?: string;
  alias?: string;
  nodeId?: string;
  online?: boolean;
}

/**
 * Interface representing a Telecom Chat Message with optimistic delivery state.
 */
export interface TelecomMessage {
  id: string;
  roomId: string;
  senderAlias: string;
  senderNodeId: string;
  content: string;
  timestamp: string;
  encrypted: boolean;
  isSelf: boolean;
  pending?: boolean;
  failed?: boolean;
  attachment?: {
    name: string;
    size?: string;
    type?: string;
  };
}

export const TELECOM_ROOMS_STORAGE_KEY = "xavier_telecom_rooms";
export const TELECOM_MESSAGES_STORAGE_KEY = "xavier_telecom_messages";
export const TELECOM_ROOMS_EVENT = "xavier_telecom_rooms_changed";

const DEFAULT_ROOMS: TelecomRoom[] = [
  {
    id: "room-general",
    name: "general",
    type: "room",
    unreadCount: 2,
    lastActiveTimestamp: Date.now() - 1000 * 60 * 10,
    description: "Mesh-wide broadcast channel for general discussion",
  },
  {
    id: "room-dev-council",
    name: "dev-council",
    type: "room",
    unreadCount: 0,
    lastActiveTimestamp: Date.now() - 1000 * 60 * 60,
    description: "Technical alignment and architecture updates",
  },
  {
    id: "peer-alpha",
    name: "Node Alpha",
    alias: "Node Alpha",
    nodeId: "node-a7f92b",
    type: "direct",
    online: true,
    unreadCount: 1,
    lastActiveTimestamp: Date.now() - 1000 * 60 * 5,
  },
  {
    id: "peer-beta",
    name: "Peer Beta",
    alias: "Peer Beta",
    nodeId: "node-c3e811",
    type: "direct",
    online: false,
    unreadCount: 0,
    lastActiveTimestamp: Date.now() - 1000 * 60 * 120,
  },
];

const DEFAULT_MESSAGES: Record<string, TelecomMessage[]> = {
  "room-general": [
    {
      id: "msg-1",
      roomId: "room-general",
      senderAlias: "Node Alpha",
      senderNodeId: "node-a7f92b",
      content: "Encrypted P2P connection established across the mesh.",
      timestamp: "10:42 AM",
      encrypted: true,
      isSelf: false,
    },
    {
      id: "msg-2",
      roomId: "room-general",
      senderAlias: "Local Node",
      senderNodeId: "node-local",
      content: "Ack. Data node sync is active and healthy.",
      timestamp: "10:43 AM",
      encrypted: true,
      isSelf: true,
    },
  ],
  "room-dev-council": [
    {
      id: "msg-3",
      roomId: "room-dev-council",
      senderAlias: "Peer Beta",
      senderNodeId: "node-c3e811",
      content: "Proposed code graph index update ready for peer verification.",
      timestamp: "09:15 AM",
      encrypted: true,
      isSelf: false,
    },
  ],
  "peer-alpha": [
    {
      id: "msg-4",
      roomId: "peer-alpha",
      senderAlias: "Node Alpha",
      senderNodeId: "node-a7f92b",
      content: "Hey, sending you the encrypted vector index chunk.",
      timestamp: "11:05 AM",
      encrypted: true,
      isSelf: false,
      attachment: {
        name: "chunk_0482.idx",
        size: "1.4 MB",
        type: "application/octet-stream",
      },
    },
  ],
};

export function getStoredRooms(): TelecomRoom[] {
  if (typeof window === "undefined" || !window.localStorage) {
    return DEFAULT_ROOMS;
  }
  try {
    const raw = localStorage.getItem(TELECOM_ROOMS_STORAGE_KEY);
    if (!raw) return DEFAULT_ROOMS;
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.length > 0) {
      return parsed.map((item) => ({
        id: String(item.id || ""),
        name: String(item.name || "unnamed"),
        type: item.type === "direct" ? "direct" : "room",
        unreadCount: typeof item.unreadCount === "number" ? Math.max(0, item.unreadCount) : 0,
        lastActiveTimestamp: typeof item.lastActiveTimestamp === "number" ? item.lastActiveTimestamp : Date.now(),
        description: item.description ? String(item.description) : undefined,
        alias: item.alias ? String(item.alias) : undefined,
        nodeId: item.nodeId ? String(item.nodeId) : undefined,
        online: typeof item.online === "boolean" ? item.online : undefined,
      }));
    }
    return DEFAULT_ROOMS;
  } catch {
    return DEFAULT_ROOMS;
  }
}

export function saveStoredRooms(rooms: TelecomRoom[]): void {
  if (typeof window === "undefined" || !window.localStorage) return;
  try {
    localStorage.setItem(TELECOM_ROOMS_STORAGE_KEY, JSON.stringify(rooms));
    window.dispatchEvent(new Event(TELECOM_ROOMS_EVENT));
  } catch (e) {
    console.warn("Failed to persist telecom rooms:", e);
  }
}

export function getStoredMessages(): Record<string, TelecomMessage[]> {
  if (typeof window === "undefined" || !window.localStorage) {
    return DEFAULT_MESSAGES;
  }
  try {
    const raw = localStorage.getItem(TELECOM_MESSAGES_STORAGE_KEY);
    if (!raw) return DEFAULT_MESSAGES;
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed;
    }
    return DEFAULT_MESSAGES;
  } catch {
    return DEFAULT_MESSAGES;
  }
}

export function saveStoredMessages(messages: Record<string, TelecomMessage[]>): void {
  if (typeof window === "undefined" || !window.localStorage) return;
  try {
    localStorage.setItem(TELECOM_MESSAGES_STORAGE_KEY, JSON.stringify(messages));
    window.dispatchEvent(new Event(TELECOM_ROOMS_EVENT));
  } catch (e) {
    console.warn("Failed to persist telecom messages:", e);
  }
}

/**
 * Custom React hook for managing telecom chat rooms, unread message count badges,
 * last active timestamps, optimistic message delivery, and local storage synchronization.
 */
export function useTelecomRooms(initialActiveRoomId = "room-general") {
  const [rooms, setRoomsState] = useState<TelecomRoom[]>(getStoredRooms);
  const [messages, setMessagesState] = useState<Record<string, TelecomMessage[]>>(getStoredMessages);
  const [activeRoomId, setActiveRoomIdState] = useState<string | null>(initialActiveRoomId);

  // Cross-tab and local event synchronization
  useEffect(() => {
    const handleSync = () => {
      setRoomsState(getStoredRooms());
      setMessagesState(getStoredMessages());
    };

    window.addEventListener("storage", handleSync);
    window.addEventListener(TELECOM_ROOMS_EVENT, handleSync);

    return () => {
      window.removeEventListener("storage", handleSync);
      window.removeEventListener(TELECOM_ROOMS_EVENT, handleSync);
    };
  }, []);

  const updateRooms = useCallback((updater: (prev: TelecomRoom[]) => TelecomRoom[]) => {
    setRoomsState((prev) => {
      const next = updater(prev);
      saveStoredRooms(next);
      return next;
    });
  }, []);

  const updateMessages = useCallback(
    (updater: (prev: Record<string, TelecomMessage[]>) => Record<string, TelecomMessage[]>) => {
      setMessagesState((prev) => {
        const next = updater(prev);
        saveStoredMessages(next);
        return next;
      });
    },
    []
  );

  // Active room selection & automatic reading
  const setActiveRoomId = useCallback(
    (id: string | null) => {
      setActiveRoomIdState(id);
      if (id) {
        updateRooms((prev) =>
          prev.map((r) => (r.id === id ? { ...r, unreadCount: 0, lastActiveTimestamp: Date.now() } : r))
        );
      }
    },
    [updateRooms]
  );

  // Unread badge counter reducer pattern
  const markRoomAsRead = useCallback(
    (roomId: string) => {
      updateRooms((prev) =>
        prev.map((r) => (r.id === roomId ? { ...r, unreadCount: 0 } : r))
      );
    },
    [updateRooms]
  );

  const incrementUnread = useCallback(
    (roomId: string, amount = 1) => {
      if (roomId === activeRoomId) return;
      updateRooms((prev) =>
        prev.map((r) =>
          r.id === roomId
            ? { ...r, unreadCount: r.unreadCount + amount, lastActiveTimestamp: Date.now() }
            : r
        )
      );
    },
    [activeRoomId, updateRooms]
  );

  // Total unread count aggregator
  const totalUnreadCount = useMemo(() => {
    return rooms.reduce((sum, r) => sum + (r.unreadCount || 0), 0);
  }, [rooms]);

  // Active room object
  const activeRoom = useMemo(() => {
    return rooms.find((r) => r.id === activeRoomId);
  }, [rooms, activeRoomId]);

  // Active room messages
  const activeRoomMessages = useMemo(() => {
    if (!activeRoomId) return [];
    return messages[activeRoomId] || [];
  }, [messages, activeRoomId]);

  // Optimistic Message Appending
  const sendMessageOptimistic = useCallback(
    (
      roomId: string,
      content: string,
      options?: {
        senderAlias?: string;
        senderNodeId?: string;
        attachment?: TelecomMessage["attachment"];
      }
    ): TelecomMessage => {
      const now = new Date();
      const timestampStr = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      const nowMs = now.getTime();

      const newMsg: TelecomMessage = {
        id: `msg-${nowMs}-${Math.random().toString(36).substring(2, 7)}`,
        roomId,
        senderAlias: options?.senderAlias || "Local Node",
        senderNodeId: options?.senderNodeId || "node-local",
        content,
        timestamp: timestampStr,
        encrypted: true,
        isSelf: true,
        pending: true,
        failed: false,
        attachment: options?.attachment,
      };

      updateMessages((prev) => {
        const roomMsgs = prev[roomId] || [];
        return {
          ...prev,
          [roomId]: [...roomMsgs, newMsg],
        };
      });

      updateRooms((prev) =>
        prev.map((r) => (r.id === roomId ? { ...r, lastActiveTimestamp: nowMs } : r))
      );

      return newMsg;
    },
    [updateMessages, updateRooms]
  );

  const markMessageDelivered = useCallback(
    (roomId: string, messageId: string) => {
      updateMessages((prev) => {
        const roomMsgs = prev[roomId];
        if (!roomMsgs) return prev;
        return {
          ...prev,
          [roomId]: roomMsgs.map((m) =>
            m.id === messageId ? { ...m, pending: false, failed: false } : m
          ),
        };
      });
    },
    [updateMessages]
  );

  const markMessageFailed = useCallback(
    (roomId: string, messageId: string) => {
      updateMessages((prev) => {
        const roomMsgs = prev[roomId];
        if (!roomMsgs) return prev;
        return {
          ...prev,
          [roomId]: roomMsgs.map((m) =>
            m.id === messageId ? { ...m, pending: false, failed: true } : m
          ),
        };
      });
    },
    [updateMessages]
  );

  // Incoming message handler with auto unread reducer logic
  const receiveMessage = useCallback(
    (
      roomId: string,
      messagePayload: Omit<TelecomMessage, "id" | "roomId" | "timestamp"> & {
        id?: string;
        timestamp?: string;
      }
    ) => {
      const now = new Date();
      const timestampStr = messagePayload.timestamp || now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      const nowMs = now.getTime();

      const newMsg: TelecomMessage = {
        id: messagePayload.id || `msg-${nowMs}-${Math.random().toString(36).substring(2, 7)}`,
        roomId,
        senderAlias: messagePayload.senderAlias,
        senderNodeId: messagePayload.senderNodeId,
        content: messagePayload.content,
        timestamp: timestampStr,
        encrypted: messagePayload.encrypted ?? true,
        isSelf: messagePayload.isSelf ?? false,
        attachment: messagePayload.attachment,
      };

      updateMessages((prev) => {
        const roomMsgs = prev[roomId] || [];
        return {
          ...prev,
          [roomId]: [...roomMsgs, newMsg],
        };
      });

      const isCurrentActive = roomId === activeRoomId;
      updateRooms((prev) =>
        prev.map((r) => {
          if (r.id !== roomId) return r;
          return {
            ...r,
            lastActiveTimestamp: nowMs,
            unreadCount: isCurrentActive ? 0 : r.unreadCount + 1,
          };
        })
      );
    },
    [activeRoomId, updateMessages, updateRooms]
  );

  // Room Management Actions
  const addRoom = useCallback(
    (roomData: Omit<TelecomRoom, "unreadCount" | "lastActiveTimestamp"> & Partial<TelecomRoom>): TelecomRoom => {
      const nowMs = Date.now();
      const newRoom: TelecomRoom = {
        id: roomData.id,
        name: roomData.name,
        type: roomData.type,
        unreadCount: roomData.unreadCount ?? 0,
        lastActiveTimestamp: roomData.lastActiveTimestamp ?? nowMs,
        description: roomData.description,
        alias: roomData.alias,
        nodeId: roomData.nodeId,
        online: roomData.online,
      };

      updateRooms((prev) => {
        const exists = prev.some((r) => r.id === newRoom.id);
        if (exists) return prev.map((r) => (r.id === newRoom.id ? { ...r, ...newRoom } : r));
        return [...prev, newRoom];
      });

      return newRoom;
    },
    [updateRooms]
  );

  const updateRoom = useCallback(
    (roomId: string, updates: Partial<TelecomRoom>) => {
      updateRooms((prev) =>
        prev.map((r) => (r.id === roomId ? { ...r, ...updates } : r))
      );
    },
    [updateRooms]
  );

  const removeRoom = useCallback(
    (roomId: string) => {
      updateRooms((prev) => prev.filter((r) => r.id !== roomId));
      updateMessages((prev) => {
        const next = { ...prev };
        delete next[roomId];
        return next;
      });
      if (activeRoomId === roomId) {
        setActiveRoomIdState(null);
      }
    },
    [activeRoomId, updateMessages, updateRooms]
  );

  const clearRoomMessages = useCallback(
    (roomId: string) => {
      updateMessages((prev) => ({
        ...prev,
        [roomId]: [],
      }));
    },
    [updateMessages]
  );

  return {
    rooms,
    messages,
    activeRoomId,
    activeRoom,
    activeRoomMessages,
    totalUnreadCount,
    setActiveRoomId,
    markRoomAsRead,
    incrementUnread,
    sendMessageOptimistic,
    markMessageDelivered,
    markMessageFailed,
    receiveMessage,
    addRoom,
    updateRoom,
    removeRoom,
    clearRoomMessages,
  };
}

export default useTelecomRooms;
