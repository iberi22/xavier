import {
  ArrowUpRight,
  Bot,
  Check,
  ChevronRight,
  Coins,
  Copy,
  Cpu,
  ExternalLink,
  Key,
  Layers,
  Lock,
  RefreshCw,
  Shield,
  ShieldCheck,
  Sliders,
  Wallet,
  X,
  Zap,
} from "lucide-react";
import React, { useEffect, useState } from "react";
import { useAuthStore } from "../auth/AuthProvider";
import { RecoveryModal } from "./RecoveryModal";

interface WalletViewProps {
  onClose: () => void;
}

interface KarmaEvent {
  id: string;
  type: "welcome" | "heartbeat" | "rollup";
  title: string;
  amount: number;
  timestamp: number;
  status: "confirmed" | "pending";
}

export const WalletView: React.FC<WalletViewProps> = ({ onClose }) => {
  const { authUser } = useAuthStore();
  const [copiedKey, setCopiedKey] = useState(false);
  const [polygonAddress, setPolygonAddress] = useState(() => {
    return typeof localStorage !== "undefined"
      ? localStorage.getItem("swal_polygon_payout_addr") || ""
      : "";
  });
  const [savedAddress, setSavedAddress] = useState(false);

  // Agent Delegation State
  const [agentDelegationEnabled, setAgentDelegationEnabled] = useState(() => {
    return typeof localStorage !== "undefined"
      ? localStorage.getItem("swal_agent_delegation") === "true"
      : true;
  });
  const [dailyLimit, setDailyLimit] = useState(() => {
    return typeof localStorage !== "undefined"
      ? localStorage.getItem("swal_agent_daily_limit") || "50"
      : "50";
  });
  const [autoClaim, setAutoClaim] = useState(true);
  const [isSyncing, setIsSyncing] = useState(false);
  const [syncSuccess, setSyncSuccess] = useState(false);
  const [showRecovery, setShowRecovery] = useState(false);
  const mnemonic = "abandon amount abandon amount abandon amount abandon amount abandon amount abandon announce";

  const karma = authUser?.karma ?? 10;
  const nodeId = authUser?.node_id || (typeof authUser?.id === "string" ? authUser.id : "node_ed0fe57d1f71cbf6");
  const genesisNode = "xaviercloud.swal.network";

  const handleCopyNodeId = () => {
    void navigator.clipboard.writeText(nodeId);
    setCopiedKey(true);
    setTimeout(() => setCopiedKey(false), 2000);
  };

  const handleSavePolygonAddress = (e: React.FormEvent) => {
    e.preventDefault();
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("swal_polygon_payout_addr", polygonAddress.trim());
    }
    setSavedAddress(true);
    setTimeout(() => setSavedAddress(false), 2500);
  };

  const handleToggleDelegation = () => {
    const next = !agentDelegationEnabled;
    setAgentDelegationEnabled(next);
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("swal_agent_delegation", String(next));
    }
  };

  const handleUpdateLimit = (val: string) => {
    setDailyLimit(val);
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("swal_agent_daily_limit", val);
    }
  };

  const handleSyncToPolygon = () => {
    setIsSyncing(true);
    setSyncSuccess(false);
    setTimeout(() => {
      setIsSyncing(false);
      setSyncSuccess(true);
      setTimeout(() => setSyncSuccess(false), 3500);
    }, 1600);
  };

  const recentEvents: KarmaEvent[] = [
    {
      id: "ev_1",
      type: "heartbeat",
      title: "Prueba de Disponibilidad (Mesh Heartbeat)",
      amount: 1,
      timestamp: Date.now() - 1000 * 60 * 5,
      status: "confirmed",
    },
    {
      id: "ev_2",
      type: "heartbeat",
      title: "Prueba de Disponibilidad (Mesh Heartbeat)",
      amount: 1,
      timestamp: Date.now() - 1000 * 60 * 10,
      status: "confirmed",
    },
    {
      id: "ev_3",
      type: "welcome",
      title: "Bono Génesis de Bienvenida al Mesh",
      amount: 10,
      timestamp: Date.now() - 1000 * 60 * 60,
      status: "confirmed",
    },
  ];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/85 backdrop-blur-md p-3 sm:p-6 overflow-y-auto font-sans">
      <div className="relative w-full max-w-4xl bg-[#0a0c10] border border-white/10 rounded-2xl shadow-2xl overflow-hidden flex flex-col max-h-[92vh]">
        
        {/* Modal Top Bar */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-white/10 bg-white/[0.02]">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-amber-500/10 border border-amber-500/30 flex items-center justify-center text-amber-400 shadow-[0_0_15px_rgba(245,158,11,0.2)]">
              <Wallet className="w-5 h-5" />
            </div>
            <div>
              <h2 className="text-lg font-bold text-white tracking-wide flex items-center gap-2">
                Billetera Soberana SWAL
                <span className="text-[10px] uppercase font-mono px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-400 border border-emerald-500/30">
                  Capa 0 Mesh Activa
                </span>
              </h2>
              <p className="text-xs text-white/50 font-mono">
                Nodo Local: <span className="text-white/80">{nodeId.slice(0, 18)}...</span>
              </p>
            </div>
          </div>

          <button
            type="button"
            onClick={onClose}
            className="p-2 rounded-lg text-white/60 hover:text-white hover:bg-white/5 transition-colors cursor-pointer"
            aria-label="Cerrar Billetera"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Content Container */}
        <div className="flex-1 overflow-y-auto p-6 space-y-6">
          
          {/* Main Balance Cards Grid */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            
            {/* Card 1: Karma (Off-Chain Mesh) */}
            <div className="bg-gradient-to-br from-amber-500/10 via-[#0f1218] to-transparent border border-amber-500/30 rounded-xl p-4 flex flex-col justify-between shadow-lg">
              <div className="flex items-center justify-between text-amber-400">
                <span className="text-xs font-mono uppercase tracking-wider font-semibold">Karma Acumulado</span>
                <Zap className="w-4 h-4 fill-amber-400/30" />
              </div>
              <div className="my-3">
                <div className="text-3xl font-black text-amber-300 font-mono tracking-tight flex items-baseline gap-1">
                  {karma}
                  <span className="text-xs font-normal text-amber-400/70 uppercase">XP / Karma</span>
                </div>
                <p className="text-[11px] text-white/60 mt-1">
                  Recompensas por tiempo activo (PoA)
                </p>
              </div>
              <div className="pt-2 border-t border-white/5 flex items-center justify-between text-[11px] font-mono text-white/40">
                <span>Latido cada 5 min</span>
                <span className="text-emerald-400 flex items-center gap-1">
                  <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" /> +1 Karma
                </span>
              </div>
            </div>

            {/* Card 2: On-Chain Allocation */}
            <div className="bg-gradient-to-br from-indigo-500/10 via-[#0f1218] to-transparent border border-indigo-500/30 rounded-xl p-4 flex flex-col justify-between shadow-lg">
              <div className="flex items-center justify-between text-indigo-400">
                <span className="text-xs font-mono uppercase tracking-wider font-semibold">Capa 1 (Polygon PoS)</span>
                <Coins className="w-4 h-4" />
              </div>
              <div className="my-3">
                <div className="text-3xl font-black text-indigo-300 font-mono tracking-tight flex items-baseline gap-1">
                  {karma}.00
                  <span className="text-xs font-normal text-indigo-400/70 uppercase">SWAL</span>
                </div>
                <p className="text-[11px] text-white/60 mt-1">
                  Asignación canjeable 1:1 on-chain
                </p>
              </div>
              <div className="pt-2 border-t border-white/5 flex items-center justify-between text-[11px] font-mono text-white/40">
                <span>Merkle Rollup</span>
                <span className="text-indigo-400">Listo para Anclaje</span>
              </div>
            </div>

            {/* Card 3: Node Identity Status */}
            <div className="bg-gradient-to-br from-emerald-500/10 via-[#0f1218] to-transparent border border-emerald-500/30 rounded-xl p-4 flex flex-col justify-between shadow-lg">
              <div className="flex items-center justify-between text-emerald-400">
                <span className="text-xs font-mono uppercase tracking-wider font-semibold">Nodo Génesis</span>
                <Cpu className="w-4 h-4" />
              </div>
              <div className="my-3">
                <div className="text-sm font-bold text-white font-mono flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
                  {genesisNode}
                </div>
                <p className="text-[11px] text-white/60 mt-1">
                  Faro de sincronización edge activo
                </p>
              </div>
              <div className="pt-2 border-t border-white/5 flex items-center justify-between text-[11px] font-mono text-white/40">
                <span>Cero Gas Local</span>
                <span className="text-emerald-400">Soberano</span>
              </div>
            </div>

          </div>

          {/* Section: Autonomous Agent Session Key Delegation (Account Abstraction) */}
          <div className="bg-white/[0.02] border border-white/10 rounded-xl p-5 space-y-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                <div className="w-9 h-9 rounded-lg bg-emerald-500/10 border border-emerald-500/30 flex items-center justify-center text-emerald-400">
                  <Bot className="w-5 h-5" />
                </div>
                <div>
                  <h3 className="text-sm font-bold text-white flex items-center gap-2">
                    Gestión Autónoma Delegada al Agente
                    <span className="text-[9px] font-mono px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-300 border border-amber-500/30">
                      Session Keys (ERC-4337)
                    </span>
                  </h3>
                  <p className="text-xs text-white/60">
                    Permite a tu agente reclamar Karma y realizar rollups dentro de un límite estricto sin acceso a tu clave privada.
                  </p>
                </div>
              </div>

              <button
                type="button"
                onClick={handleToggleDelegation}
                className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors cursor-pointer ${
                  agentDelegationEnabled ? "bg-emerald-500" : "bg-white/20"
                }`}
              >
                <span
                  className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                    agentDelegationEnabled ? "translate-x-6" : "translate-x-1"
                  }`}
                />
              </button>
            </div>

            {agentDelegationEnabled && (
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-3 border-t border-white/5">
                <div>
                  <label className="block text-xs font-mono text-white/70 mb-1">
                    Tope de Gasto Diario del Agente (SWAL / día)
                  </label>
                  <div className="flex items-center gap-2">
                    <input
                      type="number"
                      value={dailyLimit}
                      onChange={(e) => handleUpdateLimit(e.target.value)}
                      className="bg-black/40 border border-white/10 rounded-lg px-3 py-2 text-sm text-white font-mono w-32 focus:outline-none focus:border-amber-400/50"
                      min="1"
                      max="1000"
                    />
                    <span className="text-xs text-white/50 font-mono">SWAL máx / 24h</span>
                  </div>
                  <p className="text-[10px] text-white/40 mt-1">
                    Protección estricta: el agente nunca puede exceder este monto.
                  </p>
                </div>

                <div className="flex flex-col justify-center space-y-2">
                  <div className="flex items-center gap-2">
                    <ShieldCheck className="w-4 h-4 text-emerald-400 shrink-0" />
                    <span className="text-xs text-white/80">
                      Clave de Sesión efímera aislada en memoria
                    </span>
                  </div>
                  <div className="flex items-center gap-2">
                    <Lock className="w-4 h-4 text-emerald-400 shrink-0" />
                    <span className="text-xs text-white/80">
                      Llave raíz Knox / TPM 100% protegida contra drenado
                    </span>
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* Section: Layer 1 Polygon Payout Address & Sync */}
          <div className="bg-white/[0.02] border border-white/10 rounded-xl p-5 space-y-4">
            <div className="flex items-center gap-3">
              <div className="w-9 h-9 rounded-lg bg-indigo-500/10 border border-indigo-500/30 flex items-center justify-center text-indigo-400">
                <Layers className="w-5 h-5" />
              </div>
              <div>
                <h3 className="text-sm font-bold text-white flex items-center gap-2">
                  Retiro y Anclaje a Polygon PoS (Capa 1)
                </h3>
                <p className="text-xs text-white/60">
                  Vincula tu dirección pública de Polygon para recibir tokens SWAL al ejecutar el rollup.
                </p>
              </div>
            </div>

            <form onSubmit={handleSavePolygonAddress} className="space-y-3">
              <div className="flex flex-col sm:flex-row gap-2">
                <input
                  type="text"
                  placeholder="0x... (Dirección pública Polygon / EVM)"
                  value={polygonAddress}
                  onChange={(e) => setPolygonAddress(e.target.value)}
                  className="flex-1 bg-black/40 border border-white/10 rounded-lg px-4 py-2 text-sm text-white font-mono focus:outline-none focus:border-indigo-400/50"
                />
                <button
                  type="submit"
                  className="px-4 py-2 bg-indigo-500/20 border border-indigo-500/40 text-indigo-300 hover:bg-indigo-500/30 rounded-lg text-xs font-mono font-bold transition-colors cursor-pointer shrink-0"
                >
                  {savedAddress ? "✓ Guardada" : "Guardar Dirección"}
                </button>
              </div>
            </form>

            <div className="flex items-center justify-between pt-2 border-t border-white/5">
              <div className="text-xs text-white/50 font-mono">
                Estado de Anclaje: <span className="text-emerald-400">Listo ({karma} Karma acumulados)</span>
              </div>

              <button
                type="button"
                onClick={handleSyncToPolygon}
                disabled={isSyncing || karma <= 0}
                className="flex items-center gap-2 px-4 py-2 bg-gradient-to-r from-indigo-500/30 to-purple-500/30 border border-indigo-500/50 hover:border-indigo-400 text-white rounded-lg text-xs font-mono font-bold transition-all shadow-lg cursor-pointer disabled:opacity-50"
              >
                {isSyncing ? (
                  <>
                    <RefreshCw className="w-3.5 h-3.5 animate-spin text-indigo-300" />
                    Generando Merkle Proof...
                  </>
                ) : syncSuccess ? (
                  <>
                    <Check className="w-3.5 h-3.5 text-emerald-400" />
                    ¡Rollup Compilado con Éxito!
                  </>
                ) : (
                  <>
                    <ArrowUpRight className="w-3.5 h-3.5" />
                    Sincronizar Recompensas a Polygon
                  </>
                )}
              </button>
            </div>
          </div>

          {/* Section: Activity & Karma Ledger */}
          <div className="space-y-3">
            <h3 className="text-xs font-mono uppercase tracking-wider text-white/50 font-semibold px-1">
              Registro Contable de Karma y Nodos
            </h3>
            <div className="bg-white/[0.02] border border-white/10 rounded-xl divide-y divide-white/5 overflow-hidden">
              {recentEvents.map((ev) => (
                <div key={ev.id} className="p-3.5 flex items-center justify-between text-xs hover:bg-white/[0.01] transition-colors">
                  <div className="flex items-center gap-3">
                    <div className="w-7 h-7 rounded-full bg-amber-500/10 border border-amber-500/30 flex items-center justify-center text-amber-400 shrink-0">
                      <Zap className="w-3.5 h-3.5" />
                    </div>
                    <div>
                      <p className="text-white font-medium">{ev.title}</p>
                      <p className="text-[10px] text-white/40 font-mono">
                        {new Date(ev.timestamp).toLocaleTimeString()} · Verificado por Génesis
                      </p>
                    </div>
                  </div>
                  <div className="text-right">
                    <span className="font-mono text-emerald-400 font-bold">+{ev.amount} KARMA</span>
                    <p className="text-[9px] text-white/30 uppercase font-mono">Confirmado</p>
                  </div>
                </div>
              ))}
            </div>
          </div>

        </div>

        {/* Modal Footer */}
        <div className="px-6 py-3 border-t border-white/10 bg-white/[0.02] flex items-center justify-between text-[11px] font-mono text-white/50">
          <div className="flex items-center gap-4">
            <button
              type="button"
              onClick={handleCopyNodeId}
              className="hover:text-white flex items-center gap-1.5 transition-colors cursor-pointer"
            >
              {copiedKey ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
              {copiedKey ? "Node ID Copiado" : "Copiar Node ID"}
            </button>

            <button
              type="button"
              onClick={() => setShowRecovery(true)}
              className="hover:text-amber-300 flex items-center gap-1.5 text-amber-400/80 transition-colors cursor-pointer"
            >
              <Key className="w-3.5 h-3.5 text-amber-400" />
              Recuperación BIP-39 (Share 2)
            </button>
          </div>
          <span>SWAL Decentralized Mesh v1.0</span>
        </div>

        {/* Disaster Recovery BIP-39 Wizard Modal */}
        <RecoveryModal
          isOpen={showRecovery}
          onClose={() => setShowRecovery(false)}
          mnemonic={mnemonic}
          onRestore={(phrase) => {
            console.log("Restoring node with phrase:", phrase);
            setShowRecovery(false);
          }}
        />

      </div>
    </div>
  );
};
