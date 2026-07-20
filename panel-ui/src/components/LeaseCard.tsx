import { Clock, Key, Shield, Trash2, User } from "lucide-react";
import type { SecretLease } from "../types";

interface LeaseCardProps {
  lease: SecretLease;
  onRevoke: (token: string) => void;
}

export default function LeaseCard({ lease, onRevoke }: LeaseCardProps) {
  const expiresAt = new Date(lease.expires_at);
  const isExpired = expiresAt < new Date();
  const timeLeft = Math.max(
    0,
    Math.floor((expiresAt.getTime() - Date.now()) / 1000),
  );

  const formatTimeLeft = (seconds: number) => {
    if (seconds > 3600)
      return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
    if (seconds > 60) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
    return `${seconds}s`;
  };

  return (
    <div
      className={`p-4 rounded-xl border transition-all ${isExpired ? "bg-red-500/5 border-red-500/20 opacity-60" : "bg-white/[0.02] border-white/[0.06] hover:border-[#39ff14]/30"}`}
    >
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-3">
          <div
            className={`w-10 h-10 rounded-lg flex items-center justify-center ${isExpired ? "bg-red-500/10" : "bg-[#39ff14]/10"}`}
          >
            <Key
              className={`w-5 h-5 ${isExpired ? "text-red-400" : "text-[#39ff14]"}`}
            />
          </div>
          <div>
            <h4 className="text-sm font-medium text-white/90">
              {lease.secret_name}
            </h4>
            <div className="flex items-center gap-2 mt-1">
              <span className="flex items-center gap-1 text-[10px] text-white/40">
                <User className="w-3 h-3" />
                {lease.agent_id}
              </span>
              <span className="text-white/20">|</span>
              <code
                className="text-[10px] font-mono text-white/30 truncate max-w-[120px]"
                title={lease.token}
              >
                {lease.token.slice(0, 8)}...
              </code>
            </div>
          </div>
        </div>
        <button
          onClick={() => onRevoke(lease.token)}
          className="p-2 text-white/20 hover:text-red-400 hover:bg-red-500/10 rounded-lg transition-all"
          title="Revoke Lease"
        >
          <Trash2 className="w-4 h-4" />
        </button>
      </div>

      <div className="mt-4 flex items-center justify-between">
        <div className="flex items-center gap-4">
          <div className="flex flex-col">
            <span className="text-[9px] uppercase tracking-widest text-white/20">
              Time Left
            </span>
            <span
              className={`text-xs font-mono mt-0.5 ${timeLeft < 300 ? "text-amber-400" : "text-white/60"}`}
            >
              {isExpired ? "Expired" : formatTimeLeft(timeLeft)}
            </span>
          </div>
          <div className="flex flex-col">
            <span className="text-[9px] uppercase tracking-widest text-white/20">
              Created
            </span>
            <span className="text-xs text-white/60 mt-0.5 font-mono">
              {new Date(lease.created_at).toLocaleTimeString()}
            </span>
          </div>
        </div>
        <div className="flex items-center gap-1.5 px-2 py-1 rounded bg-black/40 border border-white/5">
          <Shield className="w-3 h-3 text-[#39ff14]/50" />
          <span className="text-[10px] text-white/40 uppercase tracking-tighter">
            Verified
          </span>
        </div>
      </div>

      {!isExpired && (
        <div className="mt-3 h-1 w-full bg-white/5 rounded-full overflow-hidden">
          <div
            className={`h-full transition-all duration-1000 ${timeLeft < 300 ? "bg-amber-400" : "bg-[#39ff14]"}`}
            style={{ width: `${Math.min(100, (timeLeft / 3600) * 100)}%` }}
          />
        </div>
      )}
    </div>
  );
}
