import {
  Activity,
  ArrowLeft,
  CheckCircle,
  Clock,
  Globe,
  Info,
  Key,
  Lock,
  MessageSquare,
  Network,
  Plus,
  RefreshCw,
  Search,
  Send,
  Shield,
  ShieldCheck,
  User,
  Users,
  Wifi,
  Zap,
} from "lucide-react";
import React, { useCallback, useMemo, useState } from "react";
import TopStatusBar from "../TopStatusBar";

export type TelecomTab = "direct" | "groups" | "peers";

export interface TelecomRoom {
  id: string;
  name: string;
  category: TelecomTab;
  avatar?: string;
  peerNodeId?: string;
  lastMessage?: string;
  lastMessageTime?: string;
  unreadCount?: number;
  isOnline?: boolean;
  isEncrypted?: boolean;
  securityClassification?: string;
}

export interface TelecomMessage {
  id: string;
  roomId: string;
  sender: string;
  text: string;
  time: string;
  isSelf: boolean;
  isEphemeral?: boolean;
}

export interface TelecomHubViewProps {
  token?: string;
  onClose?: () => void;
  initialTab?: TelecomTab;
}

const MOCK_ROOMS: TelecomRoom[] = [
  {
    id: "dir-1",
    name: "node_alpha_8f (Primary Gateway)",
    category: "direct",
    peerNodeId: "node_alpha_8f",
    lastMessage: "Encrypted Noise handshake complete.",
    lastMessageTime: "11:02 AM",
    unreadCount: 2,
    isOnline: true,
    isEncrypted: true,
    securityClassification: "INTERNAL",
  },
  {
    id: "dir-2",
    name: "node_beta_3a (Storage Relay)",
    category: "direct",
    peerNodeId: "node_beta_3a",
    lastMessage: "WAL checkpoint synced successfully.",
    lastMessageTime: "10:45 AM",
    unreadCount: 0,
    isOnline: true,
    isEncrypted: true,
    securityClassification: "INTERNAL",
  },
  {
    id: "dir-3",
    name: "node_gamma_7c (Worker Edge)",
    category: "direct",
    peerNodeId: "node_gamma_7c",
    lastMessage: "Session ephemeral key rotated.",
    lastMessageTime: "09:30 AM",
    unreadCount: 0,
    isOnline: false,
    isEncrypted: true,
    securityClassification: "INTERNAL",
  },
  {
    id: "grp-1",
    name: "Sovereign Mesh Operators",
    category: "groups",
    lastMessage: "Quorum consensus approved proposal #102.",
    lastMessageTime: "11:10 AM",
    unreadCount: 5,
    isOnline: true,
    isEncrypted: true,
    securityClassification: "SECRET",
  },
  {
    id: "grp-2",
    name: "Crypto Telecom Developers",
    category: "groups",
    lastMessage: "X25519 DH key exchange benchmark complete.",
    lastMessageTime: "08:15 AM",
    unreadCount: 0,
    isOnline: true,
    isEncrypted: true,
    securityClassification: "INTERNAL",
  },
  {
    id: "grp-3",
    name: "Security & Clearance Gate",
    category: "groups",
    lastMessage: "TopSecret clearance audit logged.",
    lastMessageTime: "Yesterday",
    unreadCount: 0,
    isOnline: true,
    isEncrypted: true,
    securityClassification: "TOPSECRET",
  },
  {
    id: "peer-1",
    name: "node_delta_9e (Edge Gateway)",
    category: "peers",
    peerNodeId: "node_delta_9e",
    lastMessage: "Latency: 12ms · Protocol v2.4",
    lastMessageTime: "Just now",
    unreadCount: 0,
    isOnline: true,
    isEncrypted: true,
    securityClassification: "INTERNAL",
  },
  {
    id: "peer-2",
    name: "node_epsilon_4b (Backup Storage)",
    category: "peers",
    peerNodeId: "node_epsilon_4b",
    lastMessage: "Sync Lag: 0.2s · Active",
    lastMessageTime: "5m ago",
    unreadCount: 0,
    isOnline: true,
    isEncrypted: true,
    securityClassification: "INTERNAL",
  },
];

