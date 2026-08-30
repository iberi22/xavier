import React, { useState, useMemo } from "react";
import {
  Building2,
  Landmark,
  HeartPulse,
  Server,
  Radio,
  Check,
  Copy,
  X,
  ArrowRight,
  ArrowLeft,
  QrCode,
  Shield,
  Zap,
} from "lucide-react";
import { QrCodeDisplay } from "../QrCodeDisplay";

export type NetworkTemplate = "enterprise" | "dao" | "health";
export type EncryptionLevel = "standard" | "high" | "maximum";

export interface SyncPolicyConfig {
  autoSync: boolean;
  syncIntervalMinutes: number;
  encryptionLevel: EncryptionLevel;
  allowPeerRelay: boolean;
}

export interface MeshNetworkConfig {
  name: string;
  template: NetworkTemplate;
  isMasterHost: boolean;
  bootstrapRelayUrl: string;
  syncPolicy: SyncPolicyConfig;
  inviteCode: string;
}

export interface CreateNetworkWizardProps {
  isOpen: boolean;
  onClose: () => void;
  onComplete?: (config: MeshNetworkConfig) => void;
}

const TEMPLATE_OPTIONS: Array<{
  id: NetworkTemplate;
  name: string;
  emoji: string;
  icon: React.ComponentType<{ className?: string }>;
  description: string;
  defaultEncryption: EncryptionLevel;
}> = [
  {
    id: "enterprise",
    name: "Enterprise Brain",
    emoji: "🏢",
    icon: Building2,
    description: "High security, centralized indexing, encrypted peer relays for corporate teams.",
    defaultEncryption: "high",
  },
  {
    id: "dao",
    name: "SWAL DAO",
    emoji: "🏛️",
    icon: Landmark,
    description: "Decentralized consensus, open peer discovery, voting-weighted sync governance.",
    defaultEncryption: "standard",
  },
  {
    id: "health",
    name: "Family Health",
    emoji: "🩺",
    icon: HeartPulse,
    description: "Zero-knowledge privacy, localized peer-only sync, maximum encrypted data vaults.",
    defaultEncryption: "maximum",
  },
];

