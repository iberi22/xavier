import {
  Cpu,
  Globe,
  KeyRound,
  LayoutDashboard,
  ListTodo,
  RefreshCw,
  ShieldAlert,
  Target,
  X,
} from "lucide-react";
import React, { useEffect, useRef, useState } from "react";
import { obtainDeviceKeyViaWebAuthn } from "./webauthn";
import "./maloca.css";

export type MalocaTabId =
  | "overview"
  | "registry"
  | "goals"
  | "backlog"
  | "challenges"
  | "models";

interface TabConfig {
  id: MalocaTabId;
  label: string;
  icon: React.ElementType;
  endpoint?: string;
  description: string;
}

const TABS: TabConfig[] = [
  {
    id: "overview",
    label: "Hub Overview",
    icon: LayoutDashboard,
    description: "Maloca ops workspace custom element host & primary node status.",
  },
  {
    id: "registry",
    label: "Ecosystem Registry",
    icon: Globe,
    endpoint: "/v1/maloca/registry",
    description: "Distributed P2P node directory and registered ecosystem services.",
  },
  {
    id: "goals",
    label: "GOAL.md Alignment",
    icon: Target,
    endpoint: "/v1/maloca/alignment",
    description: "Canonical SWAL project mission alignment & milestone tracking.",
  },
  {
    id: "backlog",
    label: "Global Backlog",
    icon: ListTodo,
    endpoint: "/v1/maloca/backlog/unified",
    description: "Unified cross-node task queue & work-item scheduling backlog.",
  },
  {
    id: "challenges",
    label: "Human Challenge",
    icon: ShieldAlert,
    endpoint: "/v1/maloca/challenges/active",
    description: "Human-in-the-loop validation requests & consensus challenges.",
  },
  {
    id: "models",
    label: "Model Connectivity",
    icon: Cpu,
    endpoint: "/v1/maloca/models/status",
    description: "LLM backend routes, ONNX cross-encoders & local inference availability.",
  },
];

type Props = {
  onClose?: () => void;
  /** Scaffold: treat local session as manager ACL (no vote weight). */
  isManager?: boolean;
};

