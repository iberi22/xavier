import React, { useState, useRef, useEffect } from "react";
import {
  Lock,
  Paperclip,
  Send,
  Hash,
  User,
  Shield,
  Search,
  CheckCheck,
  FileText,
  Menu,
  X,
  Sparkles,
  Wifi,
} from "lucide-react";

/**
 * Interface representing a P2P chat channel (Direct Peer Chat or Network Room).
 */
export interface ChatChannel {
  id: string;
  name: string;
  type: "direct" | "room";
  alias?: string;
  nodeId?: string;
  online?: boolean;
  unreadCount?: number;
  description?: string;
}

/**
 * Interface representing a P2P encrypted chat message.
 */
export interface ChatMessage {
  id: string;
  channelId: string;
  senderAlias: string;
  senderNodeId: string;
  content: string;
  timestamp: string;
  encrypted: boolean;
  isSelf?: boolean;
  attachment?: {
    name: string;
    size?: string;
    type?: string;
  };
}

/**
 * Props for the MeshChatView component.
 */
export interface MeshChatViewProps {
  initialChannelId?: string;
  channels?: ChatChannel[];
  messages?: ChatMessage[];
  onSendMessage?: (channelId: string, content: string, attachment?: File | null) => void;
  className?: string;
}

// Default initial network rooms and direct peer chats
const DEFAULT_CHANNELS: ChatChannel[] = [
  {
    id: "room-general",
    name: "general",
    type: "room",
    unreadCount: 2,
    description: "Mesh-wide broadcast channel for general discussion",
  },
  {
    id: "room-dev-council",
    name: "dev-council",
    type: "room",
    unreadCount: 0,
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
  },
  {
    id: "peer-beta",
    name: "Peer Beta",
    alias: "Peer Beta",
    nodeId: "node-c3e811",
    type: "direct",
    online: false,
    unreadCount: 0,
  },
];