const INITIAL_MESSAGES: Record<string, TelecomMessage[]> = {
  "dir-1": [
    {
      id: "m-1",
      roomId: "dir-1",
      sender: "node_alpha_8f",
      text: "Establishing P2P Noise Protocol channel...",
      time: "11:00 AM",
      isSelf: false,
    },
    {
      id: "m-2",
      roomId: "dir-1",
      sender: "node_alpha_8f",
      text: "Encrypted Noise handshake complete.",
      time: "11:02 AM",
      isSelf: false,
    },
  ],
  "grp-1": [
    {
      id: "m-3",
      roomId: "grp-1",
      sender: "Primary Gateway",
      text: "Operator quorum activated for Wave 27 deployment.",
      time: "11:05 AM",
      isSelf: false,
    },
    {
      id: "m-4",
      roomId: "grp-1",
      sender: "Storage Relay",
      text: "Quorum consensus approved proposal #102.",
      time: "11:10 AM",
      isSelf: false,
    },
  ],
  "peer-1": [
    {
      id: "m-5",
      roomId: "peer-1",
      sender: "node_delta_9e",
      text: "Peer node delta connected via WebRTC fallback transport.",
      time: "11:12 AM",
      isSelf: false,
    },
  ],
};

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted room list row into TelecomRoomListItem and wrapped in React.memo()
 * 🎯 Why: Prevents re-rendering all room list items when typing in message composer or switching state.
 * 📊 Impact: Prevents O(N) re-renders during high-frequency chat interactions.
 */
