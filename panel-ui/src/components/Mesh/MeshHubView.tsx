import {
  Activity,
  ArrowLeft,
  CheckCircle,
  Database,
  Globe,
  Heart,
  MessageSquare,
  Network,
  RefreshCw,
  Shield,
  ShieldCheck,
  Vote,
  Wifi,
  Zap,
} from "lucide-react";
import React, { useCallback, useEffect, useState } from "react";
import MeshConfig from "../MeshConfig";
import TopStatusBar from "../TopStatusBar";

export type MeshTab =
  | "networks"
  | "topology"
  | "governance"
  | "chat"
  | "health";

interface MeshHubViewProps {
  token?: string;
  onClose?: () => void;
  initialTab?: MeshTab;
}

interface GovernanceProposal {
  id: string;
  title: string;
  proposer: string;
  votesFor: number;
  votesAgainst: number;
  status: "active" | "passed" | "rejected";
  expiresAt: string;
}

interface PeerHealthInfo {
  nodeId: string;
  alias: string;
  syncLagSecs: number;
  status: "Healthy" | "RetryImmediately" | "RetryWithBackoff" | "Stale";
  lastSeen: string;
}

const MOCK_PROPOSALS: GovernanceProposal[] = [
  {
    id: "prop-101",
    title: "Upgrade Mesh Sync Protocol to v2.4 (Quorum Consensus)",
    proposer: "node_alpha_8f",
    votesFor: 14,
    votesAgainst: 2,
    status: "active",
    expiresAt: "In 2 days",
  },
  {
    id: "prop-102",
    title: "Adjust Validator False-Positive Dampening Factor",
    proposer: "node_beta_3a",
    votesFor: 22,
    votesAgainst: 0,
    status: "passed",
    expiresAt: "Closed",
  },
];

const MOCK_PEER_HEALTH: PeerHealthInfo[] = [
  {
    nodeId: "node_alpha_8f",
    alias: "Primary Gateway",
    syncLagSecs: 12,
    status: "Healthy",
    lastSeen: "2s ago",
  },
  {
    nodeId: "node_beta_3a",
    alias: "Storage Relay",
    syncLagSecs: 45,
    status: "Healthy",
    lastSeen: "10s ago",
  },
  {
    nodeId: "node_gamma_7c",
    alias: "Worker Edge",
    syncLagSecs: 120,
    status: "RetryImmediately",
    lastSeen: "2m ago",
  },
];

