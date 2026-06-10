import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  Clock,
  Copy,
  Eye,
  EyeOff,
  FileText,
  Fingerprint,
  Globe,
  Key,
  Lock,
  Plus,
  RefreshCw,
  Shield,
  Trash2,
  Unlock,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";

// ── Mock Data ──────────────────────────────────────────────────────────────

interface TokenEntry {
  id: string;
  label: string;
  prefix: string;
  created: Date;
  lastUsed: Date | null;
  expiresAt: Date | null;
  scopes: string[];
  active: boolean;
}

const MOCK_TOKENS: TokenEntry[] = [
  {
    id: "t1",
    label: "Desktop App (local)",
    prefix: "xav_loc_****",
    active: true,
    created: new Date(Date.now() - 30 * 86400000),
    lastUsed: new Date(Date.now() - 60000),
    expiresAt: null,
    scopes: ["read", "write", "admin"],
  },
  {
    id: "t2",
    label: "CI Pipeline",
    prefix: "xav_ci_*****",
    active: true,
    created: new Date(Date.now() - 15 * 86400000),
    lastUsed: new Date(Date.now() - 4 * 3600000),
    expiresAt: new Date(Date.now() + 60 * 86400000),
    scopes: ["read"],
  },
  {
    id: "t3",
    label: "Jules Agent",
    prefix: "xav_jul_****",
    active: false,
    created: new Date(Date.now() - 60 * 86400000),
    lastUsed: new Date(Date.now() - 7 * 86400000),
    expiresAt: new Date(Date.now() - 1 * 86400000),
    scopes: ["read", "write"],
  },
];

interface ApiKey {
  id: string;
  provider: string;
  prefix: string;
  active: boolean;
  lastValidated: Date | null;
}

const MOCK_API_KEYS: ApiKey[] = [
  {
    id: "k1",
    provider: "OpenAI",
    prefix: "sk-proj-****",
    active: true,
    lastValidated: new Date(Date.now() - 3600000),
  },
  {
    id: "k2",
    provider: "Gemini",
    prefix: "AIza****",
    active: true,
    lastValidated: new Date(Date.now() - 7200000),
  },
  {
    id: "k3",
    provider: "Anthropic",
    prefix: "sk-ant-****",
    active: false,
    lastValidated: null,
  },
];

interface AuditEntry {
  id: string;
  event: string;
  source: string;
  timestamp: Date;
  level: "info" | "warning" | "error";
}

const MOCK_AUDIT: AuditEntry[] = [
  {
    id: "a1",
    event: "Token authenticated",
    source: "panel-ui desktop",
    timestamp: new Date(Date.now() - 2 * 60000),
    level: "info",
  },
  {
    id: "a2",
    event: "Memory indexed (247 items)",
    source: "xavier-core",
    timestamp: new Date(Date.now() - 8 * 60000),
    level: "info",
  },
  {
    id: "a3",
    event: "Failed auth attempt (3x)",
    source: "127.0.0.1:52881",
    timestamp: new Date(Date.now() - 25 * 60000),
    level: "warning",
  },
  {
    id: "a4",
    event: "Config updated: embedding model",
    source: "panel-ui desktop",
    timestamp: new Date(Date.now() - 42 * 60000),
    level: "info",
  },
  {
    id: "a5",
    event: "Telegram bot error",
    source: "messaging-gateway",
    timestamp: new Date(Date.now() - 5 * 60000),
    level: "error",
  },
  {
    id: "a6",
    event: "API key validated (Gemini)",
    source: "xavier-core",
    timestamp: new Date(Date.now() - 120 * 60000),
    level: "info",
  },
];

// ── Helpers ────────────────────────────────────────────────────────────────

function formatRelative(date: Date | null): string {
  if (!date) return "Never";
  const diff = (Date.now() - date.getTime()) / 1000;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 0) return `in ${Math.floor(-diff / 86400)}d`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function ScopeChip({ scope }: { scope: string }) {
  const colors: Record<string, string> = {
    read: "bg-blue-500/10 text-blue-400 border-blue-500/20",
    write: "bg-amber-500/10 text-amber-400 border-amber-500/20",
    admin: "bg-red-500/10 text-red-400 border-red-500/20",
  };
  return (
    <span
      className={`px-1.5 py-0.5 rounded text-[9px] font-mono border uppercase tracking-wider ${colors[scope] || "bg-white/5 text-white/40 border-white/10"}`}
    >
      {scope}
    </span>
  );
}