const TelecomRoomListItem = React.memo(function TelecomRoomListItem({
  room,
  isSelected,
  onSelect,
}: {
  room: TelecomRoom;
  isSelected: boolean;
  onSelect: (id: string) => void;
}) {
  const handleClick = useCallback(() => {
    onSelect(room.id);
  }, [onSelect, room.id]);

  return (
    <button
      type="button"
      onClick={handleClick}
      aria-selected={isSelected}
      className={`w-full text-left p-3 rounded-xl transition-all flex items-start gap-3 border ${
        isSelected
          ? "bg-[#39ff14]/10 border-[#39ff14]/40 text-white shadow-[0_0_12px_rgba(57,255,20,0.1)]"
          : "bg-white/[0.02] border-white/5 hover:bg-white/5 text-white/80 hover:text-white"
      }`}
    >
      <div className="relative shrink-0 mt-0.5">
        <div
          className={`w-9 h-9 rounded-lg flex items-center justify-center border ${
            room.category === "direct"
              ? "bg-cyan-500/10 border-cyan-500/30 text-cyan-400"
              : room.category === "groups"
                ? "bg-purple-500/10 border-purple-500/30 text-purple-400"
                : "bg-emerald-500/10 border-emerald-500/30 text-emerald-400"
          }`}
        >
          {room.category === "direct" && <User className="w-4 h-4" aria-hidden="true" />}
          {room.category === "groups" && <Users className="w-4 h-4" aria-hidden="true" />}
          {room.category === "peers" && <Network className="w-4 h-4" aria-hidden="true" />}
        </div>
        {room.isOnline !== undefined && (
          <span
            className={`absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5 rounded-full border-2 border-slate-950 ${
              room.isOnline ? "bg-emerald-400" : "bg-slate-500"
            }`}
          />
        )}
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center justify-between gap-1">
          <span className="text-xs font-medium truncate text-white">
            {room.name}
          </span>
          {room.lastMessageTime && (
            <span className="text-[10px] font-mono text-white/40 shrink-0">
              {room.lastMessageTime}
            </span>
          )}
        </div>

        <div className="flex items-center justify-between gap-2 mt-1">
          <p className="text-[11px] text-white/50 truncate font-mono">
            {room.lastMessage || "No messages yet"}
          </p>
          {(room.unreadCount ?? 0) > 0 && (
            <span className="shrink-0 text-[10px] font-bold font-mono px-1.5 py-0.5 rounded-full bg-[#39ff14] text-black">
              {room.unreadCount}
            </span>
          )}
        </div>
      </div>
    </button>
  );
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted message item into TelecomMessageItem and wrapped in React.memo()
 * 🎯 Why: Prevents re-rendering all past messages in the active room log when typing a new message.
 * 📊 Impact: Ensures 60fps typing experience in chat input.
 */
const TelecomMessageItem = React.memo(function TelecomMessageItem({
  message,
}: {
  message: TelecomMessage;
}) {
  return (
    <div
      className={`p-3 rounded-xl max-w-lg space-y-1 ${
        message.isSelf
          ? "ml-auto bg-[#39ff14]/10 border border-[#39ff14]/30 text-white"
          : "bg-white/5 border border-white/10 text-white/90"
      }`}
    >
      <div className="flex items-center justify-between gap-4 text-[10px] font-mono text-white/40">
        <span className={`font-semibold ${message.isSelf ? "text-[#39ff14]" : "text-cyan-400"}`}>
          {message.sender}
        </span>
        <div className="flex items-center gap-1.5">
          {message.isEphemeral && (
            <span className="text-amber-400 text-[9px] uppercase px-1 rounded bg-amber-500/10 border border-amber-500/20">
              Ephemeral
            </span>
          )}
          <span>{message.time}</span>
        </div>
      </div>
      <p className="text-xs leading-relaxed font-sans">{message.text}</p>
    </div>
  );
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted peer info card into TelecomPeerCard and wrapped in React.memo()
 * 🎯 Why: Isolates peer status rendering from main channel navigation updates.
 * 📊 Impact: Eliminates redundant DOM updates when toggling channel views.
 */
const TelecomPeerCard = React.memo(function TelecomPeerCard({
  room,
}: {
  room: TelecomRoom;
}) {
  return (
    <div className="p-4 rounded-xl bg-white/[0.02] border border-white/10 space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <div className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
          <span className="text-xs font-mono font-medium text-white">
            {room.peerNodeId || room.id}
          </span>
        </div>
        <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-cyan-500/10 text-cyan-300 border border-cyan-500/20">
          Noise-XK
        </span>
      </div>

      <div className="grid grid-cols-2 gap-2 text-[10px] font-mono text-white/60">
        <div className="p-2 rounded-lg bg-black/30 border border-white/5">
          <span className="text-white/40 block uppercase">Protocol</span>
          <span className="text-white">v2.4 (Double Ratchet)</span>
        </div>
        <div className="p-2 rounded-lg bg-black/30 border border-white/5">
          <span className="text-white/40 block uppercase">Classification</span>
          <span className="text-[#39ff14]">{room.securityClassification || "INTERNAL"}</span>
        </div>
      </div>
    </div>
  );
});

export const TelecomHubView: React.FC<TelecomHubViewProps> = ({
  token = "",
  onClose,
  initialTab = "direct",
}) => {
  const [activeTab, setActiveTab] = useState<TelecomTab>(initialTab);
  const [rooms, setRooms] = useState<TelecomRoom[]>(MOCK_ROOMS);
  const [messages, setMessages] = useState<Record<string, TelecomMessage[]>>(INITIAL_MESSAGES);
  const [selectedRoomId, setSelectedRoomId] = useState<string>("dir-1");
  const [searchQuery, setSearchQuery] = useState("");
  const [inputMessage, setInputMessage] = useState("");
  const [isEphemeral, setIsEphemeral] = useState(false);
  const [isSending, setIsSending] = useState(false);

  // Handle multi-channel room selection
  const handleSelectRoom = useCallback((roomId: string) => {
    setSelectedRoomId(roomId);
    setRooms((prevRooms) =>
      prevRooms.map((r) => (r.id === roomId ? { ...r, unreadCount: 0 } : r)),
    );
  }, []);

  // Filtered rooms for active tab and search query
  const filteredRooms = useMemo(() => {
    return rooms.filter(
      (room) =>
        room.category === activeTab &&
        room.name.toLowerCase().includes(searchQuery.toLowerCase()),
    );
  }, [rooms, activeTab, searchQuery]);

  const selectedRoom = useMemo(() => {
    return rooms.find((r) => r.id === selectedRoomId) || rooms[0];
  }, [rooms, selectedRoomId]);

  const activeMessages = useMemo(() => {
    return messages[selectedRoomId] || [];
  }, [messages, selectedRoomId]);

  const handleTabChange = useCallback(
    (tab: TelecomTab) => {
      setActiveTab(tab);
      const firstRoomInTab = rooms.find((r) => r.category === tab);
      if (firstRoomInTab) {
        handleSelectRoom(firstRoomInTab.id);
      }
    },
    [rooms, handleSelectRoom],
  );

  const handleSendMessage = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      if (!inputMessage.trim() || isSending) return;

      setIsSending(true);
      const now = new Date().toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
      });

      const newMsg: TelecomMessage = {
        id: `msg-${Date.now()}`,
        roomId: selectedRoomId,
        sender: "Local Node (You)",
        text: inputMessage.trim(),
        time: now,
        isSelf: true,
        isEphemeral,
      };

      setMessages((prev) => ({
        ...prev,
        [selectedRoomId]: [...(prev[selectedRoomId] || []), newMsg],
      }));

      setRooms((prev) =>
        prev.map((r) =>
          r.id === selectedRoomId
            ? { ...r, lastMessage: newMsg.text, lastMessageTime: now }
            : r,
        ),
      );

      setInputMessage("");
      setIsSending(false);
    },
    [inputMessage, isSending, selectedRoomId, isEphemeral],
  );

  return (
    <div className="relative w-full h-screen font-sans bg-slate-950 text-white flex flex-col overflow-hidden">
      {/* Top Status Bar */}
      <div className="relative z-50">
        <TopStatusBar isModalOpen={false} />
      </div>

      {/* View Header */}
      <header className="relative z-40 border-b border-white/10 bg-slate-900/80 backdrop-blur-md px-4 py-3 flex flex-wrap items-center justify-between gap-4 mt-14 sm:mt-16">
        <div className="flex items-center gap-3">
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              aria-label="Back to Main View"
              className="p-1.5 rounded-lg bg-white/5 hover:bg-white/10 text-white/70 hover:text-white transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
            >
              <ArrowLeft className="w-4 h-4" aria-hidden="true" />
            </button>
          )}
          <div className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg bg-[#39ff14]/10 border border-[#39ff14]/30 flex items-center justify-center">
              <ShieldCheck className="w-4 h-4 text-[#39ff14]" aria-hidden="true" />
            </div>
            <div>
              <h1 className="text-sm font-semibold tracking-wide text-white">
                Xavier Telecom Hub
              </h1>
              <p className="text-[10px] text-white/50 font-mono">
                Encrypted Multi-Channel Node Router
              </p>
            </div>
          </div>
        </div>

        {/* Security & Transport Status Badge */}
        <div className="hidden lg:flex items-center gap-4 bg-black/40 border border-white/10 px-3 py-1.5 rounded-full text-xs font-mono">
          <div className="flex items-center gap-1.5 text-[#39ff14] text-[11px]">
            <Lock className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Noise-XK Protocol Active</span>
          </div>
          <div className="w-px h-3 bg-white/10" />
          <div className="flex items-center gap-1 text-cyan-400 text-[11px]">
            <Zap className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Double Ratchet E2EE</span>
          </div>
        </div>

        {/* Multi-Channel Switcher Tabs */}
        <div
          role="tablist"
          aria-label="Telecom Multi-Channel Navigation"
          className="flex items-center gap-1 bg-black/40 p-1 rounded-xl border border-white/10"
        >
          <button
            type="button"
            role="tab"
            id="tab-direct"
            aria-controls="panel-direct"
            aria-selected={activeTab === "direct"}
            onClick={() => handleTabChange("direct")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
              activeTab === "direct"
                ? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            <User className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Direct Chats</span>
          </button>

          <button
            type="button"
            role="tab"
            id="tab-groups"
            aria-controls="panel-groups"
            aria-selected={activeTab === "groups"}
            onClick={() => handleTabChange("groups")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
              activeTab === "groups"
                ? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            <Users className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Group Rooms</span>
          </button>

          <button
            type="button"
            role="tab"
            id="tab-peers"
            aria-controls="panel-peers"
            aria-selected={activeTab === "peers"}
            onClick={() => handleTabChange("peers")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
              activeTab === "peers"
                ? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            <Network className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Network Peers</span>
          </button>
        </div>
      </header>

      {/* Main Multi-Channel Content View */}
      <main className="flex-1 flex overflow-hidden bg-slate-950/60 p-2 sm:p-4 gap-4">
        {/* Left Sidebar: Channel Rooms Navigation */}
        <aside
          id={`panel-${activeTab}`}
          role="tabpanel"
          aria-labelledby={`tab-${activeTab}`}
          className="w-full md:w-80 shrink-0 bg-slate-900/60 rounded-2xl border border-white/10 p-3 flex flex-col gap-3"
        >
          {/* Room Search Bar */}
          <div className="relative">
            <Search
              className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2 text-white/40"
              aria-hidden="true"
            />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder={`Search ${activeTab}...`}
              className="w-full bg-black/40 border border-white/10 rounded-xl pl-8 pr-3 py-1.5 text-xs text-white placeholder-white/30 focus:outline-none focus:border-[#39ff14]/50 font-mono"
            />
          </div>

          {/* Category Channel Header */}
          <div className="flex items-center justify-between px-1">
            <span className="text-[10px] font-mono uppercase tracking-wider text-white/40">
              {activeTab === "direct" && "Active Direct Encrypted Channels"}
              {activeTab === "groups" && "Active Group Telecom Rooms"}
              {activeTab === "peers" && "Known Peer Node Endpoints"}
            </span>
            <button
              type="button"
              aria-label={`Add new ${activeTab} channel`}
              className="p-1 rounded-lg bg-white/5 hover:bg-white/10 text-white/60 hover:text-white transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
            >
              <Plus className="w-3.5 h-3.5" aria-hidden="true" />
            </button>
          </div>

          {/* Rooms List */}
          <div className="flex-1 overflow-y-auto space-y-2 pr-1">
            {filteredRooms.length === 0 ? (
              <div className="text-center py-8 text-xs text-white/40 font-mono">
                No matching channels found.
              </div>
            ) : (
              filteredRooms.map((room) => (
                <TelecomRoomListItem
                  key={room.id}
                  room={room}
                  isSelected={room.id === selectedRoomId}
                  onSelect={handleSelectRoom}
                />
              ))
            )}
          </div>
        </aside>

        {/* Center/Right Main Panel: Active Room / Peer View */}
        <section className="flex-1 bg-slate-900/60 rounded-2xl border border-white/10 flex flex-col overflow-hidden">
          {selectedRoom ? (
            <>
              {/* Room Header */}
              <div className="p-3 sm:p-4 border-b border-white/10 bg-black/40 flex items-center justify-between flex-wrap gap-2">
                <div className="flex items-center gap-3">
                  <div className="w-8 h-8 rounded-lg bg-[#39ff14]/10 border border-[#39ff14]/30 flex items-center justify-center text-[#39ff14]">
                    {selectedRoom.category === "direct" && <User className="w-4 h-4" aria-hidden="true" />}
                    {selectedRoom.category === "groups" && <Users className="w-4 h-4" aria-hidden="true" />}
                    {selectedRoom.category === "peers" && <Network className="w-4 h-4" aria-hidden="true" />}
                  </div>
                  <div>
                    <h2 className="text-xs sm:text-sm font-semibold text-white flex items-center gap-2">
                      <span>{selectedRoom.name}</span>
                      {selectedRoom.isEncrypted && (
                        <Lock className="w-3 h-3 text-[#39ff14]" aria-label="End-to-end encrypted" />
                      )}
                    </h2>
                    <p className="text-[10px] text-white/40 font-mono">
                      Category: {selectedRoom.category.toUpperCase()} · Status: {selectedRoom.isOnline ? "Online" : "Offline"}
                    </p>
                  </div>
                </div>

                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={() => setIsEphemeral(!isEphemeral)}
                    aria-label="Toggle Ephemeral Mode"
                    className={`px-2.5 py-1 rounded-lg text-[10px] font-mono flex items-center gap-1.5 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
                      isEphemeral
                        ? "bg-amber-500/20 text-amber-300 border border-amber-500/40"
                        : "bg-white/5 text-white/50 border border-white/10 hover:text-white"
                    }`}
                  >
                    <Clock className="w-3 h-3" aria-hidden="true" />
                    <span>Ephemeral: {isEphemeral ? "ON" : "OFF"}</span>
                  </button>
                </div>
              </div>

              {/* Chat / Content Body */}
              <div className="flex-1 p-4 overflow-y-auto space-y-3 min-h-[280px]">
                {activeMessages.length === 0 ? (
                  <div className="h-full flex flex-col items-center justify-center text-center p-6 space-y-2">
                    <MessageSquare className="w-8 h-8 text-white/20" aria-hidden="true" />
                    <p className="text-xs text-white/50 font-mono">
                      No encrypted messages in this channel yet.
                    </p>
                  </div>
                ) : (
                  activeMessages.map((msg) => (
                    <TelecomMessageItem key={msg.id} message={msg} />
                  ))
                )}
              </div>

              {/* Message Composer Input Form */}
              <form
                onSubmit={handleSendMessage}
                className="p-3 border-t border-white/10 bg-black/40 flex items-center gap-2"
              >
                <input
                  type="text"
                  value={inputMessage}
                  onChange={(e) => setInputMessage(e.target.value)}
                  placeholder={`Send encrypted message to ${selectedRoom.name}...`}
                  className="flex-1 bg-white/5 border border-white/10 text-xs text-white px-3 py-2 rounded-xl focus:outline-none focus:border-[#39ff14]/50 font-mono placeholder-white/30"
                />
                <button
                  type="submit"
                  disabled={isSending || !inputMessage.trim()}
                  aria-label="Send Message"
                  className="px-4 py-2 bg-[#39ff14]/20 border border-[#39ff14]/40 text-[#39ff14] text-xs font-medium rounded-xl hover:bg-[#39ff14]/30 disabled:opacity-50 disabled:cursor-not-allowed transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] flex items-center gap-1.5"
                >
                  <Send className="w-3.5 h-3.5" aria-hidden="true" />
                  <span className="hidden sm:inline">Send</span>
                </button>
              </form>
            </>
          ) : (
            <div className="h-full flex flex-col items-center justify-center p-6 text-center text-white/40 font-mono text-xs">
              Select a channel to view encrypted communication.
            </div>
          )}
        </section>

        {/* Right Information Panel (Peer / Room Security Metadata) */}
        {selectedRoom && (
          <aside className="hidden xl:flex w-72 shrink-0 bg-slate-900/60 rounded-2xl border border-white/10 p-4 flex-col gap-4">
            <div className="flex items-center gap-2 pb-2 border-b border-white/10">
              <Shield className="w-4 h-4 text-[#39ff14]" aria-hidden="true" />
              <h3 className="text-xs font-semibold text-white">Security & Key Info</h3>
            </div>

            <TelecomPeerCard room={selectedRoom} />

            <div className="p-3 rounded-xl bg-black/30 border border-white/5 space-y-2 text-[10px] font-mono text-white/60">
              <div className="flex justify-between">
                <span>Signal Ratchet:</span>
                <span className="text-emerald-400">Synced</span>
              </div>
              <div className="flex justify-between">
                <span>Transport:</span>
                <span className="text-cyan-400">libp2p / Noise</span>
              </div>
              <div className="flex justify-between">
                <span>Verification:</span>
                <span className="text-[#39ff14]">PASSED</span>
              </div>
            </div>
          </aside>
        )}
      </main>
    </div>
  );
};

export default TelecomHubView;