export const MeshHubView: React.FC<MeshHubViewProps> = ({
  token = "",
  onClose,
  initialTab = "networks",
}) => {
  const [activeTab, setActiveTab] = useState<MeshTab>(initialTab);
  const [chatMessage, setChatMessage] = useState("");
  const [chatLog, setChatLog] = useState<
    Array<{ sender: string; text: string; time: string }>
  >([
    {
      sender: "Primary Gateway",
      text: "Peer connection established. P2P channel active.",
      time: "10:14",
    },
    {
      sender: "Storage Relay",
      text: "Vector index snapshot synchronized successfully.",
      time: "10:15",
    },
  ]);

  const handleSendMessage = (e: React.FormEvent) => {
    e.preventDefault();
    if (!chatMessage.trim()) return;
    setChatLog((prev) => [
      ...prev,
      {
        sender: "Local Node (You)",
        text: chatMessage.trim(),
        time: new Date().toLocaleTimeString([], {
          hour: "2-digit",
          minute: "2-digit",
        }),
      },
    ]);
    setChatMessage("");
  };

  return (
    <div className="relative w-full h-screen font-sans bg-slate-950 text-white flex flex-col overflow-hidden">
      {/* Top Status Bar Integration */}
      <div className="relative z-50">
        <TopStatusBar isModalOpen={false} />
      </div>

      {/* Navigation Header */}
      <header className="relative z-40 border-b border-white/10 bg-slate-900/80 backdrop-blur-md px-4 py-3 flex items-center justify-between mt-14 sm:mt-16">
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
              <Network className="w-4 h-4 text-[#39ff14]" aria-hidden="true" />
            </div>
            <div>
              <h1 className="text-sm font-semibold tracking-wide text-white">
                Xavier Mesh Hub
              </h1>
              <p className="text-[10px] text-white/50 font-mono">
                P2P Decentralized Node View
              </p>
            </div>
          </div>
        </div>

        {/* Quick Active Network Indicator */}
        <div className="hidden md:flex items-center gap-4 bg-black/40 border border-white/10 px-3 py-1.5 rounded-full text-xs">
          <div className="flex items-center gap-1.5 text-emerald-400 font-mono text-[11px]">
            <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
            <span>Active Network: P2P-Mesh-Mainnet</span>
          </div>
          <div className="w-px h-3 bg-white/10" />
          <div className="flex items-center gap-1 text-white/60 text-[11px]">
            <Globe className="w-3.5 h-3.5 text-cyan-400" aria-hidden="true" />
            <span>3 Connected Peers</span>
          </div>
        </div>

        {/* Tab Navigation */}
        <nav
          className="flex items-center gap-1 bg-black/30 p-1 rounded-xl border border-white/10"
          aria-label="Mesh Navigation Tabs"
        >
          <button
            type="button"
            onClick={() => setActiveTab("networks")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
              activeTab === "networks"
                ? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            <Wifi className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Networks</span>
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("topology")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
              activeTab === "topology"
                ? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            <Network className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Topology</span>
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("governance")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
              activeTab === "governance"
                ? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            <Vote className="w-3.5 h-3.5" aria-hidden="true" />
            <span>DAO Governance</span>
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("chat")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
              activeTab === "chat"
                ? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            <MessageSquare className="w-3.5 h-3.5" aria-hidden="true" />
            <span>P2P Chat</span>
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("health")}
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
              activeTab === "health"
                ? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
                : "text-white/60 hover:text-white hover:bg-white/5"
            }`}
          >
            <Heart className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Family Health</span>
          </button>
        </nav>
      </header>

      {/* Main Tab View Content */}
      <main className="flex-1 overflow-y-auto p-4 md:p-6 bg-slate-950/60">
        {/* Networks Tab */}
        {activeTab === "networks" && (
          <div className="h-full">
            <MeshConfig token={token} />
          </div>
        )}

        {/* Topology Tab */}
        {activeTab === "topology" && (
          <div className="space-y-6 max-w-6xl mx-auto">
            <div className="flex justify-between items-center">
              <div>
                <h2 className="text-xl font-light text-white tracking-tight">
                  Mesh Network Topology
                </h2>
                <p className="text-xs text-white/40 mt-1">
                  Visual node layout, routing distances, and active sync links.
                </p>
              </div>
              <div className="flex gap-2">
                <div className="px-3 py-1 rounded-lg bg-white/5 border border-white/10 text-xs font-mono text-white/70">
                  Total Nodes: 4
                </div>
                <div className="px-3 py-1 rounded-lg bg-emerald-500/10 border border-emerald-500/20 text-xs font-mono text-emerald-400">
                  Mesh Consensus: 100%
                </div>
              </div>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              <div className="p-5 rounded-2xl bg-white/[0.02] border border-white/10 space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-medium text-white/80">
                    Local Node (You)
                  </span>
                  <span className="w-2 h-2 rounded-full bg-[#39ff14]" />
                </div>
                <p className="text-[11px] font-mono text-white/40">
                  Role: Primary Validator
                </p>
                <div className="text-[10px] text-emerald-400/80 bg-emerald-500/10 p-2 rounded-lg border border-emerald-500/20 font-mono">
                  Latency: 0ms (Self)
                </div>
              </div>

              <div className="p-5 rounded-2xl bg-white/[0.02] border border-white/10 space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-medium text-white/80">
                    node_alpha_8f
                  </span>
                  <span className="w-2 h-2 rounded-full bg-emerald-400" />
                </div>
                <p className="text-[11px] font-mono text-white/40">
                  Role: Gateway Node
                </p>
                <div className="text-[10px] text-cyan-400/80 bg-cyan-500/10 p-2 rounded-lg border border-cyan-500/20 font-mono">
                  Latency: 14ms · 1 Hop
                </div>
              </div>

              <div className="p-5 rounded-2xl bg-white/[0.02] border border-white/10 space-y-3">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-medium text-white/80">
                    node_beta_3a
                  </span>
                  <span className="w-2 h-2 rounded-full bg-emerald-400" />
                </div>
                <p className="text-[11px] font-mono text-white/40">
                  Role: Storage Relay
                </p>
                <div className="text-[10px] text-cyan-400/80 bg-cyan-500/10 p-2 rounded-lg border border-cyan-500/20 font-mono">
                  Latency: 28ms · 1 Hop
                </div>
              </div>
            </div>

            <div className="p-6 rounded-2xl bg-black/40 border border-white/10 flex flex-col items-center justify-center min-h-[300px] text-center space-y-4">
              <Network
                className="w-12 h-12 text-[#39ff14]/40 animate-pulse"
                aria-hidden="true"
              />
              <div>
                <h3 className="text-sm font-medium text-white">
                  Live Peer Connections Graph
                </h3>
                <p className="text-xs text-white/40 max-w-md mt-1">
                  P2P nodes communicate over libp2p and WebRTC transport channels with continuous background auto-repair.
                </p>
              </div>
            </div>
          </div>
        )}

        {/* DAO Governance Tab */}
        {activeTab === "governance" && (
          <div className="space-y-6 max-w-6xl mx-auto">
            <div className="flex justify-between items-center">
              <div>
                <h2 className="text-xl font-light text-white tracking-tight">
                  DAO Governance & Tokenomics
                </h2>
                <p className="text-xs text-white/40 mt-1">
                  Decentralized voting proposals and validator penalty metrics.
                </p>
              </div>
              <button
                type="button"
                className="px-4 py-2 bg-[#39ff14]/10 border border-[#39ff14]/30 text-[#39ff14] text-xs font-medium rounded-xl hover:bg-[#39ff14]/20 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
              >
                + New Proposal
              </button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              {MOCK_PROPOSALS.map((prop) => (
                <div
                  key={prop.id}
                  className="p-5 rounded-2xl bg-white/[0.02] border border-white/10 space-y-4"
                >
                  <div className="flex items-center justify-between">
                    <span className="text-[10px] font-mono text-[#39ff14] uppercase tracking-wider">
                      {prop.id}
                    </span>
                    <span
                      className={`text-[10px] px-2 py-0.5 rounded-full font-mono uppercase ${
                        prop.status === "active"
                          ? "bg-amber-500/20 text-amber-300 border border-amber-500/30"
                          : "bg-emerald-500/20 text-emerald-300 border border-emerald-500/30"
                      }`}
                    >
                      {prop.status}
                    </span>
                  </div>
                  <h3 className="text-sm font-medium text-white">
                    {prop.title}
                  </h3>
                  <div className="flex items-center justify-between text-xs text-white/50 border-t border-white/5 pt-3">
                    <span>Proposer: {prop.proposer}</span>
                    <span>Votes: {prop.votesFor} For / {prop.votesAgainst} Against</span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* P2P Chat Tab */}
        {activeTab === "chat" && (
          <div className="max-w-4xl mx-auto h-full flex flex-col bg-slate-900/60 rounded-2xl border border-white/10 overflow-hidden">
            <div className="p-4 border-b border-white/10 bg-black/40 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <MessageSquare
                  className="w-4 h-4 text-[#39ff14]"
                  aria-hidden="true"
                />
                <h3 className="text-sm font-medium text-white">
                  Encrypted Mesh P2P Chat
                </h3>
              </div>
              <span className="text-[10px] font-mono text-emerald-400 bg-emerald-500/10 px-2.5 py-1 rounded-full border border-emerald-500/20">
                End-to-End Encrypted
              </span>
            </div>

            <div className="flex-1 p-4 space-y-3 overflow-y-auto min-h-[320px]">
              {chatLog.map((msg, index) => (
                <div
                  key={`${msg.sender}-${index}`}
                  className={`p-3 rounded-xl max-w-lg ${
                    msg.sender.startsWith("Local Node")
                      ? "ml-auto bg-[#39ff14]/10 border border-[#39ff14]/20 text-white"
                      : "bg-white/5 border border-white/10 text-white/90"
                  }`}
                >
                  <div className="flex justify-between items-center text-[10px] text-white/40 mb-1">
                    <span className="font-medium text-[#39ff14]">
                      {msg.sender}
                    </span>
                    <span className="font-mono">{msg.time}</span>
                  </div>
                  <p className="text-xs leading-relaxed">{msg.text}</p>
                </div>
              ))}
            </div>

            <form
              onSubmit={handleSendMessage}
              className="p-3 border-t border-white/10 bg-black/40 flex gap-2"
            >
              <input
                type="text"
                value={chatMessage}
                onChange={(e) => setChatMessage(e.target.value)}
                placeholder="Type a peer-to-peer message..."
                className="flex-1 bg-white/5 border border-white/10 text-xs text-white px-3 py-2 rounded-xl focus:outline-none focus:border-[#39ff14]/50 font-mono"
              />
              <button
                type="submit"
                className="px-4 py-2 bg-[#39ff14]/20 border border-[#39ff14]/40 text-[#39ff14] text-xs font-medium rounded-xl hover:bg-[#39ff14]/30 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
              >
                Send
              </button>
            </form>
          </div>
        )}

        {/* Family Health Tab */}
        {activeTab === "health" && (
          <div className="space-y-6 max-w-6xl mx-auto">
            <div className="flex justify-between items-center">
              <div>
                <h2 className="text-xl font-light text-white tracking-tight">
                  Family Node Health & Auto-Repair Module
                </h2>
                <p className="text-xs text-white/40 mt-1">
                  Peer reconnection status, sync lag, and auto-repair policies.
                </p>
              </div>
              <div className="flex items-center gap-2 bg-emerald-500/10 border border-emerald-500/20 text-emerald-400 text-xs px-3 py-1.5 rounded-xl font-mono">
                <CheckCircle className="w-4 h-4" aria-hidden="true" />
                <span>Auto-Repair Engine: Enabled</span>
              </div>
            </div>

            <div className="space-y-3">
              {MOCK_PEER_HEALTH.map((peer) => (
                <div
                  key={peer.nodeId}
                  className="p-4 rounded-2xl bg-white/[0.02] border border-white/10 flex flex-col md:flex-row items-start md:items-center justify-between gap-4"
                >
                  <div className="flex items-center gap-3">
                    <Heart
                      className={`w-5 h-5 ${
                        peer.status === "Healthy"
                          ? "text-emerald-400"
                          : "text-amber-400 animate-bounce"
                      }`}
                      aria-hidden="true"
                    />
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-white">
                          {peer.alias}
                        </span>
                        <code className="text-[10px] text-white/40 font-mono">
                          {peer.nodeId}
                        </code>
                      </div>
                      <p className="text-xs text-white/40 mt-0.5">
                        Last seen: {peer.lastSeen}
                      </p>
                    </div>
                  </div>

                  <div className="flex items-center gap-4 text-xs">
                    <div className="text-right">
                      <span className="text-[10px] uppercase text-white/40 block font-mono">
                        Sync Lag
                      </span>
                      <span className="font-mono text-white/80">
                        {peer.syncLagSecs}s
                      </span>
                    </div>

                    <div className="text-right">
                      <span className="text-[10px] uppercase text-white/40 block font-mono">
                        Auto-Repair Decision
                      </span>
                      <span
                        className={`font-mono text-[11px] px-2 py-0.5 rounded ${
                          peer.status === "Healthy"
                            ? "bg-emerald-500/20 text-emerald-300"
                            : "bg-amber-500/20 text-amber-300"
                        }`}
                      >
                        {peer.status}
                      </span>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </main>
    </div>
  );
};

export default MeshHubView;