// ── Sections ───────────────────────────────────────────────────────────────

function TokensSection() {
  const [tokens, setTokens] = useState<TokenEntry[]>(MOCK_TOKENS);
  const [revealed, setRevealed] = useState<string[]>([]);
  const [creating, setCreating] = useState(false);

  const toggleReveal = (id: string) =>
    setRevealed((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );

  const revoke = (id: string) =>
    setTokens((prev) =>
      prev.map((t) => (t.id === id ? { ...t, active: false } : t)),
    );

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="space-y-4"
    >
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-medium text-white/80">API Tokens</h3>
          <p className="text-[10px] text-white/30 mt-0.5">
            Manage tokens for accessing the Xavier API
          </p>
        </div>
        <button
          onClick={() => setCreating(!creating)}
          className="flex items-center gap-1.5 px-3 py-1.5 bg-[#39ff14]/8 border border-[#39ff14]/20 text-[#39ff14] text-xs rounded-lg hover:bg-[#39ff14]/12 transition-all"
        >
          <Plus className="w-3.5 h-3.5" />
          New Token
        </button>
      </div>

      {/* Create form (mock) */}
      <AnimatePresence>
        {creating && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            className="overflow-hidden"
          >
            <div className="p-4 rounded-xl bg-[#39ff14]/[0.03] border border-[#39ff14]/10 space-y-3">
              <h4 className="text-[10px] uppercase tracking-widest text-[#39ff14]/60">
                Create New Token
              </h4>
              <div>
                <label className="text-[10px] text-white/40 mb-1 block">
                  Label
                </label>
                <input
                  type="text"
                  placeholder="e.g. Jules Agent v2"
                  className="w-full bg-black/30 border border-white/10 text-white/80 text-xs px-3 py-2 rounded-lg outline-none"
                />
              </div>
              <div>
                <label className="text-[10px] text-white/40 mb-1 block">
                  Scopes
                </label>
                <div className="flex gap-2">
                  {["read", "write", "admin"].map((s) => (
                    <label
                      key={s}
                      className="flex items-center gap-1.5 cursor-pointer"
                    >
                      <input
                        type="checkbox"
                        defaultChecked={s === "read"}
                        className="accent-[#39ff14]"
                      />
                      <ScopeChip scope={s} />
                    </label>
                  ))}
                </div>
              </div>
              <div>
                <label className="text-[10px] text-white/40 mb-1 block">
                  Expiration (optional)
                </label>
                <input
                  type="date"
                  className="bg-black/30 border border-white/10 text-white/80 text-xs px-3 py-2 rounded-lg outline-none [color-scheme:dark]"
                />
              </div>
              <div className="flex gap-2 pt-1">
                <button className="flex-1 py-2 bg-[#39ff14]/10 border border-[#39ff14]/20 text-[#39ff14] text-xs rounded-lg hover:bg-[#39ff14]/15 transition-all">
                  Generate Token (mock)
                </button>
                <button
                  onClick={() => setCreating(false)}
                  className="px-3 py-2 text-white/30 hover:text-white/60 text-xs border border-white/10 rounded-lg transition-all"
                >
                  Cancel
                </button>
              </div>
              <div className="flex items-start gap-2 p-2 bg-amber-500/5 border border-amber-500/15 rounded-lg">
                <AlertTriangle className="w-3.5 h-3.5 text-amber-400 flex-shrink-0 mt-0.5" />
                <p className="text-[9px] text-amber-400/70">
                  Mock UI — backend token creation pending. See GitHub issue
                  #token-management-api.
                </p>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Token list */}
      <div className="space-y-2">
        {tokens.map((token) => (
          <div
            key={token.id}
            className={`p-4 rounded-xl border transition-colors ${token.active ? "bg-white/[0.02] border-white/[0.06]" : "bg-black/20 border-white/[0.03] opacity-50"}`}
          >
            <div className="flex items-start justify-between gap-3">
              <div className="flex items-center gap-2.5">
                <div
                  className={`w-6 h-6 rounded-lg flex items-center justify-center ${token.active ? "bg-[#39ff14]/10" : "bg-white/5"}`}
                >
                  {token.active ? (
                    <Key className="w-3 h-3 text-[#39ff14]/70" />
                  ) : (
                    <Lock className="w-3 h-3 text-white/20" />
                  )}
                </div>
                <div>
                  <p className="text-xs font-medium text-white/80">
                    {token.label}
                  </p>
                  <div className="flex items-center gap-1.5 mt-1">
                    <code className="text-[10px] font-mono text-white/40">
                      {revealed.includes(token.id)
                        ? "xav_revealed_mock_token_value_here"
                        : token.prefix}
                    </code>
                    <button
                      onClick={() => toggleReveal(token.id)}
                      className="text-white/20 hover:text-white/50 transition-colors"
                    >
                      {revealed.includes(token.id) ? (
                        <EyeOff className="w-3 h-3" />
                      ) : (
                        <Eye className="w-3 h-3" />
                      )}
                    </button>
                    <button
                      onClick={() =>
                        navigator.clipboard.writeText("mock-token")
                      }
                      className="text-white/20 hover:text-white/50 transition-colors"
                    >
                      <Copy className="w-3 h-3" />
                    </button>
                  </div>
                </div>
              </div>
              {token.active && (
                <button
                  onClick={() => revoke(token.id)}
                  className="p-1.5 text-red-400/40 hover:text-red-400 hover:bg-red-500/10 rounded-lg transition-all"
                  title="Revoke token"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              )}
            </div>
            <div className="flex items-center gap-4 mt-3 pt-2.5 border-t border-white/[0.04]">
              <div className="flex gap-1">
                {token.scopes.map((s) => (
                  <ScopeChip key={s} scope={s} />
                ))}
              </div>
              <div className="flex items-center gap-1 text-[9px] text-white/25 ml-auto">
                <Clock className="w-2.5 h-2.5" />
                <span>Created {formatRelative(token.created)}</span>
                {token.lastUsed && (
                  <>
                    <span>·</span>
                    <span>Used {formatRelative(token.lastUsed)}</span>
                  </>
                )}
                {token.expiresAt && (
                  <>
                    <span>·</span>
                    <span
                      className={
                        token.expiresAt < new Date()
                          ? "text-red-400"
                          : "text-white/25"
                      }
                    >
                      {token.expiresAt < new Date()
                        ? "Expired"
                        : `Expires ${formatRelative(token.expiresAt)}`}
                    </span>
                  </>
                )}
              </div>
            </div>
          </div>
        ))}
      </div>
    </motion.div>
  );
}

