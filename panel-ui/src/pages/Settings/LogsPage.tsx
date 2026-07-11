import { useCallback, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  Bug,
  Clock,
  FileText,
  Info,
  RefreshCw,
  ScrollText,
  Search,
  Zap,
} from "lucide-react";
import { motion, AnimatePresence } from "motion/react";
import { ApiClient, type LogEntry, type LogStats } from "../../api/client";

interface LogsPageProps {
  token: string;
}

type LevelFilter = "all" | "error" | "warn" | "info" | "debug";

const POLL_INTERVAL_MS = 3000;

export default function LogsPage({ token }: LogsPageProps) {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [stats, setStats] = useState<LogStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [levelFilter, setLevelFilter] = useState<LevelFilter>("all");
  const [query, setQuery] = useState("");
  const [autoRefresh, setAutoRefresh] = useState(true);

  const apiClient = useRef(new ApiClient(token));
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const fetchLogs = useCallback(async () => {
    try {
      const data = await apiClient.current.getLogs({
        level: levelFilter === "all" ? undefined : levelFilter,
        q: query.trim() || undefined,
        limit: 200,
      });
      setLogs(data);
      setError(null);
    } catch (err) {
      setError("Failed to fetch logs — is the server running?");
      console.error(err);
    } finally {
      setLoading(false);
    }
  }, [levelFilter, query]);

  const fetchStats = useCallback(async () => {
    try {
      const data = await apiClient.current.getLogStats();
      setStats(data);
    } catch {
      // Stats are optional — the log list still renders.
    }
  }, []);

  // Initial + manual fetch
  useEffect(() => {
    setLoading(true);
    fetchLogs();
    fetchStats();
  }, [fetchLogs, fetchStats]);

  // Debounced refetch when the text query changes (avoid hammering the API
  // on every keystroke).
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => fetchLogs(), 250);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [query, fetchLogs]);

  // Auto-refresh polling.
  useEffect(() => {
    if (!autoRefresh) return;
    const id = setInterval(() => {
      fetchLogs();
      fetchStats();
    }, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [autoRefresh, fetchLogs, fetchStats]);

  const getLevelIcon = (level: LogEntry["level"]) => {
    switch (level) {
      case "error":
        return <AlertTriangle className="w-3 h-3 text-red-400" />;
      case "warn":
        return <AlertTriangle className="w-3 h-3 text-amber-400" />;
      case "debug":
      case "trace":
        return <Bug className="w-3 h-3 text-cyan-400" />;
      default:
        return <Info className="w-3 h-3 text-[#39ff14]" />;
    }
  };

  const getLevelColor = (level: LogEntry["level"]) => {
    switch (level) {
      case "error":
        return "text-red-400";
      case "warn":
        return "text-amber-400";
      case "debug":
      case "trace":
        return "text-cyan-400";
      default:
        return "text-[#39ff14]";
    }
  };

  const getLevelBadge = (level: LogEntry["level"]) => {
    const base = "px-1.5 py-0.5 rounded text-[9px] font-bold uppercase tracking-wider";
    switch (level) {
      case "error":
        return `${base} bg-red-500/15 text-red-400 border border-red-500/20`;
      case "warn":
        return `${base} bg-amber-500/15 text-amber-400 border border-amber-500/20`;
      case "debug":
      case "trace":
        return `${base} bg-cyan-500/15 text-cyan-400 border border-cyan-500/20`;
      default:
        return `${base} bg-[#39ff14]/15 text-[#39ff14] border border-[#39ff14]/20`;
    }
  };

  const formatTime = (timestamp: string): string => {
    try {
      const d = new Date(timestamp);
      return d.toLocaleTimeString([], { hour12: false });
    } catch {
      return timestamp;
    }
  };

  const levelCounts = {
    error: logs.filter((l) => l.level === "error").length,
    warn: logs.filter((l) => l.level === "warn").length,
    info: logs.filter((l) => l.level === "info").length,
  };

  return (
    <div className="space-y-4 h-full flex flex-col">
      {/* Header + stats */}
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-medium text-white/80 flex items-center gap-2">
            <ScrollText className="w-4 h-4 text-[#39ff14]" />
            System Logs
          </h3>
          <p className="text-[10px] text-white/30 mt-0.5">
            Live observability stream from the service log store.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => {
              setAutoRefresh((v) => !v);
            }}
            className={`px-2.5 py-1.5 rounded-lg text-[10px] font-medium transition-all flex items-center gap-1.5 border ${
              autoRefresh
                ? "bg-[#39ff14]/10 text-[#39ff14] border-[#39ff14]/30"
                : "bg-white/[0.02] text-white/40 border-white/10 hover:text-white/60"
            }`}
            title="Toggle auto-refresh"
          >
            <Zap className="w-3 h-3" />
            {autoRefresh ? "Live" : "Paused"}
          </button>
          <button
            onClick={() => {
              fetchLogs();
              fetchStats();
            }}
            className="p-2 text-white/20 hover:text-[#39ff14] hover:bg-[#39ff14]/10 rounded-lg transition-all"
            title="Refresh"
          >
            <RefreshCw className={`w-4 h-4 ${loading ? "animate-spin" : ""}`} />
          </button>
        </div>
      </div>

      {/* Stats row */}
      <div className="grid grid-cols-4 gap-2">
        <StatCard
          label="Total"
          value={stats?.total_entries ?? "—"}
          icon={<FileText className="w-3 h-3" />}
        />
        <StatCard
          label="Errors / 1h"
          value={stats?.errors_last_hour ?? "—"}
          accent="error"
          icon={<AlertTriangle className="w-3 h-3" />}
        />
        <StatCard
          label="Errors / 24h"
          value={stats?.errors_today ?? "—"}
          accent="error"
          icon={<AlertTriangle className="w-3 h-3" />}
        />
        <StatCard
          label="Warnings / 24h"
          value={stats?.warnings_today ?? "—"}
          accent="warn"
          icon={<AlertTriangle className="w-3 h-3" />}
        />
      </div>

      {/* Filters */}
      <div className="flex items-center gap-2">
        <div className="flex gap-1 bg-black/40 rounded-lg p-1 border border-white/5">
          {(["all", "error", "warn", "info", "debug"] as LevelFilter[]).map(
            (lvl) => (
              <button
                key={lvl}
                onClick={() => setLevelFilter(lvl)}
                className={`px-2.5 py-1 rounded-md text-[10px] font-medium uppercase tracking-wide transition-all ${
                  levelFilter === lvl
                    ? "bg-[#39ff14]/15 text-[#39ff14]"
                    : "text-white/40 hover:text-white/70"
                }`}
              >
                {lvl}
                {lvl !== "all" && levelCounts[lvl as keyof typeof levelCounts] !== undefined && (
                  <span className="ml-1 opacity-50">
                    {levelCounts[lvl as keyof typeof levelCounts]}
                  </span>
                )}
              </button>
            ),
          )}
        </div>
        <div className="flex-1 relative">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-white/20" />
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search logs (message, metadata)…"
            className="w-full h-8 bg-black/50 border border-white/10 rounded-lg pl-8 pr-3 text-xs text-white/90 outline-none focus:border-[#39ff14]/40 transition-colors placeholder:text-white/20"
          />
        </div>
      </div>

      {error && (
        <div className="p-3 rounded-xl bg-red-500/10 border border-red-500/20 flex items-center gap-2 text-red-400 text-xs">
          <AlertTriangle className="w-4 h-4 shrink-0" />
          {error}
        </div>
      )}

      {/* Log list */}
      <div className="flex-1 overflow-y-auto space-y-1 pr-1">
        <AnimatePresence mode="popLayout">
          {logs.length === 0 && !loading ? (
            <div className="flex flex-col items-center justify-center py-16 rounded-2xl border border-dashed border-white/5 bg-white/[0.01]">
              <ScrollText className="w-12 h-12 text-white/5 mb-3" />
              <p className="text-white/30 text-sm">No log entries found.</p>
              <p className="text-white/15 text-[10px] mt-1">
                Server errors (5xx) are captured here automatically.
              </p>
            </div>
          ) : (
            logs.map((entry) => (
              <motion.div
                key={entry.id}
                layout
                initial={{ opacity: 0, x: -8 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0 }}
                className="flex items-start gap-2.5 px-3 py-2 rounded-lg border border-white/[0.03] bg-white/[0.015] hover:bg-white/[0.04] hover:border-white/[0.08] transition-colors group font-mono"
              >
                {/* Timestamp */}
                <div className="flex items-center gap-1 text-[10px] text-white/25 shrink-0 w-16 pt-0.5">
                  <Clock className="w-2.5 h-2.5" />
                  <span>{formatTime(entry.timestamp)}</span>
                </div>

                {/* Level badge */}
                <div className="shrink-0 w-14 flex justify-center pt-0.5">
                  <span className={getLevelBadge(entry.level)}>
                    {getLevelIcon(entry.level)}
                    <span className="ml-0.5 align-middle">{entry.level}</span>
                  </span>
                </div>

                {/* Source/module */}
                {entry.module && (
                  <div className="shrink-0 text-[10px] text-white/30 w-40 truncate pt-0.5">
                    {entry.module}
                  </div>
                )}

                {/* Message */}
                <div className="flex-1 min-w-0 pt-0.5">
                  <p className={`text-[11px] leading-relaxed break-words ${getLevelColor(entry.level)}`}>
                    {entry.message}
                  </p>
                  {entry.source && (
                    <span className="text-[9px] text-white/15 uppercase tracking-tight">
                      {entry.source}
                    </span>
                  )}
                </div>
              </motion.div>
            ))
          )}
        </AnimatePresence>
        {loading && logs.length === 0 && (
          <div className="flex items-center justify-center py-8 text-white/20 text-xs">
            <RefreshCw className="w-4 h-4 animate-spin mr-2" />
            Loading logs…
          </div>
        )}
      </div>
    </div>
  );
}

function StatCard({
  label,
  value,
  icon,
  accent,
}: {
  label: string;
  value: number | string;
  icon: React.ReactNode;
  accent?: "error" | "warn";
}) {
  const valueColor =
    accent === "error"
      ? "text-red-400"
      : accent === "warn"
        ? "text-amber-400"
        : "text-white/90";
  return (
    <div className="px-3 py-2 rounded-xl border border-white/[0.04] bg-white/[0.02]">
      <div className="flex items-center gap-1.5 text-[9px] uppercase tracking-wider text-white/30 mb-1">
        {icon}
        {label}
      </div>
      <div className={`text-lg font-light ${valueColor}`}>{value}</div>
    </div>
  );
}
