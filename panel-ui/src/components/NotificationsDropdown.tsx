import {
  Activity,
  AlertTriangle,
  Bot,
  Brain,
  CheckCheck,
  Clock,
  RefreshCw,
  X,
  Zap,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";

export interface Notification {
  id: string;
  islandId: IslandId;
  title: string;
  body: string;
  timestamp: Date;
  read: boolean;
  severity?: "info" | "warning" | "error" | "success";
}

type IslandId = "system" | "memory" | "agents" | "errors";

interface Island {
  id: IslandId;
  label: string;
  icon: React.ReactNode;
  color: string;
  bgColor: string;
  borderColor: string;
}

const ISLANDS: Island[] = [
  {
    id: "system",
    label: "System",
    icon: <Activity className="w-3 h-3" />,
    color: "text-cyan-400",
    bgColor: "bg-cyan-500/10",
    borderColor: "border-cyan-500/20",
  },
  {
    id: "memory",
    label: "Memory",
    icon: <Brain className="w-3 h-3" />,
    color: "text-[#39ff14]",
    bgColor: "bg-[#39ff14]/5",
    borderColor: "border-[#39ff14]/15",
  },
  {
    id: "agents",
    label: "Agents",
    icon: <Bot className="w-3 h-3" />,
    color: "text-purple-400",
    bgColor: "bg-purple-500/10",
    borderColor: "border-purple-500/20",
  },
  {
    id: "errors",
    label: "Errors",
    icon: <AlertTriangle className="w-3 h-3" />,
    color: "text-red-400",
    bgColor: "bg-red-500/10",
    borderColor: "border-red-500/20",
  },
];

// ── Mock notifications ─────────────────────────────────────────────────────
const MOCK_NOTIFICATIONS: Notification[] = [
  {
    id: "n1",
    islandId: "memory",
    title: "Memory Indexed",
    read: false,
    timestamp: new Date(Date.now() - 2 * 60000),
    body: "247 new memories indexed from project scan. BM25 index updated.",
    severity: "success",
  },
  {
    id: "n2",
    islandId: "memory",
    title: "Memory Indexed",
    read: false,
    timestamp: new Date(Date.now() - 8 * 60000),
    body: "14 episodic memories summarized and compacted.",
    severity: "info",
  },
  {
    id: "n3",
    islandId: "agents",
    title: "Agent Task Complete",
    read: false,
    timestamp: new Date(Date.now() - 15 * 60000),
    body: "Antigravity agent completed code analysis task in 4.2s.",
    severity: "success",
  },
  {
    id: "n4",
    islandId: "system",
    title: "Xavier Started",
    read: true,
    timestamp: new Date(Date.now() - 42 * 60000),
    body: "Xavier backend v0.6.1-beta started on port 8006. SQLite-Vec loaded.",
    severity: "info",
  },
  {
    id: "n5",
    islandId: "system",
    title: "Node Sync",
    read: true,
    timestamp: new Date(Date.now() - 65 * 60000),
    body: "Synchronized with 4 peer nodes. Consensus: 98%.",
    severity: "success",
  },
  {
    id: "n6",
    islandId: "errors",
    title: "Telegram Disconnected",
    read: false,
    timestamp: new Date(Date.now() - 5 * 60000),
    body: "Telegram bot token not configured. Messaging channel offline.",
    severity: "error",
  },
  {
    id: "n7",
    islandId: "errors",
    title: "OpenAI Quota Warning",
    read: true,
    timestamp: new Date(Date.now() - 120 * 60000),
    body: "OpenAI API usage at 89% of monthly quota. Consider switching to Gemini.",
    severity: "warning",
  },
];

function formatRelativeTime(date: Date): string {
  const diff = (Date.now() - date.getTime()) / 1000;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function NotificationItem({
  notif,
  onRead,
}: {
  notif: Notification;
  onRead: (id: string) => void;
}) {
  const severityDot = {
    success: "bg-green-400",
    info: "bg-cyan-400",
    warning: "bg-amber-400",
    error: "bg-red-400",
  }[notif.severity || "info"];

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: -4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, x: 20, height: 0 }}
      className={`flex gap-2.5 p-2.5 rounded-lg transition-colors cursor-default group ${
        notif.read ? "bg-transparent" : "bg-white/[0.025]"
      } hover:bg-white/[0.04]`}
    >
      <div className="flex-shrink-0 mt-0.5">
        <div
          className={`w-1.5 h-1.5 rounded-full ${severityDot} ${notif.read ? "opacity-30" : ""}`}
        />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center justify-between gap-2">
          <p
            className={`text-[11px] font-medium truncate ${notif.read ? "text-white/40" : "text-white/80"}`}
          >
            {notif.title}
          </p>
          <span className="flex-shrink-0 text-[9px] text-white/20 flex items-center gap-1">
            <Clock className="w-2.5 h-2.5" />
            {formatRelativeTime(notif.timestamp)}
          </span>
        </div>
        <p
          className={`text-[10px] mt-0.5 leading-relaxed ${notif.read ? "text-white/25" : "text-white/45"}`}
        >
          {notif.body}
        </p>
      </div>
      {!notif.read && (
        <button
          onClick={() => onRead(notif.id)}
          className="flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity p-0.5 text-white/20 hover:text-white/50"
        >
          <X className="w-3 h-3" />
        </button>
      )}
    </motion.div>
  );
}

