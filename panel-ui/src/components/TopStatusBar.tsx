import {
  Activity,
  Bell,
  Bot,
  Crown,
  Database,
  Globe,
  Hash,
  Home,
  Key,
  MessageCircle,
  MessageSquare,
  RefreshCw,
  Send,
  Server,
  Settings,
  ShieldCheck,
  Users,
  Wifi,
  WifiOff,
  X,
  Zap,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect, useRef, useState } from "react";
import { ApiClient, getApiUrl, getRemoteUrl, setRemoteUrl } from "../api/client";
import { getApiTokenSync } from "../hooks/useApiToken";
import FounderNodeStatusCard from "./FounderNodeStatusCard";
import MessagingConfigModal from "./MessagingConfigModal";
import NotificationsDropdown from "./NotificationsDropdown";
import OperationModeBadge from "./OperationModeBadge";
import WorkspaceSelector from "./WorkspaceSelector";
import LoadingSpinner from "./ui/LoadingSpinner";

type MessagingPlatform =
  | "telegram"
  | "discord"
  | "slack"
  | "teams"
  | "whatsapp";

interface TopStatusBarProps {
  isModalOpen?: boolean;
  isLoading?: boolean;
}

// Declare the vite define constant
declare const __APP_VERSION__: string;

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function getToken(): string {
  return getApiTokenSync();
}

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Wrapped TopStatusBar in React.memo()
 * 🎯 Why: TopStatusBar is a complex component containing multiple intervals, fetches,
 *         and animated layout calculations. Updates to chat messages or other parent state in App.tsx shouldn't
 *         re-render this bar unnecessarily.
 * 📊 Impact: Prevents unnecessary heavy tree renders and layout recalculations.
 */