function ApiKeysSection() {
  const [keys, setKeys] = useState<ApiKey[]>(MOCK_API_KEYS);
  const [revealed, setRevealed] = useState<string[]>([]);

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="space-y-4"
    >
      <div>
        <h3 className="text-sm font-medium text-white/80">
          AI Provider API Keys
        </h3>
        <p className="text-[10px] text-white/30 mt-0.5">
          Configure API keys for LLM providers. Stored encrypted locally.
        </p>
      </div>
      <div className="space-y-3">
        {keys.map((key) => (
          <div
            key={key.id}
            className="p-4 rounded-xl bg-white/[0.02] border border-white/[0.06] space-y-2"
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2.5">
                <div className="w-6 h-6 rounded bg-white/5 flex items-center justify-center text-[9px] font-bold text-white/40">
                  {key.provider[0]}
                </div>
                <span className="text-xs font-medium text-white/80">
                  {key.provider}
                </span>
                {key.active ? (
                  <CheckCircle2 className="w-3.5 h-3.5 text-green-400" />
                ) : (
                  <AlertTriangle className="w-3.5 h-3.5 text-amber-400/60" />
                )}
              </div>
              <div className="flex items-center gap-1.5">
                <span className="text-[9px] text-white/25">
                  {key.lastValidated
                    ? `Validated ${formatRelative(key.lastValidated)}`
                    : "Not validated"}
                </span>
                <button
                  className="p-1.5 text-white/20 hover:text-cyan-400 hover:bg-cyan-500/10 rounded-lg transition-all"
                  title="Re-validate"
                >
                  <RefreshCw className="w-3 h-3" />
                </button>
              </div>
            </div>
            <div className="flex gap-2">
              <input
                type={revealed.includes(key.id) ? "text" : "password"}
                defaultValue="mock-api-key-value"
                className="flex-1 bg-black/30 border border-white/10 text-white/60 text-xs px-3 py-2 rounded-lg outline-none font-mono focus:border-[#39ff14]/30 transition-all"
              />
              <button
                onClick={() =>
                  setRevealed((prev) =>
                    prev.includes(key.id)
                      ? prev.filter((x) => x !== key.id)
                      : [...prev, key.id],
                  )
                }
                className="px-2 text-white/30 hover:text-white/60 border border-white/10 rounded-lg transition-all"
              >
                {revealed.includes(key.id) ? (
                  <EyeOff className="w-3.5 h-3.5" />
                ) : (
                  <Eye className="w-3.5 h-3.5" />
                )}
              </button>
              <button className="px-3 py-1 text-xs text-[#39ff14]/70 border border-[#39ff14]/20 rounded-lg hover:bg-[#39ff14]/5 transition-all">
                Save
              </button>
            </div>
          </div>
        ))}
      </div>
    </motion.div>
  );
}