export default function MalocaView({ onClose, isManager = true }: Props) {
  const [activeTab, setActiveTab] = useState<MalocaTabId>("overview");
  const [error, setError] = useState<string | null>(null);
  const [_isReady, setIsReady] = useState(false);
  const [deviceKey, setDeviceKey] = useState<string | null>(null);
  const [isWebAuthnLoading, setIsWebAuthnLoading] = useState(false);
  const panelRef = useRef<HTMLElement | null>(null);

  // Tab dynamic data states
  const [tabData, setTabData] = useState<Record<string, any>>({});
  const [tabLoading, setTabLoading] = useState<Record<string, boolean>>({});
  const [tabError, setTabError] = useState<Record<string, string | null>>({});

  const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const xavierUrl = isTauri ? "http://127.0.0.1:8006" : (window.location.origin || "http://127.0.0.1:8006");

  const handleObtainWebAuthnKey = async () => {
    setIsWebAuthnLoading(true);
    setError(null);
    try {
      const key = await obtainDeviceKeyViaWebAuthn();
      setDeviceKey(key);
    } catch (err: any) {
      setError(err?.message || "Error al obtener clave WebAuthn");
    } finally {
      setIsWebAuthnLoading(false);
    }
  };

  const fetchTabData = async (tab: TabConfig) => {
    if (!tab.endpoint) return;
    setTabLoading((prev) => ({ ...prev, [tab.id]: true }));
    setTabError((prev) => ({ ...prev, [tab.id]: null }));

    try {
      const res = await fetch(`${xavierUrl}${tab.endpoint}`);
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}: ${res.statusText}`);
      }
      const data = await res.json();
      setTabData((prev) => ({ ...prev, [tab.id]: data }));
    } catch (err: any) {
      setTabError((prev) => ({
        ...prev,
        [tab.id]: err?.message || "Error fetching endpoint data",
      }));
    } finally {
      setTabLoading((prev) => ({ ...prev, [tab.id]: false }));
    }
  };

  useEffect(() => {
    const currentTabConfig = TABS.find((t) => t.id === activeTab);
    if (currentTabConfig?.endpoint && !tabData[activeTab] && !tabLoading[activeTab]) {
      fetchTabData(currentTabConfig);
    }
  }, [activeTab]);

  useEffect(() => {
    // Fallback dynamic import of the Custom Element if the sandbox lacks direct access
    const embedPkg = "@swal/maloca-embed";
    import(/* @vite-ignore */ embedPkg)
      .then(() => {
        console.log("@swal/maloca-embed loaded successfully");
      })
      .catch((err) => {
        console.warn(
          "Failed to dynamically load @swal/maloca-embed. Ensure it is registered in the workspace/environment.",
          err
        );
      });
  }, []);

  useEffect(() => {
    const element = panelRef.current;
    if (!element) return;

    const handleReady = (e: Event) => {
      console.log("Maloca Panel Ready:", e);
      setIsReady(true);
    };

    const handleError = (e: Event) => {
      console.error("Maloca Panel Error:", e);
      const customErr = e as CustomEvent;
      setError(customErr.detail?.message || "Error loading Maloca Custom Element");
    };

    element.addEventListener("maloca-ready", handleReady);
    element.addEventListener("maloca-error", handleError);

    return () => {
      element.removeEventListener("maloca-ready", handleReady);
      element.removeEventListener("maloca-error", handleError);
    };
  }, []);

  const activeTabConfig = TABS.find((t) => t.id === activeTab) || TABS[0];

  return (
    <div className="maloca-root absolute inset-0 z-40">
      <link
        rel="stylesheet"
        href="https://fonts.googleapis.com/css2?family=Source+Sans+3:wght@400;500;600&family=IBM+Plex+Mono:wght@400;500&display=swap"
      />
      <div className="maloca-shell relative">
        {onClose && (
          <button
            type="button"
            className="maloca-btn maloca-close focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500"
            onClick={onClose}
            aria-label="Cerrar Maloca"
          >
            <X size={16} aria-hidden="true" />
          </button>
        )}

        <header className="maloca-header">
          <div>
            <h1 className="maloca-brand">Maloca</h1>
            <p className="maloca-subtitle">Ops workspace · host primario Xavier</p>
          </div>
          <div className="flex items-center gap-2 mt-2 sm:mt-0">
            <button
              type="button"
              className="maloca-btn flex items-center gap-1.5 px-3 py-1.5 text-xs font-mono bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700 rounded transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500"
              onClick={handleObtainWebAuthnKey}
              disabled={isWebAuthnLoading}
              title="Obtener Clave de Dispositivo vía WebAuthn PRF"
            >
              <KeyRound size={14} aria-hidden="true" />
              {isWebAuthnLoading ? "Autenticando..." : "WebAuthn Key"}
            </button>
          </div>
        </header>

        {deviceKey && (
          <div className="maloca-card my-2 p-2.5 bg-slate-900/90 border border-emerald-500/40 rounded text-xs">
            <span className="text-emerald-400 font-medium block mb-1">Device Key (WebAuthn PRF):</span>
            <code className="font-mono text-slate-300 break-all select-all block bg-slate-950 p-1.5 rounded">
              {deviceKey}
            </code>
          </div>
        )}
        {error && (
          <div
            className="maloca-card"
            style={{ borderColor: "var(--maloca-danger)" }}
          >
            <span className="maloca-muted">Xavier /maloca: {error}</span>
          </div>
        )}

        {/* Multi-Tab Navigation Sub-Header */}
        <nav
          className="maloca-tab-bar"
          role="tablist"
          aria-label="Maloca Navigation Tabs"
        >
          {TABS.map((tab) => {
            const Icon = tab.icon;
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                type="button"
                id={`tab-${tab.id}`}
                data-tab={`tab-${tab.id}`}
                role="tab"
                aria-selected={isActive}
                aria-controls={`panel-${tab.id}`}
                tabIndex={isActive ? 0 : -1}
                className={`maloca-tab tab-${tab.id} ${isActive ? "active" : ""}`}
                onClick={() => setActiveTab(tab.id)}
              >
                <Icon size={16} aria-hidden="true" />
                <span>{tab.label}</span>
              </button>
            );
          })}
        </nav>

        {/* Tab Content Panels */}
        <main className="mt-2">
          {activeTab === "overview" && (
            <div
              id="panel-overview"
              role="tabpanel"
              aria-labelledby="tab-overview"
              className="maloca-panel"
            >
              <swal-maloca-panel
                ref={panelRef as any}
                app-id="xavier"
                xavier-url={xavierUrl}
              />
            </div>
          )}

          {activeTab !== "overview" && (
            <div
              id={`panel-${activeTab}`}
              role="tabpanel"
              aria-labelledby={`tab-${activeTab}`}
              className="maloca-subview"
            >
              <div className="flex flex-wrap items-center justify-between gap-2 mb-4 pb-3 border-b border-[var(--maloca-border)]">
                <div>
                  <h2 className="text-lg font-semibold flex items-center gap-2">
                    {React.createElement(activeTabConfig.icon, {
                      size: 20,
                      "aria-hidden": "true",
                    })}
                    {activeTabConfig.label}
                  </h2>
                  <p className="maloca-subtitle text-xs mt-0.5">
                    {activeTabConfig.description}
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  <span className="maloca-mono text-xs text-slate-400">
                    {activeTabConfig.endpoint}
                  </span>
                  <button
                    type="button"
                    className="maloca-btn flex items-center gap-1.5 text-xs py-1 px-2.5"
                    onClick={() => fetchTabData(activeTabConfig)}
                    disabled={tabLoading[activeTab]}
                    title="Refresh endpoint data"
                  >
                    <RefreshCw
                      size={13}
                      className={tabLoading[activeTab] ? "animate-spin" : ""}
                      aria-hidden="true"
                    />
                    Refresh
                  </button>
                </div>
              </div>

              {tabLoading[activeTab] && (
                <div className="p-8 text-center text-slate-500 font-mono text-xs">
                  Loading {activeTabConfig.endpoint}...
                </div>
              )}

              {tabError[activeTab] && (
                <div className="p-4 bg-rose-950/30 border border-rose-800/50 rounded-lg text-rose-300 text-xs">
                  <p className="font-semibold mb-1">Failed to fetch data:</p>
                  <code className="font-mono text-rose-200">
                    {tabError[activeTab]}
                  </code>
                </div>
              )}

              {!tabLoading[activeTab] && !tabError[activeTab] && (
                <div className="space-y-4">
                  <div className="bg-slate-900/60 p-4 rounded-lg border border-[var(--maloca-border)] overflow-x-auto">
                    <pre className="maloca-mono text-xs text-emerald-300">
                      {JSON.stringify(
                        tabData[activeTab] || {
                          status: "connected",
                          endpoint: activeTabConfig.endpoint,
                          timestamp: new Date().toISOString(),
                          host: xavierUrl,
                          manager_mode: isManager,
                        },
                        null,
                        2
                      )}
                    </pre>
                  </div>
                </div>
              )}
            </div>
          )}
        </main>
      </div>
    </div>
  );
}