interface NotificationsDropdownProps {
  onClose: () => void;
  anchorRef?: React.RefObject<HTMLElement>;
}

export default function NotificationsDropdown({
  onClose,
}: NotificationsDropdownProps) {
  const [notifications, setNotifications] =
    useState<Notification[]>(MOCK_NOTIFICATIONS);
  const [activeIsland, setActiveIsland] = useState<IslandId | "all">("all");

  const unreadCount = notifications.filter((n) => !n.read).length;

  const markRead = (id: string) => {
    setNotifications((prev) =>
      prev.map((n) => (n.id === id ? { ...n, read: true } : n)),
    );
  };

  const markAllRead = () => {
    setNotifications((prev) => prev.map((n) => ({ ...n, read: true })));
  };

  const filtered =
    activeIsland === "all"
      ? notifications
      : notifications.filter((n) => n.islandId === activeIsland);

  const islandCounts = ISLANDS.reduce<Record<IslandId, number>>(
    (acc, island) => {
      acc[island.id] = notifications.filter(
        (n) => n.islandId === island.id && !n.read,
      ).length;
      return acc;
    },
    {} as Record<IslandId, number>,
  );

  return (
    <>
      {/* Backdrop */}
      <div className="fixed inset-0 z-[65]" onClick={onClose} />

      <motion.div
        initial={{ opacity: 0, y: -8, scale: 0.97 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: -8, scale: 0.97 }}
        transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
        className="absolute right-0 top-full mt-2 w-80 max-h-[420px] bg-[#060606] border border-white/[0.07] rounded-xl shadow-2xl flex flex-col overflow-hidden z-[66]"
        style={{ top: "100%" }}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-white/[0.05]">
          <div className="flex items-center gap-2">
            <span className="text-xs font-semibold text-white/80 tracking-wide">
              Notifications
            </span>
            {unreadCount > 0 && (
              <span className="px-1.5 py-0.5 bg-[#39ff14]/15 border border-[#39ff14]/20 text-[#39ff14] text-[9px] font-bold rounded-full">
                {unreadCount}
              </span>
            )}
          </div>
          <div className="flex items-center gap-1">
            {unreadCount > 0 && (
              <button
                onClick={markAllRead}
                className="flex items-center gap-1 px-2 py-1 text-[9px] text-white/30 hover:text-white/60 hover:bg-white/5 rounded-lg transition-all"
              >
                <CheckCheck className="w-3 h-3" />
                All read
              </button>
            )}
            <button
              onClick={onClose}
              className="p-1 text-white/20 hover:text-white/50 hover:bg-white/5 rounded-lg transition-all"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        {/* Island filter tabs */}
        <div className="flex gap-1 px-3 py-2 border-b border-white/[0.04] overflow-x-auto">
          <button
            onClick={() => setActiveIsland("all")}
            className={`flex-shrink-0 flex items-center gap-1 px-2 py-1 rounded-md text-[9px] uppercase tracking-widest transition-all ${
              activeIsland === "all"
                ? "bg-white/10 text-white/80"
                : "text-white/30 hover:text-white/60 hover:bg-white/5"
            }`}
          >
            <RefreshCw className="w-2.5 h-2.5" />
            All
          </button>
          {ISLANDS.map((island) => (
            <button
              key={island.id}
              onClick={() => setActiveIsland(island.id)}
              className={`flex-shrink-0 flex items-center gap-1 px-2 py-1 rounded-md text-[9px] uppercase tracking-widest transition-all relative ${
                activeIsland === island.id
                  ? `${island.bgColor} ${island.color} border ${island.borderColor}`
                  : "text-white/30 hover:text-white/60 hover:bg-white/5"
              }`}
            >
              {island.icon}
              {island.label}
              {islandCounts[island.id] > 0 && (
                <span
                  className={`ml-0.5 px-1 py-px rounded-full text-[8px] font-bold ${island.bgColor} ${island.color}`}
                >
                  {islandCounts[island.id]}
                </span>
              )}
            </button>
          ))}
        </div>

        {/* Notifications list */}
        <div className="flex-1 overflow-y-auto p-2">
          <AnimatePresence>
            {filtered.length === 0 ? (
              <motion.div
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                className="flex flex-col items-center justify-center py-8 text-white/20"
              >
                <Zap className="w-6 h-6 mb-2 opacity-30" />
                <p className="text-[11px]">No notifications</p>
              </motion.div>
            ) : (
              filtered.map((n) => (
                <NotificationItem key={n.id} notif={n} onRead={markRead} />
              ))
            )}
          </AnimatePresence>
        </div>

        {/* Footer */}
        <div className="px-4 py-2 border-t border-white/[0.04] bg-black/20">
          <p className="text-[9px] text-white/15 text-center">
            Mock data — persistence backend pending
          </p>
        </div>
      </motion.div>
    </>
  );
}
