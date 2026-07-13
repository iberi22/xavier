import { useEffect, useState, useCallback } from "react";
import { History, RefreshCw, Activity, ShieldCheck, ShieldAlert, Clock } from "lucide-react";
import { motion, AnimatePresence } from "motion/react";
import { ApiClient } from "../../api/client";
import { SecretAuditLog } from "../../types";

interface LeaseHistoryPageProps {
  token: string;
}

export default function LeaseHistoryPage({ token }: LeaseHistoryPageProps) {
  const [history, setHistory] = useState<SecretAuditLog[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const apiClient = new ApiClient(token);

  const fetchHistory = useCallback(async () => {
    try {
      setLoading(true);
      const data = await apiClient.getLeaseHistory();
      setHistory(data);
      setError(null);
    } catch (err) {
      setError("Failed to fetch lease audit logs");
      console.error(err);
    } finally {
      setLoading(false);
    }
  }, [token]);

  useEffect(() => {
    fetchHistory();
  }, [fetchHistory]);

  const getEventIcon = (type: string) => {
    switch (type) {
      case "LEND": return <ShieldCheck className="w-3 h-3 text-[#39ff14]/70" />;
      case "REVOKE": return <ShieldAlert className="w-3 h-3 text-red-400/70" />;
      default: return <Activity className="w-3 h-3 text-white/40" />;
    }
  };

  const getEventColor = (type: string) => {
    switch (type) {
      case "LEND": return "text-[#39ff14]/80";
      case "REVOKE": return "text-red-400/80";
      default: return "text-white/70";
    }
  };

  const formatRelative = (timestamp: string | number): string => {
    const date = new Date(timestamp);
    const diff = (Date.now() - date.getTime()) / 1000;
    if (diff < 60) return "just now";
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-medium text-white/80">Lease Audit History</h3>
          <p className="text-[10px] text-white/30 mt-0.5">
            Immutable log of all secret lending and revocation events.
          </p>
        </div>
        <button
          onClick={fetchHistory}
          className="p-2 text-white/20 hover:text-[#39ff14] hover:bg-[#39ff14]/10 rounded-lg transition-all"
          title="Refresh History"
        >
          <RefreshCw className={`w-4 h-4 ${loading ? "animate-spin" : ""}`} />
        </button>
      </div>

      {error && (
        <div className="p-4 rounded-xl bg-red-500/10 border border-red-500/20 flex items-center gap-3 text-red-400 text-xs">
          <ShieldAlert className="w-4 h-4" />
          {error}
        </div>
      )}

      <div className="space-y-2">
        <AnimatePresence mode="popLayout">
          {history.length === 0 && !loading ? (
            <div className="flex flex-col items-center justify-center py-12 rounded-2xl border border-dashed border-white/5 bg-white/[0.01]">
              <History className="w-12 h-12 text-white/5 mb-4" />
              <p className="text-white/30 text-sm">No audit logs found.</p>
            </div>
          ) : (
            history.map((entry) => (
              <motion.div
                key={entry.id}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                className="flex items-start gap-4 px-4 py-3 rounded-xl border border-white/[0.04] bg-white/[0.02] hover:bg-white/[0.04] transition-colors group"
              >
                <div className={`mt-1 p-1.5 rounded-lg bg-black/40 border border-white/5 group-hover:border-white/10 transition-colors`}>
                  {getEventIcon(entry.event_type)}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between gap-2">
                    <p className={`text-xs font-medium ${getEventColor(entry.event_type)}`}>
                      {entry.event_type === "LEND" ? "Secret Lent" : "Lease Revoked"}
                    </p>
                    <div className="flex items-center gap-1 text-[10px] text-white/20">
                      <Clock className="w-2.5 h-2.5" />
                      <span>{formatRelative(entry.timestamp)}</span>
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-x-3 gap-y-1 mt-1 text-[10px]">
                    <span className="text-white/60">
                      <span className="text-white/20 mr-1 uppercase tracking-tighter font-medium">Agent:</span>
                      {entry.agent_id}
                    </span>
                    {entry.secret_id && (
                      <span className="text-white/60">
                        <span className="text-white/20 mr-1 uppercase tracking-tighter font-medium">Secret:</span>
                        {entry.secret_id}
                      </span>
                    )}
                    <span className="text-white/40 font-mono">
                      <span className="text-white/10 mr-1 uppercase tracking-tighter font-medium">Token:</span>
                      {entry.session_token?.slice(0, 8)}...
                    </span>
                  </div>
                  {entry.reason && (
                    <p className="mt-2 text-[10px] text-white/30 italic border-l border-white/10 pl-2">
                      {entry.reason}
                    </p>
                  )}
                </div>
              </motion.div>
            ))
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
