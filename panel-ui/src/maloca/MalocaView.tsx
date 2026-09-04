import React from "react";
import { X, RefreshCw, KeyRound } from "lucide-react";
import { OverviewTab } from "./components/OverviewTab";
import { RegistryTab } from "./components/RegistryTab";
import { GovernanceTab } from "./components/GovernanceTab";
import { SupportTab } from "./components/SupportTab";
import { BacklogTab } from "./components/BacklogTab";
import { TABS, TabConfig } from "./tabs";
import { useMalocaView } from "./useMalocaView";


type Props = {
  onClose?: () => void;
};

export default function MalocaView({ onClose }: Props) {
  const {
    activeTab,
    setActiveTab,
    deviceKey,
    isWebAuthnLoading,
    handleObtainWebAuthnKey,
  } = useMalocaView();

  const renderActiveTab = () => {
    switch (activeTab) {
      case "overview": return <OverviewTab />;
      case "registry": return <RegistryTab />;
      case "governance": return <GovernanceTab />;
      case "support": return <SupportTab />;
      case "backlog": return <BacklogTab />;
      default: return (
        <div className="flex items-center justify-center h-48 text-white/40 font-mono text-sm">
          Module '{activeTab}' under construction...
        </div>
      );
    }
  };

  const activeTabConfig = TABS.find((t) => t.id === activeTab) || TABS[0];

  return (
    <div className="maloca-root absolute inset-0 z-40 bg-[#050505] text-white overflow-hidden flex flex-col font-sans">
      <div className="maloca-shell relative flex-1 flex flex-col max-w-[1600px] w-full mx-auto p-4 sm:p-6 lg:p-8 overflow-hidden h-full">
        {onClose && (
          <button
            type="button"
            className="absolute top-6 right-6 p-2 rounded-full bg-white/5 hover:bg-white/10 text-white/50 hover:text-white transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500 z-50"
            onClick={onClose}
            aria-label="Cerrar Maloca"
          >
            <X size={20} aria-hidden="true" />
          </button>
        )}

        {/* Header */}
        <header className="flex flex-col sm:flex-row sm:items-end justify-between gap-4 pb-6 border-b border-white/10 shrink-0">
          <div>
            <h1 className="text-2xl sm:text-3xl font-bold tracking-tight text-white mb-1 flex items-center gap-3">
              <span className="w-3 h-3 rounded-full bg-[#39ff14] shadow-[0_0_12px_rgba(57,255,20,0.5)] animate-pulse" />
              Maloca Ops Hub
            </h1>
            <p className="text-sm font-mono text-white/50">Primary Xavier Ecosystem Workspace</p>
          </div>

          <div className="flex items-center gap-3">
            <button
              type="button"
              className="flex items-center gap-2 px-4 py-2 text-xs font-mono bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-400 border border-emerald-500/30 rounded-lg transition-all"
              onClick={handleObtainWebAuthnKey}
              disabled={isWebAuthnLoading}
            >
              <KeyRound size={16} />
              {isWebAuthnLoading ? "Authenticating..." : "Node Attestation"}
            </button>
          </div>
        </header>

        {deviceKey && (
          <div className="my-4 p-3 glass-panel border border-emerald-500/40 rounded-lg bg-[#0a0a0a] shrink-0">
            <span className="text-xs text-emerald-400 font-mono mb-1 block">Attestation Key (WebAuthn PRF):</span>
            <code className="text-[11px] font-mono text-white/70 break-all bg-black/50 p-2 rounded block">
              {deviceKey}
            </code>
          </div>
        )}

        <div className="flex flex-col lg:flex-row gap-6 mt-6 flex-1 min-h-0">
          {/* Sidebar Navigation */}
          <nav className="w-full lg:w-64 shrink-0 flex lg:flex-col gap-2 overflow-x-auto lg:overflow-x-visible custom-scrollbar pb-2 lg:pb-0">
            {TABS.map((tab) => {
              const Icon = tab.icon;
              const isActive = activeTab === tab.id;
              return (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id)}
                  className={`flex items-center gap-3 px-4 py-3 rounded-xl transition-all whitespace-nowrap lg:whitespace-normal text-left
                    ${isActive
                      ? 'bg-white/10 text-white shadow-[inset_3px_0_0_#39ff14]'
                      : 'text-white/50 hover:bg-white/5 hover:text-white/80'}`}
                >
                  <Icon size={18} className={isActive ? 'text-[#39ff14]' : ''} />
                  <div className="hidden sm:block">
                    <div className={`text-sm font-medium ${isActive ? 'text-white' : ''}`}>{tab.label}</div>
                    <div className="text-[10px] opacity-60 hidden lg:block mt-0.5 line-clamp-1">{tab.description}</div>
                  </div>
                  <span className="sm:hidden text-sm font-medium">{tab.label}</span>
                </button>
              );
            })}
          </nav>

          {/* Main Content Area */}
          <main className="flex-1 min-w-0 bg-[#0a0a0a] rounded-2xl border border-white/5 p-4 sm:p-6 lg:p-8 overflow-y-auto custom-scrollbar relative">
            <div className="max-w-5xl mx-auto">
              {renderActiveTab()}
            </div>
          </main>
        </div>
      </div>
    </div>
  );
}