function AuditLogSection() {
  const levelColors = {
    info: "text-cyan-400/60",
    warning: "text-amber-400/60",
    error: "text-red-400/70",
  };
  const levelBg = {
    info: "bg-cyan-500/5",
    warning: "bg-amber-500/5",
    error: "bg-red-500/5",
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="space-y-4"
    >
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-medium text-white/80">Audit Log</h3>
          <p className="text-[10px] text-white/30 mt-0.5">
            Recent security-relevant events. Persistence pending backend.
          </p>
        </div>
        <button className="flex items-center gap-1 px-2.5 py-1.5 text-xs text-white/30 border border-white/10 rounded-lg hover:border-white/20 hover:text-white/60 transition-all">
          <RefreshCw className="w-3 h-3" />
          Refresh
        </button>
      </div>
      <div className="space-y-1.5">
        {MOCK_AUDIT.map((entry) => (
          <div
            key={entry.id}
            className={`flex items-start gap-3 px-3 py-2.5 rounded-lg border border-white/[0.04] ${levelBg[entry.level]}`}
          >
            <Activity
              className={`w-3 h-3 flex-shrink-0 mt-0.5 ${levelColors[entry.level]}`}
            />
            <div className="flex-1 min-w-0">
              <div className="flex items-center justify-between gap-2">
                <p
                  className={`text-[11px] ${entry.level === "error" ? "text-red-400/80" : entry.level === "warning" ? "text-amber-400/80" : "text-white/65"}`}
                >
                  {entry.event}
                </p>
                <span className="flex-shrink-0 text-[9px] text-white/20">
                  {formatRelative(entry.timestamp)}
                </span>
              </div>
              <p className="text-[9px] text-white/25 mt-0.5 font-mono">
                {entry.source}
              </p>
            </div>
          </div>
        ))}
      </div>
      <div className="flex items-center gap-2 p-3 bg-amber-500/5 border border-amber-500/15 rounded-lg">
        <AlertTriangle className="w-3.5 h-3.5 text-amber-400/70 flex-shrink-0" />
        <p className="text-[10px] text-amber-400/60">
          Audit log persistence requires backend implementation. See GitHub
          issue #security-audit-log.
        </p>
      </div>
    </motion.div>
  );
}