export default React.memo(function TopStatusBar({
  isModalOpen = false,
  isLoading: propIsLoading,
}: TopStatusBarProps) {
  const [time, setTime] = useState(new Date());
  const [memoryCount, setMemoryCount] = useState(0);
  const [unreadCount, setUnreadCount] = useState(0);
  const [internalLoading, setInternalLoading] = useState(true);
  const isLoading = propIsLoading !== undefined ? propIsLoading : internalLoading;
  const [metrics, setMetrics] = useState({
    cpu_percent: 0,
    ram_used_gb: 0,
    ram_total_gb: 0,
  });
  const [config, setConfig] = useState({
    has_openai: false,
    has_gemini: false,
    has_telegram: false,
  });
  const [showConfig, setShowConfig] = useState(false);
  const [showFounderCard, setShowFounderCard] = useState(false);
  const [showMessaging, setShowMessaging] = useState(false);
  const [messagingTab, setMessagingTab] =
    useState<MessagingPlatform>("telegram");
  const [showNotifications, setShowNotifications] = useState(false);
  const bellRef = useRef<HTMLButtonElement>(null);
  const [nodeStatus, setNodeStatus] = useState<"connected" | "disconnected" | "retrying">("connected");
  const [retryCount, setRetryCount] = useState(0);
  const [remoteUrl, setRemoteUrlState] = useState<string>(getRemoteUrl());
  const [showNodeModal, setShowNodeModal] = useState(false);
  const [inputRemoteUrl, setInputRemoteUrl] = useState<string>(remoteUrl);
  const [testingConnection, setTestingConnection] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null);

  // Manual Memory Sync state
  const [showSyncPopover, setShowSyncPopover] = useState(false);
  const [isSyncing, setIsSyncing] = useState(false);
  const [syncLatencyMs, setSyncLatencyMs] = useState<number | null>(null);
  const [lastSyncTime, setLastSyncTime] = useState<Date | null>(null);

  const [modules, setModules] = useState({
    time: true,
    founder: true,
    channels: true,
    resources: true,
    security: true,
    sync: true,
    ai: true,
    notifications: true,
  });

  const toggleModule = (key: keyof typeof modules) => {
    setModules((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  useEffect(() => {
    const fetchConfig = async () => {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          const res = await invoke<any>("get_current_config_state");
          if (res) setConfig(res);
        } catch (err) {
          console.debug("Error fetching config state via Tauri:", err);
        }
      } else {
        try {
          const token = getToken();
          const res = await fetch(getApiUrl("/v1/config/providers"), {
            headers: { "X-Xavier-Token": token },
          });
          if (res.ok) {
            const data = await res.json();
            const providers = data.providers || [];
            const has_openai = providers.some(
              (p: any) => p.provider === "openai" && (p.api_key || p.model),
            );
            const has_gemini = providers.some(
              (p: any) => p.provider === "gemini" && (p.api_key || p.model),
            );
            setConfig((prev) => ({ ...prev, has_openai, has_gemini }));
          }
        } catch (err) {
          console.debug("Error fetching config fallback:", err);
        }
      }
    };

    fetchConfig();

    const fetchMetrics = async () => {
      // 1. Fetch realtime metrics from Tauri or HTTP /health
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          const met = await invoke<any>("get_realtime_metrics");
          if (met) setMetrics(met);
          setNodeStatus("connected");
          setRetryCount(0);
        } catch (err) {
          console.debug("Error fetching realtime metrics:", err);
        }
      } else {
        try {
          const res = await fetch(getApiUrl("/health"));
          if (res.ok) {
            setNodeStatus("connected");
            setRetryCount(0);
            const data = await res.json();
            if (data.system) {
              const cpu_percent = data.system.cpu_usage ?? 0;
              const ram_percent = data.system.ram_usage_percent ?? 0;
              const deviceMemory =
                (navigator as unknown as { deviceMemory?: number })
                  .deviceMemory || 8;
              const ram_used_gb = (ram_percent / 100) * deviceMemory;
              setMetrics({
                cpu_percent,
                ram_used_gb,
                ram_total_gb: deviceMemory,
              });
            }
          } else {
            setNodeStatus("disconnected");
            setRetryCount((prev) => prev + 1);
          }
        } catch (err) {
          console.debug("Error fetching metrics from /health:", err);
          setNodeStatus("disconnected");
          setRetryCount((prev) => prev + 1);
        }
      }

      // 2. Fetch memory count from REST API
      try {
        const token = getToken();
        const res = await fetch(getApiUrl("/v1/memories?limit=1"), {
          headers: { "X-Xavier-Token": token },
        });
        if (res.ok) {
          const data = await res.json();
          if (data.pagination?.total !== undefined) {
            setMemoryCount(data.pagination.total);
          }
        }
      } catch (err) {
        console.debug("Error fetching memories count:", err);
      }

      // 3. Fetch notifications unread count from REST API
      try {
        const token = getToken();
        const res = await fetch(getApiUrl("/notifications"), {
          headers: { "X-Xavier-Token": token },
        });
        if (res.ok) {
          const data = await res.json();
          if (Array.isArray(data)) {
            const unread = data.filter((n: any) => !n.read).length;
            setUnreadCount(unread);
          } else {
            setUnreadCount(0);
          }
        } else {
          setUnreadCount(0);
        }
      } catch (err) {
        console.debug("Error fetching notifications unread count:", err);
        setUnreadCount(0);
      } finally {
        setInternalLoading(false);
      }
    };

    fetchMetrics();
    const metricsInterval = setInterval(fetchMetrics, 3000);
    const timeInterval = setInterval(() => setTime(new Date()), 1000);

    let unlisten: (() => void) | undefined;
    let notifyInterval: ReturnType<typeof setInterval> | undefined;

    if (isTauri) {
      import("@tauri-apps/api/event")
        .then(({ listen }) => {
          listen<any>("new-notification", () => {
            fetchMetrics();
          }).then((fn) => {
            unlisten = fn;
          });
        })
        .catch((err) => console.debug("Error listening for events:", err));
    } else {
      notifyInterval = setInterval(fetchMetrics, 30000);
    }

    return () => {
      clearInterval(metricsInterval);
      clearInterval(timeInterval);
      if (notifyInterval) clearInterval(notifyInterval);
      if (unlisten) unlisten();
    };
  }, []);

  const spring = { type: "spring" as const, stiffness: 200, damping: 25 };

  // Open messaging modal on a specific platform icon click
  const openMessaging = (platform: MessagingPlatform) => {
    setMessagingTab(platform);
    setShowMessaging(true);
  };

  const handleTriggerSync = async () => {
    if (isSyncing) return;
    setIsSyncing(true);
    const startTime = performance.now();
    try {
      const token = getToken();
      const client = new ApiClient(token);

      // Step 1: Push local changes to peer/cloud memory sync
      await client.syncPush();

      // Step 2: Pull remote changes from peer/cloud memory sync
      await client.syncPull();

      const elapsed = Math.round(performance.now() - startTime);
      setSyncLatencyMs(elapsed);
      setLastSyncTime(new Date());

      if (typeof window !== "undefined" && window.dispatchEvent) {
        window.dispatchEvent(
          new CustomEvent("xavier-toast", {
            detail: {
              message: `Memory sync completed successfully (${elapsed}ms)`,
              type: "success",
            },
          }),
        );
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Sync operation failed";
      if (typeof window !== "undefined" && window.dispatchEvent) {
        window.dispatchEvent(
          new CustomEvent("xavier-error-toast", {
            detail: {
              message: `Sync failed: ${msg}`,
              type: "error",
            },
          }),
        );
      }
    } finally {
      setIsSyncing(false);
    }
  };

  const handleSaveRemoteUrl = (url: string | null) => {
    setRemoteUrl(url);
    const active = getRemoteUrl();
    setRemoteUrlState(active);
    setInputRemoteUrl(active);
    setTestResult(null);
    setShowNodeModal(false);
  };

  const handleTestConnection = async () => {
    setTestingConnection(true);
    setTestResult(null);
    try {
      const target = inputRemoteUrl.trim().replace(/\/+$/, "");
      const healthUrl = target ? `${target}/health` : getApiUrl("/health");
      const res = await fetch(healthUrl);
      if (res.ok) {
        setTestResult({ ok: true, msg: "Connected successfully to node!" });
      } else {
        setTestResult({
          ok: false,
          msg: `HTTP Error ${res.status}: ${res.statusText}`,
        });
      }
    } catch (_err) {
      setTestResult({
        ok: false,
        msg: "Connection failed. Check node URL, CORS policies or network availability.",
      });
    } finally {
      setTestingConnection(false);
    }
  };

  return (
    <>
      <div className="absolute inset-0 z-[60] pointer-events-none overflow-hidden">
        {/* Left Group */}
        <motion.div
          layout
          transition={spring}
          className={`flex gap-2 pointer-events-auto ${isModalOpen ? "absolute left-2 lg:left-4 top-1/2 -translate-y-1/2 flex-col items-start z-[60]" : "absolute left-4 md:left-6 top-6 flex-row items-start"}`}
        >
          {/* Time & Date Pill */}
          {modules.time && (
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-3 py-1 flex items-center gap-2 h-7 text-white/80 shrink-0"
            >
              <span className="font-mono text-[10px] hidden md:inline-block">
                {time.toLocaleDateString(undefined, {
                  month: "numeric",
                  day: "numeric",
                })}
              </span>
              <div className="w-px h-2.5 bg-white/20 hidden md:block" />
              <span className="font-mono text-[10px] min-w-[50px] text-center">
                {time.toLocaleTimeString(undefined, {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </span>
            </motion.div>
          )}

          {/* SWAL Genesis Founder Node Telemetry Badge */}
          {modules.founder && (
            <div className="relative">
              <motion.button
                layout
                transition={spring}
                type="button"
                onClick={() => setShowFounderCard((prev) => !prev)}
                className="bg-[#0a0a0a]/80 backdrop-blur-md border border-amber-500/30 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0 hover:bg-amber-500/10 transition-colors"
                title="SWAL Genesis Founder Node Telemetry"
                aria-label="Founder Node Telemetry HUD Card"
              >
                <Crown className="w-3 h-3 text-amber-400" />
                <span className="font-mono text-[9px] text-amber-300 font-bold tracking-wider uppercase hidden sm:inline-block">
                  Founder Node
                </span>
                <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
              </motion.button>

              <AnimatePresence>
                {showFounderCard && (
                  <motion.div
                    initial={{ opacity: 0, y: 10, scale: 0.95 }}
                    animate={{ opacity: 1, y: 0, scale: 1 }}
                    exit={{ opacity: 0, y: 10, scale: 0.95 }}
                    className="absolute left-0 top-full mt-2 z-[80]"
                  >
                    <FounderNodeStatusCard
                      onClose={() => setShowFounderCard(false)}
                    />
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          )}

          <OperationModeBadge />

          <WorkspaceSelector />

          {/* System Resources Pill */}
          {modules.resources && (
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-3 py-1 flex items-center gap-3 h-7 shrink-0 hidden lg:flex"
            >
              {isLoading ? (
                <div className="flex items-center gap-1.5 px-1">
                  <LoadingSpinner size={12} />
                  <span className="font-mono text-[10px] text-white/50">
                    Loading...
                  </span>
                </div>
              ) : (
                <>
                  <div
                    className="flex items-center gap-1 text-[10px] text-white/70"
                    title={`Memory: ${metrics.ram_used_gb.toFixed(1)}GB / ${metrics.ram_total_gb.toFixed(1)}GB`}
                  >
                    <Database className="w-3 h-3 text-blue-400" />
                    <span className="font-mono">
                      {Math.round(metrics.ram_used_gb)}G
                    </span>
                  </div>
                  <div
                    className="flex items-center gap-1 text-[10px] text-white/70"
                    title={`CPU: ${Math.round(metrics.cpu_percent)}%`}
                  >
                    <Activity className="w-3 h-3 text-red-400" />
                    <span className="font-mono">
                      {Math.round(metrics.cpu_percent)}%
                    </span>
                  </div>
                  <div
                    className="flex items-center gap-1 text-[10px] text-[#39ff14]"
                    title="GPU: ON"
                  >
                    <Zap className="w-3 h-3 fill-[#39ff14]/20" />
                  </div>
                </>
              )}
            </motion.div>
          )}

          {/* Communication Channels — each icon clickable */}
          {modules.channels && (
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-2 h-7 shrink-0 hidden md:flex"
            >
              {/* Discord */}
              <button
                type="button"
                onClick={() => openMessaging("discord")}
                title="Discord — Click to configure"
                aria-label="Configure Discord"
                className="relative group p-0.5 rounded-full hover:bg-indigo-500/10 transition-colors"
              >
                <MessageCircle
                  className="w-3 h-3 text-indigo-400/40 group-hover:text-indigo-400 transition-colors"
                  aria-hidden="true"
                />
              </button>

              {/* Slack */}
              <button
                type="button"
                onClick={() => openMessaging("slack")}
                title="Slack — Click to configure"
                aria-label="Configure Slack"
                className="relative group p-0.5 rounded-full hover:bg-amber-500/10 transition-colors"
              >
                <Hash
                  className="w-3 h-3 text-amber-400/40 group-hover:text-amber-400 transition-colors"
                  aria-hidden="true"
                />
              </button>

              {/* Teams */}
              <button
                type="button"
                onClick={() => openMessaging("teams")}
                title="MS Teams — Click to configure"
                aria-label="Configure MS Teams"
                className="relative group p-0.5 rounded-full hover:bg-purple-500/10 transition-colors"
              >
                <Users
                  className="w-3 h-3 text-purple-400/40 group-hover:text-purple-400 transition-colors"
                  aria-hidden="true"
                />
              </button>

              {/* Telegram — may be configured */}
              <button
                type="button"
                onClick={() => openMessaging("telegram")}
                title={
                  config.has_telegram
                    ? "Telegram (Active)"
                    : "Telegram — Click to configure"
                }
                aria-label={
                  config.has_telegram
                    ? "Telegram (Active)"
                    : "Configure Telegram"
                }
                className="relative group p-0.5 rounded-full hover:bg-blue-500/10 transition-colors"
              >
                <Send
                  className={`w-3 h-3 transition-colors ${config.has_telegram ? "text-blue-400" : "text-blue-400/40 group-hover:text-blue-400"}`}
                  aria-hidden="true"
                />
                {config.has_telegram && (
                  <div
                    className="absolute -top-0.5 -right-0.5 w-1.5 h-1.5 rounded-full bg-blue-400 msg-active-dot shadow-[0_0_4px_rgba(96,165,250,0.6)]"
                    aria-hidden="true"
                  />
                )}
              </button>

              {/* WhatsApp */}
              <button
                type="button"
                onClick={() => openMessaging("whatsapp")}
                title="WhatsApp — Click to configure"
                aria-label="Configure WhatsApp"
                className="relative group p-0.5 rounded-full hover:bg-green-500/10 transition-colors"
              >
                <MessageSquare
                  className="w-3 h-3 text-green-400/40 group-hover:text-green-400 transition-colors"
                  aria-hidden="true"
                />
              </button>
            </motion.div>
          )}
        </motion.div>

        {/* Center — Identity */}
        <motion.div
          layout
          transition={spring}
          className="absolute pointer-events-auto top-6 left-1/2 -translate-x-1/2 z-[60]"
        >
          <div className="relative group">
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/90 backdrop-blur-md border border-[#39ff14]/20 shadow-[0_0_12px_rgba(57,255,20,0.08)] rounded-full px-3 py-1 flex items-center justify-center cursor-default shrink-0 min-h-[28px]"
            >
              <span className="text-[#39ff14] font-mono tracking-widest text-[8px] uppercase font-bold">
                Xavier {__APP_VERSION__}
              </span>
            </motion.div>

            {/* Gear Button */}
            <button
              type="button"
              onClick={() => setShowConfig(!showConfig)}
              className="absolute -right-7 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity text-white/30 hover:text-[#39ff14] p-1.5 outline-none"
              title="Configure Status Bar"
              aria-label="Configure Status Bar"
            >
              <Settings
                className="w-3.5 h-3.5 hover:animate-[spin_4s_linear_infinite]"
                aria-hidden="true"
              />
            </button>

            {/* Config Popover */}
            <AnimatePresence>
              {showConfig && (
                <motion.div
                  initial={{ opacity: 0, y: 10, scale: 0.95 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: 10, scale: 0.95 }}
                  className="absolute top-full mt-3 left-1/2 -translate-x-1/2 w-48 bg-[#0a0a0a]/95 backdrop-blur-xl border border-white/10 rounded-xl p-3 shadow-2xl flex flex-col gap-2 z-[60]"
                >
                  <h3 className="text-[10px] uppercase tracking-widest text-white/40 mb-1 px-1">
                    Modules
                  </h3>
                  {Object.entries({
                    time: "Time & Date",
                    founder: "Founder Node",
                    resources: "System Resources",
                    channels: "Communication",
                    security: "Security & Proxy",
                    sync: "Node Sync",
                    ai: "AI Providers",
                    notifications: "Notifications",
                  }).map(([key, label]) => (
                    <button
                      type="button"
                      key={key}
                      onClick={() => toggleModule(key as keyof typeof modules)}
                      className="flex items-center justify-between px-2 py-1.5 hover:bg-white/5 rounded-lg transition-colors group/btn outline-none"
                    >
                      <span className="text-xs text-white/80 font-mono">
                        {label}
                      </span>
                      <div
                        className={`w-3 h-3 rounded-sm border flex items-center justify-center transition-colors ${modules[key as keyof typeof modules] ? "bg-[#39ff14]/20 border-[#39ff14]/50" : "border-white/20 group-hover/btn:border-white/40"}`}
                      >
                        {modules[key as keyof typeof modules] && (
                          <div className="w-1.5 h-1.5 bg-[#39ff14] rounded-[1px]" />
                        )}
                      </div>
                    </button>
                  ))}
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </motion.div>

        {/* Right Group */}
        <motion.div
          layout
          transition={spring}
          className={`flex gap-2 pointer-events-auto ${isModalOpen ? "absolute right-2 lg:right-4 top-1/2 -translate-y-1/2 flex-col items-end z-[60]" : "absolute right-4 md:right-6 top-6 flex-row items-start"}`}
        >
          {/* Active Node Indicator */}
          <motion.button
            layout
            transition={spring}
            type="button"
            onClick={() => {
              setInputRemoteUrl(getRemoteUrl());
              setTestResult(null);
              setShowNodeModal(true);
            }}
            className={`bg-[#0a0a0a]/80 backdrop-blur-md border ${
              nodeStatus === "connected"
                ? "border-emerald-500/30 hover:border-emerald-400/50"
                : "border-amber-500/50 hover:border-amber-400/80 bg-amber-500/10"
            } shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0 transition-colors cursor-pointer`}
            title={`Node: ${remoteUrl || "Local"} | Status: ${nodeStatus}${retryCount > 0 ? ` (Retries: ${retryCount})` : ""}`}
            aria-label="Node Connection Settings"
          >
            {nodeStatus === "connected" ? (
              <Server className="w-3 h-3 text-emerald-400" />
            ) : (
              <WifiOff className="w-3 h-3 text-amber-400 animate-pulse" />
            )}
            <span
              className={`font-mono text-[9px] uppercase tracking-wide hidden sm:inline-block ${
                nodeStatus === "connected"
                  ? "text-emerald-300"
                  : "text-amber-300 font-bold"
              }`}
            >
              {remoteUrl ? "Remote Node" : "Local Node"}
            </span>
            {nodeStatus !== "connected" && (
              <span className="font-mono text-[8px] bg-amber-500/20 text-amber-300 border border-amber-500/40 px-1 rounded animate-pulse">
                Retry #{retryCount}
              </span>
            )}
          </motion.button>

          {/* Maloca ops workspace */}
          <motion.button
            layout
            transition={spring}
            type="button"
            onClick={() => {
              window.location.hash = "#/maloca";
            }}
            className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0 hover:border-emerald-400/30 hover:bg-emerald-500/5 transition-colors"
            title="Abrir Maloca (ops workspace)"
          >
            <Home className="w-3 h-3 text-emerald-300/80" />
            <span className="font-mono text-[9px] text-emerald-200/80 uppercase tracking-wide hidden sm:inline-block">
              Maloca
            </span>
          </motion.button>

          {/* Security & Proxy */}
          {modules.security && (
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-2 h-7 shrink-0 lg:flex"
            >
              <div
                className="flex items-center gap-1"
                title="TPM HW Encryption Active"
              >
                <ShieldCheck className="w-3 h-3 text-emerald-400" />
              </div>
              <div className="w-px h-2.5 bg-white/20" />
              <div className="flex items-center gap-1" title="API Token Proxy">
                <Key className="w-3 h-3 text-yellow-500" />
              </div>
            </motion.div>
          )}

          {/* Node Sync Pill */}
          {modules.sync && (
            <div className="relative">
              <motion.button
                layout
                transition={spring}
                type="button"
                onClick={() => setShowSyncPopover((prev) => !prev)}
                className="bg-[#0a0a0a]/80 backdrop-blur-md border border-cyan-500/30 hover:border-cyan-400/60 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0 sm:flex cursor-pointer transition-colors"
                title="Cloud & Peer Memory Synchronization Status"
                aria-label="Memory Sync Control & Status"
              >
                <Wifi className={`w-3 h-3 text-cyan-400 ${isSyncing ? "animate-pulse text-cyan-300" : ""}`} />
                <span className="font-mono text-[9px] text-cyan-300 uppercase tracking-wide hidden md:inline-block">
                  Sync
                </span>
                <div className="w-8 h-0.5 bg-black/50 rounded-full overflow-hidden border border-white/5 mx-0.5 hidden xl:block">
                  <div className={`h-full bg-cyan-400 w-[98%] shadow-[0_0_6px_rgba(34,211,238,0.5)] ${isSyncing ? "animate-pulse" : ""}`} />
                </div>
                {isSyncing ? (
                  <RefreshCw className="w-2.5 h-2.5 text-cyan-300 animate-spin" aria-label="Syncing progress indicator" />
                ) : (
                  <span className="font-mono text-[9px] text-cyan-400 font-bold">
                    {syncLatencyMs !== null ? `${syncLatencyMs}ms` : "98%"}
                  </span>
                )}
              </motion.button>

              <AnimatePresence>
                {showSyncPopover && (
                  <motion.div
                    initial={{ opacity: 0, y: 10, scale: 0.95 }}
                    animate={{ opacity: 1, y: 0, scale: 1 }}
                    exit={{ opacity: 0, y: 10, scale: 0.95 }}
                    className="absolute right-0 top-full mt-2 w-60 bg-[#0a0a0a]/95 backdrop-blur-xl border border-cyan-500/30 rounded-xl p-3.5 shadow-2xl z-[80] space-y-3 font-mono"
                  >
                    <div className="flex items-center justify-between border-b border-white/10 pb-2">
                      <div className="flex items-center gap-1.5">
                        <Wifi className="w-3.5 h-3.5 text-cyan-400" />
                        <span className="text-xs font-bold text-white uppercase tracking-wider">
                          Memory Sync
                        </span>
                      </div>
                      <button
                        type="button"
                        onClick={() => setShowSyncPopover(false)}
                        className="text-white/40 hover:text-white transition-colors cursor-pointer"
                        aria-label="Close popover"
                      >
                        <X className="w-3.5 h-3.5" />
                      </button>
                    </div>

                    <div className="space-y-1.5 text-[10px] text-white/70">
                      <div className="flex justify-between">
                        <span>Sync Health:</span>
                        <span className="text-cyan-400 font-bold">Optimal (98%)</span>
                      </div>
                      <div className="flex justify-between">
                        <span>Peer Connection:</span>
                        <span className="text-emerald-400 font-bold">4 Active</span>
                      </div>
                      {syncLatencyMs !== null && (
                        <div className="flex justify-between">
                          <span>Last Sync Latency:</span>
                          <span className="text-cyan-300 font-bold">{syncLatencyMs}ms</span>
                        </div>
                      )}
                      {lastSyncTime && (
                        <div className="flex justify-between">
                          <span>Last Synced At:</span>
                          <span className="text-white/50">
                            {lastSyncTime.toLocaleTimeString(undefined, {
                              hour: "2-digit",
                              minute: "2-digit",
                              second: "2-digit",
                            })}
                          </span>
                        </div>
                      )}
                    </div>

                    <div className="pt-1">
                      <button
                        type="button"
                        onClick={handleTriggerSync}
                        disabled={isSyncing}
                        className="w-full py-1.5 px-3 bg-cyan-500/20 hover:bg-cyan-500/30 border border-cyan-500/40 text-cyan-300 rounded-lg text-xs font-bold transition-all flex items-center justify-center gap-2 disabled:opacity-50 cursor-pointer"
                        aria-label="Sync Now"
                      >
                        {isSyncing ? (
                          <>
                            <RefreshCw className="w-3.5 h-3.5 animate-spin text-cyan-300" />
                            <span>Syncing...</span>
                          </>
                        ) : (
                          <>
                            <RefreshCw className="w-3.5 h-3.5 text-cyan-300" />
                            <span>Sync Now</span>
                          </>
                        )}
                      </button>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </div>
          )}

          {/* AI Providers Pill */}
          {modules.ai && (config.has_gemini || config.has_openai) && (
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0"
            >
              <Bot className="w-3 h-3 text-purple-400" />
              <div className="flex items-center gap-1">
                {config.has_gemini && (
                  <span className="hidden md:inline-block text-[8px] bg-blue-500/15 text-blue-300 border border-blue-500/25 px-1 py-px rounded uppercase tracking-wider font-mono">
                    GEM
                  </span>
                )}
                {config.has_openai && (
                  <span className="hidden md:inline-block text-[8px] bg-emerald-500/15 text-emerald-300 border border-emerald-500/25 px-1 py-px rounded uppercase tracking-wider font-mono">
                    OAI
                  </span>
                )}
              </div>
            </motion.div>
          )}

          {/* Notifications Bell — opens dropdown */}
          {modules.notifications && (
            <div className="relative">
              <motion.button
                ref={bellRef}
                layout
                transition={spring}
                onClick={() => setShowNotifications((prev) => !prev)}
                className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2 hover:bg-white/5 hover:border-white/20 transition-all flex items-center justify-center h-7 w-7 shrink-0"
                title={`${memoryCount} Memories | ${unreadCount} Unread`}
              >
                <div className="relative flex items-center justify-center">
                  <Bell className="w-3.5 h-3.5 text-white/60" />
                  {unreadCount > 0 && (
                    <div className="absolute -top-1.5 -right-1.5 bg-red-500 text-white text-[7px] font-bold px-1 rounded-full border border-[#0a0a0a] min-w-[13px] text-center shadow-[0_0_5px_rgba(239,68,68,0.4)]">
                      {unreadCount > 9 ? "9+" : unreadCount}
                    </div>
                  )}
                </div>
              </motion.button>

              {/* Dropdown */}
              <AnimatePresence>
                {showNotifications && (
                  <NotificationsDropdown
                    onClose={() => setShowNotifications(false)}
                  />
                )}
              </AnimatePresence>
            </div>
          )}
        </motion.div>
      </div>

      {/* Node Connection Modal Overlay */}
      <AnimatePresence>
        {showNodeModal && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="fixed inset-0 z-[100] bg-black/70 backdrop-blur-md flex items-center justify-center p-4 pointer-events-auto"
            onClick={() => setShowNodeModal(false)}
          >
            <motion.div
              initial={{ scale: 0.95, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              exit={{ scale: 0.95, opacity: 0 }}
              className="bg-[#0a0a0a] border border-white/10 rounded-2xl p-6 w-full max-w-md shadow-2xl space-y-5"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="flex items-center justify-between border-b border-white/10 pb-4">
                <div className="flex items-center gap-2">
                  <Server className="w-5 h-5 text-emerald-400" />
                  <div>
                    <h3 className="text-sm font-bold text-white uppercase tracking-wider font-mono">
                      Xavier Node Connectivity
                    </h3>
                    <p className="text-[10px] text-white/50">
                      Configure active remote node URL & auto-reconnect settings
                    </p>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => setShowNodeModal(false)}
                  className="text-white/40 hover:text-white transition-colors"
                  aria-label="Close modal"
                >
                  <X className="w-4 h-4" />
                </button>
              </div>

              <div className="space-y-4">
                {/* Active Mode Banner */}
                <div className="bg-white/5 border border-white/10 p-3 rounded-xl flex items-center justify-between text-xs font-mono">
                  <span className="text-white/60">Current Base URL:</span>
                  <span
                    className="text-emerald-400 font-bold truncate max-w-[200px]"
                    title={getApiUrl("")}
                  >
                    {getApiUrl("") || "Relative / Default"}
                  </span>
                </div>

                {/* Status Indicator */}
                <div className="flex items-center gap-2 text-xs font-mono">
                  <span className="text-white/60">Status:</span>
                  <span
                    className={`px-2 py-0.5 rounded-full text-[10px] font-bold uppercase border ${
                      nodeStatus === "connected"
                        ? "bg-emerald-500/10 text-emerald-400 border-emerald-500/30"
                        : "bg-amber-500/10 text-amber-400 border-amber-500/30 animate-pulse"
                    }`}
                  >
                    {nodeStatus === "connected"
                      ? "Connected"
                      : `Disconnected (${retryCount} retries)`}
                  </span>
                </div>

                {/* Remote URL Input */}
                <div className="space-y-1.5">
                  <label
                    htmlFor="remote-node-url-input"
                    className="block text-xs font-mono text-white/70"
                  >
                    Remote Node Endpoint (e.g., https://xavier-node.domain.com:8006)
                  </label>
                  <div className="relative">
                    <Globe className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-white/40" />
                    <input
                      id="remote-node-url-input"
                      type="text"
                      value={inputRemoteUrl}
                      onChange={(e) => setInputRemoteUrl(e.target.value)}
                      placeholder="https://node.swal.local:8006"
                      className="w-full pl-9 pr-3 py-2 bg-black/50 border border-white/15 rounded-xl text-white text-xs font-mono focus:outline-none focus:border-emerald-400 transition-colors"
                    />
                  </div>
                </div>

                {testResult && (
                  <div
                    className={`p-3 rounded-xl border text-xs font-mono ${
                      testResult.ok
                        ? "bg-emerald-500/10 border-emerald-500/30 text-emerald-300"
                        : "bg-red-500/10 border-red-500/30 text-red-300"
                    }`}
                  >
                    {testResult.msg}
                  </div>
                )}

                {/* Button Controls */}
                <div className="flex items-center justify-between gap-2 pt-2 border-t border-white/10">
                  <button
                    type="button"
                    onClick={handleTestConnection}
                    disabled={testingConnection}
                    className="px-3 py-1.5 bg-white/5 hover:bg-white/10 border border-white/10 text-white/80 rounded-xl text-xs font-mono font-bold transition-colors flex items-center gap-1.5 disabled:opacity-50"
                  >
                    {testingConnection && (
                      <RefreshCw className="w-3 h-3 animate-spin" />
                    )}
                    Test
                  </button>
                  <div className="flex items-center gap-2">
                    {remoteUrl && (
                      <button
                        type="button"
                        onClick={() => handleSaveRemoteUrl(null)}
                        className="px-3 py-1.5 bg-red-500/10 hover:bg-red-500/20 border border-red-500/30 text-red-300 rounded-xl text-xs font-mono font-bold transition-colors"
                      >
                        Use Local
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={() => handleSaveRemoteUrl(inputRemoteUrl)}
                      className="px-4 py-1.5 bg-emerald-500 text-black hover:bg-emerald-400 rounded-xl text-xs font-mono font-bold transition-colors"
                    >
                      Save Remote Node
                    </button>
                  </div>
                </div>
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Messaging Config Modal — full screen overlay */}
      <AnimatePresence>
        {showMessaging && (
          <MessagingConfigModal
            initialTab={messagingTab}
            onClose={() => setShowMessaging(false)}
          />
        )}
      </AnimatePresence>
    </>
  );
});
