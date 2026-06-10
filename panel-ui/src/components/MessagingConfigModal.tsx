import {
  AlertTriangle,
  Bell,
  CheckCircle2,
  ChevronRight,
  Copy,
  Eye,
  EyeOff,
  Hash,
  Loader2,
  MessageCircle,
  MessageSquare,
  RefreshCw,
  Send,
  Shield,
  Users,
  X,
  XCircle,
  Zap,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";

interface MessagingConfigModalProps {
  onClose: () => void;
  initialTab?: MessagingPlatform;
}

type MessagingPlatform =
  | "telegram"
  | "discord"
  | "slack"
  | "teams"
  | "whatsapp";
type ConnectionStatus = "connected" | "disconnected" | "testing" | "error";

interface PlatformConfig {
  id: MessagingPlatform;
  label: string;
  icon: React.ReactNode;
  color: string;
  accentColor: string;
  description: string;
  fields: FieldDef[];
}

interface FieldDef {
  key: string;
  label: string;
  type: "text" | "password" | "url" | "number";
  placeholder: string;
  hint?: string;
}

// ── Mock data (replace with real state/backend later) ──────────────────────
const MOCK_STATUS: Record<MessagingPlatform, ConnectionStatus> = {
  telegram: "disconnected",
  discord: "disconnected",
  slack: "disconnected",
  teams: "disconnected",
  whatsapp: "disconnected",
};

const MOCK_PERMISSIONS: Record<string, boolean> = {
  receive_messages: true,
  send_messages: true,
  send_memory_updates: false,
  send_agent_alerts: true,
  allow_commands: false,
};

// ── Platform definitions ───────────────────────────────────────────────────
const PLATFORMS: PlatformConfig[] = [
  {
    id: "telegram",
    label: "Telegram",
    color: "text-blue-400",
    accentColor: "border-blue-500/30 bg-blue-500/5",
    icon: <Send className="w-4 h-4" />,
    description:
      "Connect Xavier to a Telegram bot for bidirectional messaging and commands.",
    fields: [
      {
        key: "bot_token",
        label: "Bot Token",
        type: "password",
        placeholder: "110201543:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw",
        hint: "Get from @BotFather on Telegram",
      },
      {
        key: "chat_id",
        label: "Chat ID",
        type: "text",
        placeholder: "-1001234567890",
        hint: "Your personal or group Chat ID",
      },
      {
        key: "webhook_url",
        label: "Webhook URL (optional)",
        type: "url",
        placeholder: "https://your-domain.com/webhook/telegram",
      },
    ],
  },
  {
    id: "discord",
    label: "Discord",
    color: "text-indigo-400",
    accentColor: "border-indigo-500/30 bg-indigo-500/5",
    icon: <MessageCircle className="w-4 h-4" />,
    description:
      "Send Xavier notifications and memory updates to a Discord channel via webhook.",
    fields: [
      {
        key: "webhook_url",
        label: "Webhook URL",
        type: "url",
        placeholder: "https://discord.com/api/webhooks/...",
        hint: "Create in Discord server settings → Integrations",
      },
      {
        key: "bot_token",
        label: "Bot Token (optional)",
        type: "password",
        placeholder: "MTE...",
        hint: "Required only for receiving commands",
      },
      {
        key: "channel_id",
        label: "Channel ID",
        type: "text",
        placeholder: "1234567890",
        hint: "Right-click channel → Copy Channel ID",
      },
    ],
  },
  {
    id: "slack",
    label: "Slack",
    color: "text-amber-400",
    accentColor: "border-amber-500/30 bg-amber-500/5",
    icon: <Hash className="w-4 h-4" />,
    description:
      "Post Xavier memory summaries and agent activity directly to Slack channels.",
    fields: [
      {
        key: "bot_token",
        label: "Bot OAuth Token",
        type: "password",
        placeholder: "xoxb-...",
        hint: "From your Slack App settings → OAuth & Permissions",
      },
      {
        key: "channel",
        label: "Channel",
        type: "text",
        placeholder: "#xavier-updates",
        hint: "Channel name or ID",
      },
      {
        key: "signing_secret",
        label: "Signing Secret",
        type: "password",
        placeholder: "abc123...",
        hint: "From Slack App → Basic Information",
      },
    ],
  },
  {
    id: "teams",
    label: "MS Teams",
    color: "text-purple-400",
    accentColor: "border-purple-500/30 bg-purple-500/5",
    icon: <Users className="w-4 h-4" />,
    description:
      "Integrate Xavier with Microsoft Teams for enterprise notification delivery.",
    fields: [
      {
        key: "webhook_url",
        label: "Incoming Webhook URL",
        type: "url",
        placeholder: "https://outlook.office.com/webhook/...",
        hint: "Create via Teams channel connectors",
      },
      {
        key: "tenant_id",
        label: "Tenant ID (optional)",
        type: "text",
        placeholder: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
      },
    ],
  },
  {
    id: "whatsapp",
    label: "WhatsApp",
    color: "text-green-400",
    accentColor: "border-green-500/30 bg-green-500/5",
    icon: <MessageSquare className="w-4 h-4" />,
    description:
      "Send Xavier updates via WhatsApp Business API (requires Meta developer account).",
    fields: [
      {
        key: "access_token",
        label: "Permanent Access Token",
        type: "password",
        placeholder: "EAAxxxxxx...",
        hint: "From Meta for Developers → WhatsApp → API Setup",
      },
      {
        key: "phone_number_id",
        label: "Phone Number ID",
        type: "text",
        placeholder: "1234567890",
        hint: "From Meta Business Suite",
      },
      {
        key: "recipient_phone",
        label: "Recipient Phone",
        type: "text",
        placeholder: "+1234567890",
        hint: "Include country code",
      },
    ],
  },
];

// ── Sub-components ─────────────────────────────────────────────────────────
function StatusDot({ status }: { status: ConnectionStatus }) {
  const cfg = {
    connected: {
      color: "bg-green-400",
      glow: "shadow-[0_0_6px_rgba(74,222,128,0.6)]",
      label: "Connected",
    },
    disconnected: { color: "bg-white/20", glow: "", label: "Not connected" },
    testing: {
      color: "bg-yellow-400 animate-pulse",
      glow: "shadow-[0_0_6px_rgba(250,204,21,0.5)]",
      label: "Testing...",
    },
    error: {
      color: "bg-red-400",
      glow: "shadow-[0_0_6px_rgba(248,113,113,0.5)]",
      label: "Error",
    },
  }[status];
  return (
    <div className="flex items-center gap-1.5">
      <div className={`w-1.5 h-1.5 rounded-full ${cfg.color} ${cfg.glow}`} />
      <span className="text-[10px] font-mono text-white/40 uppercase tracking-widest">
        {cfg.label}
      </span>
    </div>
  );
}

function PermissionsPanel() {
  const [perms, setPerms] = useState(MOCK_PERMISSIONS);
  const labels: Record<string, { label: string; desc: string }> = {
    receive_messages: {
      label: "Receive Messages",
      desc: "Xavier reads incoming messages from this channel",
    },
    send_messages: {
      label: "Send Messages",
      desc: "Xavier can send messages to this channel",
    },
    send_memory_updates: {
      label: "Memory Update Notifications",
      desc: "Notify when new memories are indexed",
    },
    send_agent_alerts: {
      label: "Agent Activity Alerts",
      desc: "Notify when agents complete tasks or encounter errors",
    },
    allow_commands: {
      label: "Allow Commands",
      desc: "Users can send commands to Xavier via this channel",
    },
  };
  return (
    <div className="space-y-2">
      <h4 className="text-[10px] uppercase tracking-widest text-white/30 mb-3">
        Channel Permissions
      </h4>
      {Object.entries(perms).map(([key, val]) => (
        <div
          key={key}
          className="flex items-center justify-between p-2.5 rounded-lg bg-white/[0.02] border border-white/[0.05] hover:border-white/10 transition-colors"
        >
          <div>
            <p className="text-xs text-white/70">{labels[key]?.label}</p>
            <p className="text-[10px] text-white/30 mt-0.5">
              {labels[key]?.desc}
            </p>
          </div>
          <button
            onClick={() => setPerms((p) => ({ ...p, [key]: !p[key] }))}
            className={`relative w-9 h-5 rounded-full transition-all duration-300 flex-shrink-0 ml-3 ${val ? "bg-[#39ff14]/20 border border-[#39ff14]/30" : "bg-white/5 border border-white/10"}`}
          >
            <div
              className={`absolute top-0.5 w-4 h-4 rounded-full transition-all duration-300 ${val ? "left-[calc(100%-18px)] bg-[#39ff14]" : "left-0.5 bg-white/30"}`}
            />
          </button>
        </div>
      ))}
    </div>
  );
}

function PlatformForm({ platform }: { platform: PlatformConfig }) {
  const [values, setValues] = useState<Record<string, string>>({});
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});
  const [status, setStatus] = useState<ConnectionStatus>(
    MOCK_STATUS[platform.id],
  );
  const [activeSection, setActiveSection] = useState<
    "credentials" | "permissions" | "advanced"
  >("credentials");

  const handleTest = () => {
    setStatus("testing");
    setTimeout(() => setStatus("error"), 2000); // Mock: always error until backend
  };

  const handleCopy = (val: string) => {
    navigator.clipboard.writeText(val).catch(() => {});
  };

  return (
    <div className="flex flex-col h-full">
      {/* Status bar */}
      <div
        className={`flex items-center justify-between px-5 py-3 rounded-xl border mb-4 ${platform.accentColor}`}
      >
        <div className="flex items-center gap-3">
          <div className={platform.color}>{platform.icon}</div>
          <div>
            <p className="text-sm font-semibold text-white/90">
              {platform.label}
            </p>
            <p className="text-[10px] text-white/40">{platform.description}</p>
          </div>
        </div>
        <StatusDot status={status} />
      </div>

      {/* Section tabs */}
      <div className="flex gap-1 mb-4 bg-black/20 rounded-lg p-1">
        {(["credentials", "permissions", "advanced"] as const).map((s) => (
          <button
            key={s}
            onClick={() => setActiveSection(s)}
            className={`flex-1 py-1.5 text-[10px] uppercase tracking-widest rounded-md transition-all duration-200 ${
              activeSection === s
                ? "bg-white/10 text-white/90"
                : "text-white/30 hover:text-white/60"
            }`}
          >
            {s === "credentials"
              ? "🔑 Credentials"
              : s === "permissions"
                ? "🛡 Permissions"
                : "⚙ Advanced"}
          </button>
        ))}
      </div>

      <div className="flex-1 overflow-y-auto">
        <AnimatePresence mode="wait">
          {activeSection === "credentials" && (
            <motion.div
              key="creds"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="space-y-4"
            >
              {platform.fields.map((field) => (
                <div key={field.key}>
                  <label className="text-[10px] uppercase tracking-widest text-white/40 mb-1.5 block">
                    {field.label}
                  </label>
                  <div className="relative">
                    <input
                      type={
                        field.type === "password" && !revealed[field.key]
                          ? "password"
                          : "text"
                      }
                      value={values[field.key] || ""}
                      onChange={(e) =>
                        setValues((v) => ({
                          ...v,
                          [field.key]: e.target.value,
                        }))
                      }
                      placeholder={field.placeholder}
                      className="w-full bg-black/30 border border-white/10 focus:border-[#39ff14]/30 focus:shadow-[0_0_10px_rgba(57,255,20,0.08)] text-white/80 text-xs px-3 py-2.5 rounded-lg outline-none transition-all font-mono pr-16"
                    />
                    <div className="absolute right-2 top-1/2 -translate-y-1/2 flex gap-1">
                      {field.type === "password" && (
                        <button
                          onClick={() =>
                            setRevealed((r) => ({
                              ...r,
                              [field.key]: !r[field.key],
                            }))
                          }
                          className="p-1 text-white/30 hover:text-white/60 transition-colors"
                        >
                          {revealed[field.key] ? (
                            <EyeOff className="w-3.5 h-3.5" />
                          ) : (
                            <Eye className="w-3.5 h-3.5" />
                          )}
                        </button>
                      )}
                      {values[field.key] && (
                        <button
                          onClick={() => handleCopy(values[field.key])}
                          className="p-1 text-white/30 hover:text-white/60 transition-colors"
                        >
                          <Copy className="w-3.5 h-3.5" />
                        </button>
                      )}
                    </div>
                  </div>
                  {field.hint && (
                    <p className="text-[10px] text-white/25 mt-1">
                      {field.hint}
                    </p>
                  )}
                </div>
              ))}

              {/* Actions */}
              <div className="flex gap-2 pt-2">
                <button
                  onClick={handleTest}
                  disabled={status === "testing"}
                  className="flex items-center gap-2 px-4 py-2 bg-white/5 border border-white/10 rounded-lg text-xs text-white/60 hover:text-white/80 hover:border-white/20 transition-all disabled:opacity-40"
                >
                  {status === "testing" ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <RefreshCw className="w-3.5 h-3.5" />
                  )}
                  Test Connection
                </button>
                <button className="flex-1 py-2 bg-[#39ff14]/10 border border-[#39ff14]/20 rounded-lg text-xs text-[#39ff14] hover:bg-[#39ff14]/15 hover:border-[#39ff14]/30 transition-all font-medium tracking-wide">
                  Save Configuration
                </button>
              </div>

              {status === "error" && (
                <motion.div
                  initial={{ opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                  className="flex items-center gap-2 p-3 rounded-lg bg-red-500/5 border border-red-500/20 text-red-400 text-xs"
                >
                  <AlertTriangle className="w-3.5 h-3.5 flex-shrink-0" />
                  <span>
                    Connection failed. Backend integration not yet implemented —
                    see GitHub issue #backend-messaging.
                  </span>
                </motion.div>
              )}
            </motion.div>
          )}

          {activeSection === "permissions" && (
            <motion.div
              key="perms"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
            >
              <PermissionsPanel />
            </motion.div>
          )}

          {activeSection === "advanced" && (
            <motion.div
              key="adv"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="space-y-4"
            >
              <h4 className="text-[10px] uppercase tracking-widest text-white/30 mb-3">
                Advanced Settings
              </h4>
              <div>
                <label className="text-[10px] uppercase tracking-widest text-white/40 mb-1.5 block">
                  Rate Limit (msg/min)
                </label>
                <input
                  type="number"
                  defaultValue={30}
                  className="w-full bg-black/30 border border-white/10 text-white/80 text-xs px-3 py-2.5 rounded-lg outline-none font-mono"
                />
              </div>
              <div>
                <label className="text-[10px] uppercase tracking-widest text-white/40 mb-1.5 block">
                  Message Prefix
                </label>
                <input
                  type="text"
                  defaultValue="[Xavier]"
                  className="w-full bg-black/30 border border-white/10 text-white/80 text-xs px-3 py-2.5 rounded-lg outline-none font-mono"
                />
              </div>
              <div>
                <label className="text-[10px] uppercase tracking-widest text-white/40 mb-1.5 block">
                  Retry Attempts
                </label>
                <input
                  type="number"
                  defaultValue={3}
                  className="w-full bg-black/30 border border-white/10 text-white/80 text-xs px-3 py-2.5 rounded-lg outline-none font-mono"
                />
              </div>
              <div className="p-3 rounded-lg bg-amber-500/5 border border-amber-500/15 flex items-start gap-2">
                <AlertTriangle className="w-3.5 h-3.5 text-amber-400 flex-shrink-0 mt-0.5" />
                <p className="text-[10px] text-amber-400/70">
                  Backend logic not implemented. These settings are saved
                  locally as mock config only. See GitHub issue tracker.
                </p>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

// ── Main Modal ─────────────────────────────────────────────────────────────
export default function MessagingConfigModal({
  onClose,
  initialTab = "telegram",
}: MessagingConfigModalProps) {
  const [activeTab, setActiveTab] = useState<MessagingPlatform>(initialTab);
  const activePlatform = PLATFORMS.find((p) => p.id === activeTab)!;

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="absolute inset-0 z-[70] flex items-center justify-center bg-black/70 backdrop-blur-sm"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <motion.div
        initial={{ opacity: 0, scale: 0.96, y: 12 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.96, y: 12 }}
        transition={{ duration: 0.3, ease: [0.22, 1, 0.36, 1] }}
        className="relative w-[780px] max-w-[95vw] h-[560px] max-h-[90vh] bg-[#060606] border border-white/[0.06] rounded-2xl flex flex-col shadow-2xl overflow-hidden"
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-white/[0.05] bg-black/20">
          <div className="flex items-center gap-3">
            <div className="w-7 h-7 rounded-lg bg-white/5 border border-white/10 flex items-center justify-center">
              <Zap className="w-3.5 h-3.5 text-[#39ff14]/70" />
            </div>
            <div>
              <h2 className="text-sm font-semibold text-white/90 tracking-wide">
                Messaging Integrations
              </h2>
              <p className="text-[10px] text-white/30">
                Connect Xavier to external communication channels
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg text-white/30 hover:text-white/70 hover:bg-white/5 transition-all"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="flex flex-1 overflow-hidden">
          {/* Sidebar — Platform tabs */}
          <div className="w-44 border-r border-white/[0.05] bg-black/10 p-3 flex flex-col gap-1">
            {PLATFORMS.map((p) => {
              const isActive = p.id === activeTab;
              const mockConnected = MOCK_STATUS[p.id] === "connected";
              return (
                <button
                  key={p.id}
                  onClick={() => setActiveTab(p.id)}
                  className={`flex items-center gap-2.5 px-3 py-2.5 rounded-lg text-left transition-all duration-200 group relative ${
                    isActive
                      ? "bg-white/[0.06] border border-white/[0.08]"
                      : "hover:bg-white/[0.03] border border-transparent"
                  }`}
                >
                  <span
                    className={`${isActive ? p.color : "text-white/30 group-hover:text-white/50"} transition-colors`}
                  >
                    {p.icon}
                  </span>
                  <span
                    className={`text-xs font-medium ${isActive ? "text-white/90" : "text-white/40 group-hover:text-white/70"} transition-colors`}
                  >
                    {p.label}
                  </span>
                  {mockConnected && (
                    <div className="absolute right-2.5 top-1/2 -translate-y-1/2 w-1.5 h-1.5 rounded-full bg-green-400 shadow-[0_0_4px_rgba(74,222,128,0.5)]" />
                  )}
                  {isActive && (
                    <ChevronRight className="absolute right-2 top-1/2 -translate-y-1/2 w-3 h-3 text-white/20" />
                  )}
                </button>
              );
            })}

            {/* Info footer */}
            <div className="mt-auto pt-3 border-t border-white/[0.05]">
              <div className="flex items-center gap-1.5 text-[9px] text-white/20">
                <Shield className="w-2.5 h-2.5" />
                <span>Tokens encrypted locally</span>
              </div>
              <div className="flex items-center gap-1.5 text-[9px] text-white/20 mt-1">
                <Bell className="w-2.5 h-2.5" />
                <span>Mocks — backend pending</span>
              </div>
            </div>
          </div>

          {/* Main content */}
          <div className="flex-1 p-5 overflow-y-auto">
            <AnimatePresence mode="wait">
              <motion.div
                key={activeTab}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="h-full"
              >
                <PlatformForm platform={activePlatform} />
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
      </motion.div>
    </motion.div>
  );
} // ── Inline/embedded version (no backdrop, no modal chrome) ─────────────────
export function MessagingConfigInner({
  initialTab = "telegram",
}: {
  initialTab?: MessagingPlatform;
}) {
  const [activeTab, setActiveTab] = useState<MessagingPlatform>(initialTab);
  const activePlatform = PLATFORMS.find((p) => p.id === activeTab)!;

  return (
    <div className="flex h-full overflow-hidden">
      {/* Sidebar */}
      <div className="w-44 border-r border-white/[0.05] bg-black/10 p-3 flex flex-col gap-1">
        {PLATFORMS.map((p) => {
          const isActive = p.id === activeTab;
          const mockConnected = MOCK_STATUS[p.id] === "connected";
          return (
            <button
              key={p.id}
              onClick={() => setActiveTab(p.id)}
              className={`flex items-center gap-2.5 px-3 py-2.5 rounded-lg text-left transition-all duration-200 group relative ${
                isActive
                  ? "bg-white/[0.06] border border-white/[0.08]"
                  : "hover:bg-white/[0.03] border border-transparent"
              }`}
            >
              <span
                className={`${isActive ? p.color : "text-white/30 group-hover:text-white/50"} transition-colors`}
              >
                {p.icon}
              </span>
              <span
                className={`text-xs font-medium ${isActive ? "text-white/90" : "text-white/40 group-hover:text-white/70"} transition-colors`}
              >
                {p.label}
              </span>
              {mockConnected && (
                <div className="absolute right-2.5 top-1/2 -translate-y-1/2 w-1.5 h-1.5 rounded-full bg-green-400 shadow-[0_0_4px_rgba(74,222,128,0.5)]" />
              )}
              {isActive && (
                <ChevronRight className="absolute right-2 top-1/2 -translate-y-1/2 w-3 h-3 text-white/20" />
              )}
            </button>
          );
        })}
        <div className="mt-auto pt-3 border-t border-white/[0.05]">
          <div className="flex items-center gap-1.5 text-[9px] text-white/20">
            <Shield className="w-2.5 h-2.5" />
            <span>Tokens encrypted locally</span>
          </div>
          <div className="flex items-center gap-1.5 text-[9px] text-white/20 mt-1">
            <Bell className="w-2.5 h-2.5" />
            <span>Mocks — backend pending</span>
          </div>
        </div>
      </div>
      {/* Content */}
      <div className="flex-1 p-5 overflow-y-auto">
        <AnimatePresence mode="wait">
          <motion.div
            key={activeTab}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="h-full"
          >
            <PlatformForm platform={activePlatform} />
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}