function ProxySection() {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="space-y-4"
    >
      <div>
        <h3 className="text-sm font-medium text-white/80">
          Proxy & Network Security
        </h3>
        <p className="text-[10px] text-white/30 mt-0.5">
          Configure outbound proxy, CORS, and network policies.
        </p>
      </div>
      <div className="space-y-3">
        <div>
          <label className="text-[10px] uppercase tracking-widest text-white/40 mb-1.5 block">
            HTTP Proxy URL
          </label>
          <input
            type="url"
            placeholder="http://proxy.internal:8080"
            className="w-full bg-black/30 border border-white/10 text-white/80 text-xs px-3 py-2.5 rounded-lg outline-none transition-all focus:border-[#39ff14]/30"
          />
        </div>
        <div>
          <label className="text-[10px] uppercase tracking-widest text-white/40 mb-1.5 block">
            Allowed CORS Origins
          </label>
          <textarea
            rows={2}
            placeholder="https://app.xavier.local&#10;http://localhost:3000"
            className="w-full bg-black/30 border border-white/10 text-white/80 text-xs px-3 py-2.5 rounded-lg outline-none font-mono resize-none focus:border-[#39ff14]/30 transition-all"
          />
        </div>
        <div className="flex items-center justify-between p-3 rounded-lg bg-white/[0.02] border border-white/[0.05]">
          <div>
            <p className="text-xs text-white/70">Enforce HTTPS Only</p>
            <p className="text-[10px] text-white/30">
              Reject all non-TLS outbound connections
            </p>
          </div>
          <button className="relative w-9 h-5 rounded-full bg-[#39ff14]/20 border border-[#39ff14]/30">
            <div className="absolute top-0.5 left-[calc(100%-18px)] w-4 h-4 rounded-full bg-[#39ff14] transition-all" />
          </button>
        </div>
        <div className="flex items-center justify-between p-3 rounded-lg bg-white/[0.02] border border-white/[0.05]">
          <div>
            <p className="text-xs text-white/70">TPM Hardware Encryption</p>
            <p className="text-[10px] text-white/30">
              Use platform TPM for token storage (if available)
            </p>
          </div>
          <div className="flex items-center gap-1.5 text-[10px] text-green-400">
            <CheckCircle2 className="w-3.5 h-3.5" />
            Active
          </div>
        </div>
      </div>
    </motion.div>
  );
}

// ── Main Component ─────────────────────────────────────────────────────────

type SecuritySection = "tokens" | "apikeys" | "audit" | "proxy";

interface SecurityConfigPanelProps {
  embedded?: boolean;
}

export default function SecurityConfigPanel({
  embedded = false,
}: SecurityConfigPanelProps) {
  const [activeSection, setActiveSection] = useState<SecuritySection>("tokens");

  const sections: {
    id: SecuritySection;
    label: string;
    icon: React.ReactNode;
  }[] = [
    {
      id: "tokens",
      label: "API Tokens",
      icon: <Key className="w-3.5 h-3.5" />,
    },
    {
      id: "apikeys",
      label: "Provider Keys",
      icon: <Fingerprint className="w-3.5 h-3.5" />,
    },
    {
      id: "audit",
      label: "Audit Log",
      icon: <FileText className="w-3.5 h-3.5" />,
    },
    { id: "proxy", label: "Network", icon: <Globe className="w-3.5 h-3.5" /> },
  ];

  return (
    <div className={`flex ${embedded ? "h-full" : "h-screen"}`}>
      {/* Mini sidebar */}
      {!embedded && (
        <div className="w-48 border-r border-white/[0.05] p-4 bg-black/10 flex flex-col gap-1">
          <div className="flex items-center gap-2 mb-4 px-2">
            <Shield className="w-4 h-4 text-emerald-400" />
            <span className="text-xs font-semibold text-white/70 tracking-wide">
              Security
            </span>
          </div>
          {sections.map((s) => (
            <button
              key={s.id}
              onClick={() => setActiveSection(s.id)}
              className={`flex items-center gap-2.5 px-3 py-2.5 rounded-lg text-xs transition-all ${
                activeSection === s.id
                  ? "active-tab text-[#39ff14]"
                  : "text-white/40 hover:text-white/70 hover:bg-white/5"
              }`}
            >
              {s.icon}
              {s.label}
            </button>
          ))}
        </div>
      )}

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-6">
        {/* Embedded mode: tabs row */}
        {embedded && (
          <div className="flex gap-1 mb-6 bg-black/20 rounded-lg p-1 w-fit">
            {sections.map((s) => (
              <button
                key={s.id}
                onClick={() => setActiveSection(s.id)}
                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-[10px] uppercase tracking-widest transition-all ${
                  activeSection === s.id
                    ? "bg-white/10 text-white/90"
                    : "text-white/30 hover:text-white/60"
                }`}
              >
                {s.icon}
                {s.label}
              </button>
            ))}
          </div>
        )}

        <AnimatePresence mode="wait">
          {activeSection === "tokens" && <TokensSection key="tokens" />}
          {activeSection === "apikeys" && <ApiKeysSection key="apikeys" />}
          {activeSection === "audit" && <AuditLogSection key="audit" />}
          {activeSection === "proxy" && <ProxySection key="proxy" />}
        </AnimatePresence>
      </div>
    </div>
  );
}