export const CreateNetworkWizard: React.FC<CreateNetworkWizardProps> = ({
  isOpen,
  onClose,
  onComplete,
}) => {
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [networkName, setNetworkName] = useState("");
  const [selectedTemplate, setSelectedTemplate] = useState<NetworkTemplate>("enterprise");
  const [isMasterHost, setIsMasterHost] = useState(true);
  const [bootstrapRelayUrl, setBootstrapRelayUrl] = useState("");
  const [autoSync, setAutoSync] = useState(true);
  const [syncIntervalMinutes, setSyncIntervalMinutes] = useState(15);
  const [encryptionLevel, setEncryptionLevel] = useState<EncryptionLevel>("high");
  const [allowPeerRelay, setAllowPeerRelay] = useState(true);
  const [copiedCode, setCopiedCode] = useState(false);

  // Generate deterministic or stable invite code for display
  const inviteCode = useMemo(() => {
    const prefix = selectedTemplate.toUpperCase().slice(0, 3);
    const sanitizedName = (networkName || "MESH").toUpperCase().replace(/[^A-Z0-9]/g, "").slice(0, 6) || "XAVIER";
    return `XAVIER-MESH-${prefix}-${sanitizedName}-8F92`;
  }, [selectedTemplate, networkName]);

  if (!isOpen) return null;

  const handleTemplateSelect = (templateId: NetworkTemplate) => {
    setSelectedTemplate(templateId);
    const tmpl = TEMPLATE_OPTIONS.find((t) => t.id === templateId);
    if (tmpl) {
      setEncryptionLevel(tmpl.defaultEncryption);
    }
  };

  const handleFinish = () => {
    const config: MeshNetworkConfig = {
      name: networkName.trim() || "New Mesh Network",
      template: selectedTemplate,
      isMasterHost,
      bootstrapRelayUrl: isMasterHost ? "" : bootstrapRelayUrl.trim(),
      syncPolicy: {
        autoSync,
        syncIntervalMinutes,
        encryptionLevel,
        allowPeerRelay,
      },
      inviteCode,
    };
    if (onComplete) {
      onComplete(config);
    }
    onClose();
  };

  const copyInviteCode = () => {
    navigator.clipboard.writeText(inviteCode);
    setCopiedCode(true);
    setTimeout(() => setCopiedCode(false), 2000);
  };

  return (
    <div className="fixed inset-0 z-50 bg-black/70 backdrop-blur-sm flex items-center justify-center p-4">
      <div className="bg-[#12141a] border border-white/10 rounded-2xl w-full max-w-2xl shadow-2xl flex flex-col overflow-hidden text-white">
        {/* Header */}
        <div className="px-6 py-5 border-b border-white/10 flex items-center justify-between bg-white/[0.02]">
          <div>
            <h2 className="text-xl font-medium tracking-tight flex items-center gap-2">
              <Zap className="w-5 h-5 text-[#39ff14]" />
              Create New Mesh Network
            </h2>
            <p className="text-xs text-white/50 mt-1">
              Step {step} of 3: {step === 1 ? "Name & Template" : step === 2 ? "Node Mode & Relay" : "Sync & Invite"}
            </p>
          </div>
          <button
            type="button"
            aria-label="Close wizard"
            onClick={onClose}
            className="p-2 text-white/40 hover:text-white hover:bg-white/10 rounded-lg transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Wizard Step Navigation Bar */}
        <div className="flex border-b border-white/5 bg-black/20">
          <div
            className={`flex-1 py-3 text-center text-xs font-medium border-b-2 transition-colors ${
              step === 1 ? "border-[#39ff14] text-[#39ff14]" : "border-transparent text-white/40"
            }`}
          >
            1. Template
          </div>
          <div
            className={`flex-1 py-3 text-center text-xs font-medium border-b-2 transition-colors ${
              step === 2 ? "border-[#39ff14] text-[#39ff14]" : "border-transparent text-white/40"
            }`}
          >
            2. Host Mode
          </div>
          <div
            className={`flex-1 py-3 text-center text-xs font-medium border-b-2 transition-colors ${
              step === 3 ? "border-[#39ff14] text-[#39ff14]" : "border-transparent text-white/40"
            }`}
          >
            3. Sync & Invite
          </div>
        </div>

        {/* Content Body */}
        <div className="p-6 space-y-6 overflow-y-auto max-h-[70vh]">
          {/* STEP 1: Name & Template */}
          {step === 1 && (
            <div className="space-y-6">
              <div className="space-y-2">
                <label htmlFor="mesh-network-name" className="text-xs font-medium uppercase tracking-wider text-white/70 block">
                  Mesh Network Name
                </label>
                <input
                  id="mesh-network-name"
                  type="text"
                  value={networkName}
                  onChange={(e) => setNetworkName(e.target.value)}
                  placeholder="e.g. Acumen Global Mesh"
                  className="w-full bg-black/40 border border-white/10 rounded-xl px-4 py-3 text-sm text-white placeholder-white/30 outline-none focus:border-[#39ff14]/50 transition-colors"
                />
              </div>

              <div className="space-y-3">
                <label className="text-xs font-medium uppercase tracking-wider text-white/70 block">
                  Select Network Template
                </label>
                <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                  {TEMPLATE_OPTIONS.map((tmpl) => {
                    const Icon = tmpl.icon;
                    const isSelected = selectedTemplate === tmpl.id;
                    return (
                      <button
                        key={tmpl.id}
                        type="button"
                        onClick={() => handleTemplateSelect(tmpl.id)}
                        className={`p-4 rounded-xl border text-left transition-all relative flex flex-col justify-between ${
                          isSelected
                            ? "border-[#39ff14] bg-[#39ff14]/[0.05] shadow-[0_0_15px_rgba(57,255,20,0.15)]"
                            : "border-white/10 bg-white/[0.02] hover:border-white/20 hover:bg-white/[0.04]"
                        }`}
                      >
                        <div>
                          <div className="flex items-center justify-between mb-2">
                            <span className="text-2xl">{tmpl.emoji}</span>
                            <Icon className={`w-5 h-5 ${isSelected ? "text-[#39ff14]" : "text-white/40"}`} />
                          </div>
                          <h3 className="text-sm font-semibold text-white mb-1">{tmpl.name}</h3>
                          <p className="text-xs text-white/50 leading-relaxed">{tmpl.description}</p>
                        </div>
                        {isSelected && (
                          <div className="mt-3 flex items-center gap-1 text-[11px] text-[#39ff14] font-medium">
                            <Check className="w-3.5 h-3.5" /> Selected
                          </div>
                        )}
                      </button>
                    );
                  })}
                </div>
              </div>
            </div>
          )}

          {/* STEP 2: Host vs Client Mode */}
          {step === 2 && (
            <div className="space-y-6">
              <div className="space-y-3">
                <label className="text-xs font-medium uppercase tracking-wider text-white/70 block">
                  Node Operating Mode
                </label>
                <div className="grid grid-cols-1 gap-4">
                  {/* Option 1: Master Host */}
                  <button
                    type="button"
                    onClick={() => setIsMasterHost(true)}
                    className={`p-5 rounded-xl border text-left transition-all flex items-start gap-4 ${
                      isMasterHost
                        ? "border-[#39ff14] bg-[#39ff14]/[0.05]"
                        : "border-white/10 bg-white/[0.02] hover:border-white/20"
                    }`}
                  >
                    <div className={`p-3 rounded-lg ${isMasterHost ? "bg-[#39ff14]/20 text-[#39ff14]" : "bg-white/5 text-white/40"}`}>
                      <Server className="w-6 h-6" />
                    </div>
                    <div className="flex-1">
                      <div className="flex items-center justify-between">
                        <h3 className="text-sm font-semibold text-white">Host as Master Node</h3>
                        {isMasterHost && <Check className="w-4 h-4 text-[#39ff14]" />}
                      </div>
                      <p className="text-xs text-white/50 mt-1">
                        This node will act as the primary seed coordinator, issuing invite tokens and hosting network state synchronization.
                      </p>
                    </div>
                  </button>

                  {/* Option 2: Join Bootstrap Relay */}
                  <button
                    type="button"
                    onClick={() => setIsMasterHost(false)}
                    className={`p-5 rounded-xl border text-left transition-all flex items-start gap-4 ${
                      !isMasterHost
                        ? "border-[#39ff14] bg-[#39ff14]/[0.05]"
                        : "border-white/10 bg-white/[0.02] hover:border-white/20"
                    }`}
                  >
                    <div className={`p-3 rounded-lg ${!isMasterHost ? "bg-[#39ff14]/20 text-[#39ff14]" : "bg-white/5 text-white/40"}`}>
                      <Radio className="w-6 h-6" />
                    </div>
                    <div className="flex-1">
                      <div className="flex items-center justify-between">
                        <h3 className="text-sm font-semibold text-white">Connect to existing bootstrap relay</h3>
                        {!isMasterHost && <Check className="w-4 h-4 text-[#39ff14]" />}
                      </div>
                      <p className="text-xs text-white/50 mt-1">
                        Join an existing network by specifying a remote bootstrap peer URL or seed node address.
                      </p>
                    </div>
                  </button>
                </div>
              </div>

              {!isMasterHost && (
                <div className="space-y-2 p-4 rounded-xl bg-white/[0.02] border border-white/10">
                  <label htmlFor="bootstrap-relay-url" className="text-xs font-medium uppercase tracking-wider text-white/70 block">
                    Bootstrap Relay URL / Address
                  </label>
                  <input
                    id="bootstrap-relay-url"
                    type="text"
                    value={bootstrapRelayUrl}
                    onChange={(e) => setBootstrapRelayUrl(e.target.value)}
                    placeholder="https://relay.mesh.xavier.internal:8443 or peer ID"
                    className="w-full bg-black/40 border border-white/10 rounded-lg px-3 py-2 text-xs text-white font-mono placeholder-white/30 outline-none focus:border-[#39ff14]/50"
                  />
                </div>
              )}
            </div>
          )}

          {/* STEP 3: Configure Sync Policies & Invite Code/QR */}
          {step === 3 && (
            <div className="space-y-6">
              {/* Sync Policies */}
              <div className="p-4 rounded-xl bg-white/[0.02] border border-white/10 space-y-4">
                <h3 className="text-xs font-medium uppercase tracking-wider text-white/70 flex items-center gap-2">
                  <Shield className="w-4 h-4 text-[#39ff14]" />
                  Initial Sync Policies
                </h3>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  {/* Auto-Sync Toggle */}
                  <div className="flex items-center justify-between p-3 rounded-lg bg-black/30 border border-white/5">
                    <div>
                      <p className="text-xs font-medium text-white">Automated Sync</p>
                      <p className="text-[10px] text-white/40">Periodic background synchronization</p>
                    </div>
                    <input
                      type="checkbox"
                      checked={autoSync}
                      onChange={(e) => setAutoSync(e.target.checked)}
                      className="w-4 h-4 accent-[#39ff14] cursor-pointer"
                    />
                  </div>

                  {/* Peer Relay Toggle */}
                  <div className="flex items-center justify-between p-3 rounded-lg bg-black/30 border border-white/5">
                    <div>
                      <p className="text-xs font-medium text-white font-mono">Allow Peer Relay</p>
                      <p className="text-[10px] text-white/40">Forward encrypted traffic for peers</p>
                    </div>
                    <input
                      type="checkbox"
                      checked={allowPeerRelay}
                      onChange={(e) => setAllowPeerRelay(e.target.checked)}
                      className="w-4 h-4 accent-[#39ff14] cursor-pointer"
                    />
                  </div>
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-4 pt-2">
                  {/* Sync Interval */}
                  <div>
                    <label htmlFor="sync-interval-select" className="text-[10px] uppercase text-white/40 tracking-wider block mb-1">
                      Sync Frequency
                    </label>
                    <select
                      id="sync-interval-select"
                      value={syncIntervalMinutes}
                      onChange={(e) => setSyncIntervalMinutes(Number(e.target.value))}
                      className="w-full bg-black/40 border border-white/10 rounded-lg px-3 py-2 text-xs text-white outline-none focus:border-[#39ff14]/40"
                    >
                      <option value={5}>Every 5 minutes</option>
                      <option value={15}>Every 15 minutes</option>
                      <option value={30}>Every 30 minutes</option>
                      <option value={60}>Every 60 minutes</option>
                    </select>
                  </div>

                  {/* Encryption Level */}
                  <div>
                    <label htmlFor="encryption-level-select" className="text-[10px] uppercase text-white/40 tracking-wider block mb-1">
                      Encryption Security Depth
                    </label>
                    <select
                      id="encryption-level-select"
                      value={encryptionLevel}
                      onChange={(e) => setEncryptionLevel(e.target.value as EncryptionLevel)}
                      className="w-full bg-black/40 border border-white/10 rounded-lg px-3 py-2 text-xs text-white outline-none focus:border-[#39ff14]/40"
                    >
                      <option value="standard">Standard (AES-256-GCM)</option>
                      <option value="high">High (HKDF-SHA256 + AES-256-GCM)</option>
                      <option value="maximum">Maximum (Zero-Knowledge Dual Layer)</option>
                    </select>
                  </div>
                </div>
              </div>

              {/* Generated Invite Code & QR */}
              <div className="p-4 rounded-xl bg-[#39ff14]/[0.03] border border-[#39ff14]/20 space-y-4">
                <div className="flex items-center justify-between">
                  <h3 className="text-xs font-medium uppercase tracking-wider text-[#39ff14] flex items-center gap-2">
                    <QrCode className="w-4 h-4" />
                    Generated Invite Code & QR
                  </h3>
                  <span className="text-[10px] text-white/40 font-mono">Valid for 24 Hours</span>
                </div>

                <div className="flex flex-col sm:flex-row items-center gap-4">
                  <QrCodeDisplay value={inviteCode} />
                  <div className="flex-1 w-full space-y-3">
                    <div>
                      <label className="text-[10px] uppercase tracking-wider text-white/40 block mb-1">
                        Network Invite Token
                      </label>
                      <div className="flex items-center gap-2">
                        <code className="flex-1 bg-black/50 border border-white/10 p-2.5 rounded-lg text-xs font-mono text-[#39ff14] break-all select-all">
                          {inviteCode}
                        </code>
                        <button
                          type="button"
                          aria-label="Copy invite code"
                          onClick={copyInviteCode}
                          className="p-2.5 bg-white/5 border border-white/10 hover:bg-white/10 rounded-lg transition-colors text-white/70 hover:text-white"
                        >
                          {copiedCode ? <Check className="w-4 h-4 text-[#39ff14]" /> : <Copy className="w-4 h-4" />}
                        </button>
                      </div>
                    </div>
                    <p className="text-[11px] text-white/50 leading-relaxed">
                      Share this code or QR with authorized peers to let them join <span className="text-white font-medium">{networkName || "your network"}</span>.
                    </p>
                  </div>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Footer Navigation Controls */}
        <div className="px-6 py-4 border-t border-white/10 flex items-center justify-between bg-white/[0.02]">
          {step > 1 ? (
            <button
              type="button"
              onClick={() => setStep((s) => (s - 1) as 1 | 2 | 3)}
              className="px-4 py-2 border border-white/10 text-xs text-white/80 hover:text-white hover:border-white/20 rounded-xl transition-all flex items-center gap-2"
            >
              <ArrowLeft className="w-4 h-4" />
              Back
            </button>
          ) : (
            <div />
          )}

          {step < 3 ? (
            <button
              type="button"
              disabled={step === 1 && !networkName.trim()}
              onClick={() => setStep((s) => (s + 1) as 1 | 2 | 3)}
              className="px-5 py-2.5 bg-[#39ff14] text-black text-xs font-semibold rounded-xl hover:bg-[#32e010] disabled:opacity-40 transition-all flex items-center gap-2 shadow-[0_0_15px_rgba(57,255,20,0.2)]"
            >
              Next
              <ArrowRight className="w-4 h-4" />
            </button>
          ) : (
            <button
              type="button"
              onClick={handleFinish}
              className="px-6 py-2.5 bg-[#39ff14] text-black text-xs font-semibold rounded-xl hover:bg-[#32e010] transition-all flex items-center gap-2 shadow-[0_0_20px_rgba(57,255,20,0.3)]"
            >
              <Check className="w-4 h-4" />
              Finish & Deploy Network
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