// Default initial messages for default channels
const DEFAULT_MESSAGES: ChatMessage[] = [
  {
    id: "msg-1",
    channelId: "room-general",
    senderAlias: "Node Alpha",
    senderNodeId: "node-a7f92b",
    content: "Encrypted P2P connection established across the mesh.",
    timestamp: "10:42 AM",
    encrypted: true,
    isSelf: false,
  },
  {
    id: "msg-2",
    channelId: "room-general",
    senderAlias: "Local Node",
    senderNodeId: "node-local",
    content: "Ack. Data node sync is active and healthy.",
    timestamp: "10:43 AM",
    encrypted: true,
    isSelf: true,
  },
  {
    id: "msg-3",
    channelId: "room-dev-council",
    senderAlias: "Peer Beta",
    senderNodeId: "node-c3e811",
    content: "Proposed code graph index update ready for peer verification.",
    timestamp: "09:15 AM",
    encrypted: true,
    isSelf: false,
  },
  {
    id: "msg-4",
    channelId: "peer-alpha",
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
];

/**
 * MeshChatView component provides an encrypted P2P chat view with channel switcher
 * (Direct Peer Chats & Network Rooms), message stream, and attachment indicator.
 */
export const MeshChatView: React.FC<MeshChatViewProps> = ({
  initialChannelId = "room-general",
  channels = DEFAULT_CHANNELS,
  messages = DEFAULT_MESSAGES,
  onSendMessage,
  className = "",
}) => {
  const [activeChannelId, setActiveChannelId] = useState<string>(initialChannelId);
  const [channelList, setChannelList] = useState<ChatChannel[]>(channels);
  const [messageList, setMessageList] = useState<ChatMessage[]>(messages);
  const [inputText, setInputText] = useState<string>("");
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [searchQuery, setSearchQuery] = useState<string>("");
  const [isSidebarOpenMobile, setIsSidebarOpenMobile] = useState<boolean>(false);

  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const messagesEndRef = useRef<HTMLDivElement | null>(null);

  const activeChannel = channelList.find((c) => c.id === activeChannelId) || channelList[0];

  // Sync state if props change
  useEffect(() => {
    setChannelList(channels);
  }, [channels]);

  useEffect(() => {
    setMessageList(messages);
  }, [messages]);

  // Auto-scroll message stream to bottom when active messages change
  useEffect(() => {
    if (messagesEndRef.current && typeof messagesEndRef.current.scrollIntoView === "function") {
      messagesEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [messageList, activeChannelId]);

  // Clear unread count when switching to channel
  const handleSelectChannel = (channelId: string) => {
    setActiveChannelId(channelId);
    setIsSidebarOpenMobile(false);
    setChannelList((prev) =>
      prev.map((c) => (c.id === channelId ? { ...c, unreadCount: 0 } : c))
    );
  };

  const handleSend = () => {
    const trimmed = inputText.trim();
    if (!trimmed && !selectedFile) return;

    const newMsg: ChatMessage = {
      id: `msg-${Date.now()}`,
      channelId: activeChannelId,
      senderAlias: "Local Node",
      senderNodeId: "node-local",
      content: trimmed,
      timestamp: new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
      encrypted: true,
      isSelf: true,
      attachment: selectedFile
        ? {
            name: selectedFile.name,
            size: `${(selectedFile.size / 1024).toFixed(1)} KB`,
            type: selectedFile.type,
          }
        : undefined,
    };

    setMessageList((prev) => [...prev, newMsg]);

    if (onSendMessage) {
      onSendMessage(activeChannelId, trimmed, selectedFile);
    }

    setInputText("");
    setSelectedFile(null);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (e.target.files && e.target.files[0]) {
      setSelectedFile(e.target.files[0]);
    }
  };

  // Filter channels based on search query
  const filteredChannels = channelList.filter(
    (c) =>
      c.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
      (c.alias && c.alias.toLowerCase().includes(searchQuery.toLowerCase())) ||
      (c.nodeId && c.nodeId.toLowerCase().includes(searchQuery.toLowerCase()))
  );

  const networkRooms = filteredChannels.filter((c) => c.type === "room");
  const directPeers = filteredChannels.filter((c) => c.type === "direct");
  const activeMessages = messageList.filter((m) => m.channelId === activeChannelId);

  return (
    <div
      className={`flex h-full w-full bg-[#09090b] border border-zinc-800 text-zinc-100 rounded-xl overflow-hidden shadow-xl ${className}`}
    >
      {/* Sidebar Overlay for Mobile Layout */}
      {isSidebarOpenMobile && (
        <div
          className="fixed inset-0 bg-black/60 z-30 md:hidden"
          onClick={() => setIsSidebarOpenMobile(false)}
        />
      )}

      {/* Sidebar Navigation (Channels & Direct Peers) */}
      <aside
        className={`fixed md:relative inset-y-0 left-0 z-40 w-72 bg-[#0c0c0e] border-r border-zinc-800/80 flex flex-col transition-transform duration-200 ease-in-out ${
          isSidebarOpenMobile ? "translate-x-0" : "-translate-x-full md:translate-x-0"
        }`}
      >
        {/* Sidebar Header */}
        <div className="p-4 border-b border-zinc-800/60 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <div className="w-8 h-8 rounded-lg bg-emerald-500/10 border border-emerald-500/30 flex items-center justify-center text-emerald-400">
              <Shield className="w-4 h-4" />
            </div>
            <div>
              <h2 className="text-xs font-semibold uppercase tracking-wider text-zinc-200">
                P2P Mesh Chat
              </h2>
              <div className="flex items-center gap-1 text-[10px] text-emerald-400/90 font-mono">
                <Wifi className="w-3 h-3 animate-pulse" />
                <span>Encrypted Channel</span>
              </div>
            </div>
          </div>
          <button
            type="button"
            onClick={() => setIsSidebarOpenMobile(false)}
            aria-label="Close channels menu"
            className="md:hidden text-zinc-400 hover:text-zinc-200 p-1 rounded-md"
          >
            <X className="w-5 h-5" aria-hidden="true" />
          </button>
        </div>

        {/* Search Bar */}
        <div className="p-3">
          <div className="relative">
            <Search className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2 text-zinc-500" />
            <input
              type="text"
              placeholder="Search channels & peers..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-full bg-zinc-900/80 border border-zinc-800/80 text-xs rounded-lg pl-8 pr-3 py-1.5 text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-zinc-700 font-mono"
            />
          </div>
        </div>

        {/* Channel & Direct Peer List */}
        <div className="flex-1 overflow-y-auto px-2 space-y-4 py-2">
          {/* Section: Network Rooms */}
          <div>
            <div className="px-3 mb-1.5 flex items-center justify-between">
              <span className="text-[10px] font-semibold uppercase tracking-widest text-zinc-500">
                Network Rooms
              </span>
              <span className="text-[10px] text-zinc-600 font-mono">{networkRooms.length}</span>
            </div>
            <div className="space-y-0.5">
              {networkRooms.map((room) => {
                const isActive = room.id === activeChannelId;
                return (
                  <button
                    key={room.id}
                    type="button"
                    onClick={() => handleSelectChannel(room.id)}
                    className={`w-full flex items-center justify-between px-3 py-2 rounded-lg text-xs font-medium transition-colors ${
                      isActive
                        ? "bg-emerald-500/10 text-emerald-300 border border-emerald-500/20"
                        : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/50"
                    }`}
                  >
                    <div className="flex items-center gap-2 truncate">
                      <Hash className={`w-3.5 h-3.5 ${isActive ? "text-emerald-400" : "text-zinc-500"}`} />
                      <span className="truncate">{room.name}</span>
                    </div>
                    {room.unreadCount && room.unreadCount > 0 ? (
                      <span className="bg-emerald-500 text-zinc-950 text-[10px] font-bold px-1.5 py-0.2 rounded-full font-mono">
                        {room.unreadCount}
                      </span>
                    ) : null}
                  </button>
                );
              })}
            </div>
          </div>

          {/* Section: Direct Peer Chats */}
          <div>
            <div className="px-3 mb-1.5 flex items-center justify-between">
              <span className="text-[10px] font-semibold uppercase tracking-widest text-zinc-500">
                Direct Peer Chats
              </span>
              <span className="text-[10px] text-zinc-600 font-mono">{directPeers.length}</span>
            </div>
            <div className="space-y-0.5">
              {directPeers.map((peer) => {
                const isActive = peer.id === activeChannelId;
                return (
                  <button
                    key={peer.id}
                    type="button"
                    onClick={() => handleSelectChannel(peer.id)}
                    className={`w-full flex items-center justify-between px-3 py-2 rounded-lg text-xs font-medium transition-colors ${
                      isActive
                        ? "bg-emerald-500/10 text-emerald-300 border border-emerald-500/20"
                        : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/50"
                    }`}
                  >
                    <div className="flex items-center gap-2 truncate">
                      <div className="relative flex-shrink-0">
                        <User className={`w-3.5 h-3.5 ${isActive ? "text-emerald-400" : "text-zinc-500"}`} />
                        <span
                          className={`absolute -bottom-0.5 -right-0.5 w-2 h-2 rounded-full border border-[#0c0c0e] ${
                            peer.online ? "bg-emerald-400" : "bg-zinc-600"
                          }`}
                        />
                      </div>
                      <div className="truncate text-left">
                        <span className="block truncate">{peer.name}</span>
                        {peer.nodeId && (
                          <span className="block text-[9px] text-zinc-600 font-mono truncate">
                            {peer.nodeId}
                          </span>
                        )}
                      </div>
                    </div>
                    {peer.unreadCount && peer.unreadCount > 0 ? (
                      <span className="bg-emerald-500 text-zinc-950 text-[10px] font-bold px-1.5 py-0.2 rounded-full font-mono">
                        {peer.unreadCount}
                      </span>
                    ) : null}
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        {/* Sidebar Footer Security Status */}
        <div className="p-3 border-t border-zinc-800/60 bg-zinc-950/40">
          <div className="flex items-center gap-2 text-[10px] text-zinc-400">
            <Lock className="w-3 h-3 text-emerald-400" />
            <span className="font-mono">AES-256-GCM Direct Mesh</span>
          </div>
        </div>
      </aside>

      {/* Main Chat Pane */}
      <main className="flex-1 flex flex-col min-w-0 bg-[#09090b]">
        {/* Chat Pane Header */}
        <header className="p-4 border-b border-zinc-800/80 bg-[#0c0c0e]/80 flex items-center justify-between gap-3">
          <div className="flex items-center gap-3 truncate">
            <button
              type="button"
              onClick={() => setIsSidebarOpenMobile(true)}
              aria-label="Open channels menu"
              className="md:hidden text-zinc-400 hover:text-zinc-200 p-1 rounded-md"
            >
              <Menu className="w-5 h-5" aria-hidden="true" />
            </button>
            <div className="w-8 h-8 rounded-lg bg-zinc-800/80 border border-zinc-700/50 flex items-center justify-center text-zinc-300">
              {activeChannel.type === "room" ? (
                <Hash className="w-4 h-4 text-emerald-400" />
              ) : (
                <User className="w-4 h-4 text-emerald-400" />
              )}
            </div>
            <div className="truncate">
              <div className="flex items-center gap-2">
                <h3 className="text-sm font-semibold text-zinc-100 truncate">
                  {activeChannel.type === "room" ? `#${activeChannel.name}` : activeChannel.name}
                </h3>
                {activeChannel.type === "direct" && activeChannel.nodeId && (
                  <span className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-zinc-800 text-zinc-400">
                    {activeChannel.nodeId}
                  </span>
                )}
              </div>
              <p className="text-[10px] text-zinc-500 truncate">
                {activeChannel.description ||
                  (activeChannel.online ? "Peer Online" : "Peer Offline")}
              </p>
            </div>
          </div>

          {/* Encryption Status Indicator */}
          <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-emerald-500/10 border border-emerald-500/20 text-emerald-400 text-[10px] font-mono flex-shrink-0">
            <Lock className="w-3 h-3" />
            <span className="hidden sm:inline">P2P Encrypted</span>
          </div>
        </header>

        {/* Message Bubble Stream */}
        <div className="flex-1 overflow-y-auto p-4 space-y-4">
          {activeMessages.length === 0 ? (
            <div className="h-full flex flex-col items-center justify-center text-zinc-500 text-xs">
              <Sparkles className="w-8 h-8 mb-2 text-zinc-600" />
              <p>No messages yet in this channel.</p>
              <p className="text-[10px] text-zinc-600 mt-1">
                Send an encrypted message to start the conversation.
              </p>
            </div>
          ) : (
            activeMessages.map((msg) => (
              <div
                key={msg.id}
                className={`flex flex-col ${msg.isSelf ? "items-end" : "items-start"}`}
              >
                {/* Sender Node Alias & Timestamp Header */}
                <div className="flex items-center gap-2 mb-1 px-1">
                  <span className="text-[10px] font-mono font-medium text-zinc-400">
                    {msg.senderAlias}
                  </span>
                  <span className="text-[9px] font-mono text-zinc-600">
                    ({msg.senderNodeId})
                  </span>
                  <span className="text-[9px] text-zinc-600">{msg.timestamp}</span>
                  {msg.encrypted && (
                    <Lock
                      className="w-2.5 h-2.5 text-emerald-400/80"
                      title="End-to-End Encrypted"
                    />
                  )}
                </div>

                {/* Message Bubble */}
                <div
                  className={`max-w-[85%] sm:max-w-[70%] rounded-2xl px-4 py-2.5 text-xs leading-relaxed shadow-sm ${
                    msg.isSelf
                      ? "bg-emerald-600/20 border border-emerald-500/30 text-emerald-100 rounded-br-none"
                      : "bg-zinc-800/80 border border-zinc-700/60 text-zinc-200 rounded-bl-none"
                  }`}
                >
                  <p className="whitespace-pre-wrap break-words">{msg.content}</p>

                  {/* Attachment Indicator / Details */}
                  {msg.attachment && (
                    <div className="mt-2 pt-2 border-t border-white/10 flex items-center gap-2 text-[11px]">
                      <FileText className="w-4 h-4 text-emerald-400 flex-shrink-0" />
                      <div className="truncate">
                        <span className="font-mono truncate block">{msg.attachment.name}</span>
                        {msg.attachment.size && (
                          <span className="text-[9px] opacity-70 block">
                            {msg.attachment.size}
                          </span>
                        )}
                      </div>
                    </div>
                  )}

                  {/* Delivery / Encryption confirmation icon for self */}
                  {msg.isSelf && (
                    <div className="flex items-center justify-end gap-1 mt-1 text-[9px] text-emerald-400/70">
                      <CheckCheck className="w-3 h-3" />
                    </div>
                  )}
                </div>
              </div>
            ))
          )}
          <div ref={messagesEndRef} />
        </div>

        {/* Selected Attachment Preview Bar */}
        {selectedFile && (
          <div className="px-4 py-2 bg-zinc-900/90 border-t border-zinc-800 flex items-center justify-between text-xs">
            <div className="flex items-center gap-2 text-zinc-300 truncate">
              <Paperclip className="w-3.5 h-3.5 text-emerald-400 flex-shrink-0" />
              <span className="font-mono truncate">{selectedFile.name}</span>
              <span className="text-[10px] text-zinc-500">
                ({(selectedFile.size / 1024).toFixed(1)} KB)
              </span>
            </div>
            <button
              type="button"
              onClick={() => setSelectedFile(null)}
              aria-label="Remove attached file"
              className="text-zinc-500 hover:text-zinc-300 p-1"
            >
              <X className="w-4 h-4" aria-hidden="true" />
            </button>
          </div>
        )}

        {/* Input Controls Footer */}
        <footer className="p-3 border-t border-zinc-800/80 bg-[#0c0c0e]/90">
          <div className="flex items-center gap-2">
            {/* Attachment Button & Hidden File Input */}
            <input
              type="file"
              ref={fileInputRef}
              onChange={handleFileChange}
              className="hidden"
            />
            <button
              type="button"
              onClick={() => fileInputRef.current?.click()}
              aria-label="Attach file to message"
              className={`p-2.5 rounded-lg border transition-colors flex-shrink-0 ${
                selectedFile
                  ? "bg-emerald-500/20 border-emerald-500/40 text-emerald-400"
                  : "bg-zinc-900 border-zinc-800 text-zinc-400 hover:text-zinc-200 hover:border-zinc-700"
              }`}
              title="Attach File"
            >
              <Paperclip className="w-4 h-4" aria-hidden="true" />
            </button>

            {/* Message Text Input */}
            <input
              type="text"
              placeholder={`Message ${
                activeChannel.type === "room" ? `#${activeChannel.name}` : activeChannel.name
              }...`}
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={handleKeyDown}
              className="flex-1 bg-zinc-900 border border-zinc-800 text-xs rounded-lg px-3.5 py-2.5 text-zinc-100 placeholder-zinc-500 focus:outline-none focus:border-zinc-700"
            />

            {/* Send Button */}
            <button
              type="button"
              onClick={handleSend}
              disabled={!inputText.trim() && !selectedFile}
              aria-label="Send message"
              className="p-2.5 rounded-lg bg-emerald-500 text-zinc-950 hover:bg-emerald-400 disabled:opacity-40 disabled:hover:bg-emerald-500 font-medium transition-colors flex-shrink-0"
            >
              <Send className="w-4 h-4" aria-hidden="true" />
            </button>
          </div>

          <div className="flex items-center justify-between mt-2 px-1 text-[9px] text-zinc-500 font-mono">
            <span>Press Enter to send encrypted message</span>
            <span className="flex items-center gap-1">
              <Lock className="w-2.5 h-2.5 text-emerald-400" />
              <span>P2P Channel Active</span>
            </span>
          </div>
        </footer>
      </main>
    </div>
  );
};

export default MeshChatView;
