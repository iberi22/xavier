import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  Bell,
  Bot,
  Database,
  Hash,
  Key,
  MessageCircle,
  MessageSquare,
  Send,
  Settings,
  ShieldCheck,
  Users,
  Wifi,
  Zap,
  Home,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { getApiUrl } from "../api/client";
import MessagingConfigModal from "./MessagingConfigModal";
import NotificationsDropdown from "./NotificationsDropdown";
import OperationModeBadge from "./OperationModeBadge";

type MessagingPlatform =
  | "telegram"
  | "discord"
  | "slack"
  | "teams"
  | "whatsapp";

interface TopStatusBarProps {
  isModalOpen?: boolean;
}

// Declare the vite define constant
declare const __APP_VERSION__: string;

async function getAuthToken(): Promise<string> {
  try {
    return await invoke<string>("get_xavier_token");
  } catch {
    return localStorage.getItem("XAVIER_TOKEN") || "";
  }
}

export default function TopStatusBar({
  isModalOpen = false,
}: TopStatusBarProps) {
  const [time, setTime] = useState(new Date());
  const [memoryCount, setMemoryCount] = useState(0);
  const [unreadCount, setUnreadCount] = useState(0);
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
  const [showMessaging, setShowMessaging] = useState(false);
  const [messagingTab, setMessagingTab] =
    useState<MessagingPlatform>("telegram");
  const [showNotifications, setShowNotifications] = useState(false);
  const bellRef = useRef<HTMLButtonElement>(null);
  const [modules, setModules] = useState({
    time: true,
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
    invoke("get_current_config_state")
      .then((res: any) => setConfig(res))
      .catch(console.error);

    const fetchMetrics = async () => {
      // 1. Fetch realtime metrics from Tauri
      try {
        const met = await invoke("get_realtime_metrics");
        setMetrics(met as any);
      } catch (err) {
        console.debug("Error fetching realtime metrics:", err);
      }

      // 2. Fetch memory count from REST API
      try {
        const token = await getAuthToken();
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
        const token = await getAuthToken();
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
      }
    };

    fetchMetrics();
    const metricsInterval = setInterval(fetchMetrics, 3000);
    const timeInterval = setInterval(() => setTime(new Date()), 1000);

    // Listen for real-time notifications via Tauri
    const unlistenPromise = listen<any>("new-notification", () => {
      fetchMetrics();
    });

    return () => {
      clearInterval(metricsInterval);
      clearInterval(timeInterval);
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const spring = { type: "spring" as const, stiffness: 200, damping: 25 };

  // Open messaging modal on a specific platform icon click
  const openMessaging = (platform: MessagingPlatform) => {
    setMessagingTab(platform);
    setShowMessaging(true);
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

          <OperationModeBadge />

          {/* System Resources Pill */}
          {modules.resources && (
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-3 py-1 flex items-center gap-3 h-7 shrink-0 hidden lg:flex"
            >
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
                onClick={() => openMessaging("discord")}
                title="Discord — Click to configure"
                aria-label="Configure Discord"
                className="relative group p-0.5 rounded-full hover:bg-indigo-500/10 transition-colors"
              >
                <MessageCircle className="w-3 h-3 text-indigo-400/40 group-hover:text-indigo-400 transition-colors" aria-hidden="true" />
              </button>

              {/* Slack */}
              <button
                onClick={() => openMessaging("slack")}
                title="Slack — Click to configure"
                aria-label="Configure Slack"
                className="relative group p-0.5 rounded-full hover:bg-amber-500/10 transition-colors"
              >
                <Hash className="w-3 h-3 text-amber-400/40 group-hover:text-amber-400 transition-colors" aria-hidden="true" />
              </button>

              {/* Teams */}
              <button
                onClick={() => openMessaging("teams")}
                title="MS Teams — Click to configure"
                aria-label="Configure MS Teams"
                className="relative group p-0.5 rounded-full hover:bg-purple-500/10 transition-colors"
              >
                <Users className="w-3 h-3 text-purple-400/40 group-hover:text-purple-400 transition-colors" aria-hidden="true" />
              </button>

              {/* Telegram — may be configured */}
              <button
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
                  <div className="absolute -top-0.5 -right-0.5 w-1.5 h-1.5 rounded-full bg-blue-400 msg-active-dot shadow-[0_0_4px_rgba(96,165,250,0.6)]" aria-hidden="true" />
                )}
              </button>

              {/* WhatsApp */}
              <button
                onClick={() => openMessaging("whatsapp")}
                title="WhatsApp — Click to configure"
                aria-label="Configure WhatsApp"
                className="relative group p-0.5 rounded-full hover:bg-green-500/10 transition-colors"
              >
                <MessageSquare className="w-3 h-3 text-green-400/40 group-hover:text-green-400 transition-colors" aria-hidden="true" />
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
              onClick={() => setShowConfig(!showConfig)}
              className="absolute -right-7 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity text-white/30 hover:text-[#39ff14] p-1.5 outline-none"
              title="Configure Status Bar"
              aria-label="Configure Status Bar"
            >
              <Settings className="w-3.5 h-3.5 hover:animate-[spin_4s_linear_infinite]" aria-hidden="true" />
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
                    resources: "System Resources",
                    channels: "Communication",
                    security: "Security & Proxy",
                    sync: "Node Sync",
                    ai: "AI Providers",
                    notifications: "Notifications",
                  }).map(([key, label]) => (
                    <button
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
            <motion.div
              layout
              transition={spring}
              className="bg-[#0a0a0a]/80 backdrop-blur-md border border-white/10 shadow-lg rounded-full px-2.5 py-1 flex items-center gap-1.5 h-7 shrink-0 sm:flex"
            >
              <Wifi className="w-3 h-3 text-cyan-400" />
              <span className="font-mono text-[9px] text-cyan-400 uppercase tracking-wide hidden md:inline-block">
                4
              </span>
              <div className="w-8 h-0.5 bg-black/50 rounded-full overflow-hidden border border-white/5 mx-0.5 hidden xl:block">
                <div className="h-full bg-cyan-400 w-[98%] shadow-[0_0_6px_rgba(34,211,238,0.5)]" />
              </div>
              <span className="font-mono text-[9px] text-cyan-400 font-bold">
                98%
              </span>
            </motion.div>
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
}
