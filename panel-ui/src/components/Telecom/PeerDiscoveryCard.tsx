import {
  Activity,
  Check,
  Copy,
  MessageSquare,
  Radio,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import React, { useCallback, useState } from "react";

export interface PeerDiscoveryCardProps {
  /** Peer public key or address */
  publicKey: string;
  /** Unique node identifier (optional) */
  nodeId?: string;
  /** Peer user-friendly alias */
  alias?: string;
  /** Node round-trip latency in milliseconds */
  latencyMs?: number | null;
  /** Node connectivity ping status */
  pingStatus?: "online" | "degraded" | "offline" | "pinging";
  /** Last active timestamp string */
  lastSeen?: string;
  /** Callback invoked when initiating a direct end-to-end encrypted chat */
  onInitiateDirectChat?: (peer: {
    publicKey: string;
    alias?: string;
    nodeId?: string;
  }) => void;
  /** Callback invoked when manually triggering a node latency ping */
  onPing?: (publicKey: string) => void;
  /** Additional CSS classes */
  className?: string;
}

export interface LatencyColorInfo {
  bg: string;
  text: string;
  border: string;
  dotBg: string;
  label: string;
}

/**
 * Computes latency badge styling and label based on latency and ping status.
 */
export const getLatencyColor = (
  latencyMs: number | null | undefined,
  pingStatus?: string,
): LatencyColorInfo => {
  if (pingStatus === "offline" || latencyMs === null || latencyMs === undefined) {
    return {
      bg: "bg-rose-500/10",
      text: "text-rose-400",
      border: "border-rose-500/30",
      dotBg: "bg-rose-500",
      label: "Offline",
    };
  }
  if (pingStatus === "pinging") {
    return {
      bg: "bg-cyan-500/10",
      text: "text-cyan-400",
      border: "border-cyan-500/30",
      dotBg: "bg-cyan-400 animate-ping",
      label: "Pinging...",
    };
  }
  if (latencyMs < 50) {
    return {
      bg: "bg-emerald-500/10",
      text: "text-emerald-400",
      border: "border-emerald-500/30",
      dotBg: "bg-emerald-400",
      label: `${latencyMs}ms (Optimal)`,
    };
  }
  if (latencyMs <= 150) {
    return {
      bg: "bg-amber-500/10",
      text: "text-amber-400",
      border: "border-amber-500/30",
      dotBg: "bg-amber-400",
      label: `${latencyMs}ms (Moderate)`,
    };
  }
  return {
    bg: "bg-rose-500/10",
    text: "text-rose-400",
    border: "border-rose-500/30",
    dotBg: "bg-rose-400",
    label: `${latencyMs}ms (High Latency)`,
  };
};

/**
 * Helper to truncate a public key string for compact display.
 */
export const truncatePublicKey = (key: string): string => {
  if (!key) return "";
  if (key.length <= 16) return key;
  return `${key.slice(0, 8)}...${key.slice(-6)}`;
};

/**
 * PeerDiscoveryCard displaying network node discovery info, cryptographic identity,
 * latency/ping status badge, and direct chat action trigger.
 */
export const PeerDiscoveryCard: React.FC<PeerDiscoveryCardProps> = ({
  publicKey,
  nodeId,
  alias,
  latencyMs,
  pingStatus = "online",
  lastSeen = "Just now",
  onInitiateDirectChat,
  onPing,
  className = "",
}) => {
  const [copied, setCopied] = useState(false);
  const [isPinging, setIsPinging] = useState(false);

  // Derive latency color badge info
  const currentPingStatus = isPinging ? "pinging" : pingStatus;
  const latencyColor = getLatencyColor(latencyMs, currentPingStatus);

  const displayAlias = alias || "Unlabeled Peer";
  const truncatedKey = truncatePublicKey(publicKey);

  const handleCopyKey = useCallback(() => {
    if (!publicKey) return;
    navigator.clipboard.writeText(publicKey).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [publicKey]);

  const handlePingNode = useCallback(() => {
    if (onPing) {
      setIsPinging(true);
      onPing(publicKey);
      setTimeout(() => setIsPinging(false), 1000);
    }
  }, [onPing, publicKey]);

  const handleStartChat = useCallback(() => {
    if (onInitiateDirectChat) {
      onInitiateDirectChat({ publicKey, alias, nodeId });
    }
  }, [onInitiateDirectChat, publicKey, alias, nodeId]);

  return (
    <div
      data-testid="peer-discovery-card"
      className={`p-5 rounded-2xl bg-slate-900/90 border border-white/10 shadow-xl backdrop-blur-md flex flex-col justify-between gap-4 transition-all hover:border-white/20 ${className}`}
    >
      {/* Header: Peer Alias & Latency Badge */}
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-xl bg-[#39ff14]/10 border border-[#39ff14]/30 flex items-center justify-center shrink-0">
            <Radio className="w-5 h-5 text-[#39ff14]" aria-hidden="true" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h3 className="text-sm font-semibold text-white tracking-wide">
                {displayAlias}
              </h3>
              <ShieldCheck
                className="w-4 h-4 text-emerald-400 shrink-0"
                aria-label="Verified Cryptographic Identity"
              />
            </div>
            {nodeId && (
              <p className="text-[11px] font-mono text-white/50 tracking-tight">
                Node ID: {nodeId}
              </p>
            )}
          </div>
        </div>

        {/* Latency & Ping Badge */}
        <div
          data-testid="latency-badge"
          className={`flex items-center gap-2 px-2.5 py-1 rounded-full border text-xs font-mono font-medium ${latencyColor.bg} ${latencyColor.text} ${latencyColor.border}`}
        >
          <span
            className={`w-2 h-2 rounded-full ${latencyColor.dotBg}`}
            aria-hidden="true"
          />
          <span>{latencyColor.label}</span>
        </div>
      </div>

      {/* Public Key Snippet with Copy Button */}
      <div className="p-3 rounded-xl bg-black/40 border border-white/5 space-y-1.5">
        <div className="flex items-center justify-between text-[10px] text-white/40 uppercase tracking-wider font-mono">
          <span>Cryptographic Public Key</span>
          <span>Ed25519 / Secp256k1</span>
        </div>
        <div className="flex items-center justify-between gap-2">
          <code
            data-testid="public-key-snippet"
            className="text-xs font-mono text-white/90 select-all tracking-wide truncate"
          >
            {truncatedKey}
          </code>
          <button
            type="button"
            onClick={handleCopyKey}
            aria-label="Copy public key to clipboard"
            className="p-1.5 rounded-lg bg-white/5 hover:bg-white/10 text-white/70 hover:text-white transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] shrink-0 flex items-center gap-1 text-[10px]"
          >
            {copied ? (
              <>
                <Check className="w-3.5 h-3.5 text-emerald-400" aria-hidden="true" />
                <span className="text-emerald-400 font-mono">Copied</span>
              </>
            ) : (
              <>
                <Copy className="w-3.5 h-3.5" aria-hidden="true" />
                <span className="font-mono">Copy</span>
              </>
            )}
          </button>
        </div>
      </div>

      {/* Footer / Action Controls */}
      <div className="flex items-center justify-between gap-3 pt-2 border-t border-white/5 text-xs text-white/50">
        <div className="flex items-center gap-1.5 text-[11px] font-mono">
          <Activity className="w-3.5 h-3.5 text-white/40" aria-hidden="true" />
          <span>Last active: {lastSeen}</span>
        </div>

        <div className="flex items-center gap-2">
          {onPing && (
            <button
              type="button"
              onClick={handlePingNode}
              disabled={isPinging}
              aria-label={`Ping node ${displayAlias}`}
              className="p-2 rounded-xl bg-white/5 hover:bg-white/10 border border-white/10 text-white/70 hover:text-white transition-all disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
            >
              <RefreshCw
                className={`w-3.5 h-3.5 ${isPinging ? "animate-spin text-cyan-400" : ""}`}
                aria-hidden="true"
              />
            </button>
          )}

          <button
            type="button"
            onClick={handleStartChat}
            aria-label={`Start Direct Chat with ${displayAlias}`}
            className="flex items-center gap-2 px-3 py-1.5 rounded-xl bg-[#39ff14]/10 hover:bg-[#39ff14]/20 border border-[#39ff14]/30 text-[#39ff14] font-medium text-xs transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] active:scale-[0.98]"
          >
            <MessageSquare className="w-3.5 h-3.5" aria-hidden="true" />
            <span>Direct Chat</span>
          </button>
        </div>
      </div>
    </div>
  );
};

export default PeerDiscoveryCard;
