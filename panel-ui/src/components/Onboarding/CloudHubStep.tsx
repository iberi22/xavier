import React, { useState } from "react";
import {
  Apple,
  Terminal,
  Download,
  KeyRound,
  Network,
  Copy,
  Check,
  ShieldCheck,
  Layers,
  Bot,
  ExternalLink,
} from "lucide-react";
import { BorderedIcon } from "../ui/BorderedIcon";
import { ThemedButton } from "../ui/ThemedButton";

interface CloudHubStepProps {
  onNext: () => void;
  partnerAlias: string;
  onChangePartnerAlias: (val: string) => void;
  mcpEnabled: boolean;
  onChangeMcpEnabled: (val: boolean) => void;
}

export const CloudHubStep: React.FC<CloudHubStepProps> = ({
  onNext,
  partnerAlias,
  onChangePartnerAlias,
  mcpEnabled,
  onChangeMcpEnabled,
}) => {
  const [copied, setCopied] = useState(false);
  const installCmd = "curl -sSL https://raw.githubusercontent.com/iberi22/xavier/main/scripts/install.sh | bash";

  const handleCopy = () => {
    navigator.clipboard.writeText(installCmd);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="space-y-6 animate-in fade-in duration-500 max-w-2xl mx-auto">
      <div className="text-center space-y-1">
        <div className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-full text-xs font-mono bg-blue-500/10 text-blue-500 border border-blue-500/20 mb-1">
          <ShieldCheck className="w-3 h-3" /> NODO CLOUD & RED DE DESPACHO
        </div>
        <h2 className="text-xl font-bold tracking-tight text-[var(--foreground,#f3f3f5)]">
          Xavier Enterprise & Sesión Soberana
        </h2>
        <p className="text-xs text-[var(--foreground-muted,#8f919a)]">
          Descarga el nodo daemon para tu hardware o inicia una sesión online como Sub-Socio SWAL.
        </p>
      </div>

      {/* 1. Descarga de Binarios Nativos */}
      <div className="p-4 rounded-xl bg-[var(--surface,#141518)] border border-[var(--border-subtle,rgba(255,255,255,0.07))] space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <BorderedIcon size="sm" active>
              <Download className="w-3.5 h-3.5 text-blue-500" />
            </BorderedIcon>
            <div>
              <h3 className="text-sm font-semibold text-[var(--foreground,#f3f3f5)]">
                1. Binarios Nativos Precompilados (Sin Compilar)
              </h3>
              <p className="text-[11px] text-[var(--foreground-muted,#8f919a)]">
                Para servidores on-premise, workstations (macOS/Linux) y nodos soberanos
              </p>
            </div>
          </div>
          <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/10 text-emerald-500 border border-emerald-500/20">
            Release v0.2.0
          </span>
        </div>

        <div className="grid grid-cols-3 gap-2 pt-1">
          <a
            href="https://github.com/iberi22/xavier/releases/download/v0.2.0/xavier-v0.2.0-aarch64-apple-darwin.tar.gz"
            className="flex flex-col items-center p-2.5 rounded-lg bg-[var(--surface-card,#1b1c20)] hover:bg-[var(--surface-elevated,#23252a)] border border-[var(--border-subtle,rgba(255,255,255,0.06))] hover:border-blue-500/40 transition-all text-center group"
          >
            <Apple className="w-5 h-5 text-[var(--foreground,#f3f3f5)] opacity-80 group-hover:text-blue-500 mb-1" />
            <span className="text-xs font-medium text-[var(--foreground,#f3f3f5)]">macOS Silicon</span>
            <span className="text-[10px] text-[var(--foreground-muted,#8f919a)]">M1 / M2 / M3 / M4</span>
          </a>

          <a
            href="https://github.com/iberi22/xavier/releases/tag/v0.2.0"
            className="flex flex-col items-center p-2.5 rounded-lg bg-[var(--surface-card,#1b1c20)] hover:bg-[var(--surface-elevated,#23252a)] border border-[var(--border-subtle,rgba(255,255,255,0.06))] hover:border-blue-500/40 transition-all text-center group"
          >
            <Apple className="w-5 h-5 text-[var(--foreground,#f3f3f5)] opacity-80 group-hover:text-blue-500 mb-1" />
            <span className="text-xs font-medium text-[var(--foreground,#f3f3f5)]">macOS Intel</span>
            <span className="text-[10px] text-[var(--foreground-muted,#8f919a)]">x86_64 Darwin</span>
          </a>

          <a
            href="https://github.com/iberi22/xavier/releases/download/v0.2.0/xavier-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"
            className="flex flex-col items-center p-2.5 rounded-lg bg-[var(--surface-card,#1b1c20)] hover:bg-[var(--surface-elevated,#23252a)] border border-[var(--border-subtle,rgba(255,255,255,0.06))] hover:border-blue-500/40 transition-all text-center group"
          >
            <Terminal className="w-5 h-5 text-[var(--foreground,#f3f3f5)] opacity-80 group-hover:text-blue-500 mb-1" />
            <span className="text-xs font-medium text-[var(--foreground,#f3f3f5)]">Linux x86_64</span>
            <span className="text-[10px] text-[var(--foreground-muted,#8f919a)]">Ubuntu / Debian / Nix</span>
          </a>
        </div>

        {/* 1-Liner command */}
        <div className="flex items-center justify-between p-2 rounded-lg bg-[var(--background,#0d0e10)] border border-[var(--border-subtle,rgba(255,255,255,0.05))] font-mono text-[11px] text-[var(--foreground-muted,#8f919a)]">
          <span className="truncate pr-2 select-all text-blue-500/90">{installCmd}</span>
          <button
            type="button"
            onClick={handleCopy}
            className="inline-flex items-center gap-1 px-2 py-1 rounded bg-[var(--surface-card,#1b1c20)] hover:bg-[var(--surface-elevated,#23252a)] text-[var(--foreground,#f3f3f5)] border border-[var(--border-subtle,rgba(255,255,255,0.08))] text-[10px] shrink-0 transition-colors"
          >
            {copied ? <Check className="w-3 h-3 text-emerald-500" /> : <Copy className="w-3 h-3" />}
            {copied ? "Copiado" : "Copiar"}
          </button>
        </div>
      </div>

      {/* 2. Sesión Sub-Socio SWAL */}
      <div className="p-4 rounded-xl bg-[var(--surface,#141518)] border border-[var(--border-subtle,rgba(255,255,255,0.07))] space-y-3">
        <div className="flex items-center gap-2">
          <BorderedIcon size="sm">
            <KeyRound className="w-3.5 h-3.5 text-amber-500" />
          </BorderedIcon>
          <div>
            <h3 className="text-sm font-semibold text-[var(--foreground,#f3f3f5)]">
              2. Sesión Online Sub-Socio SWAL
            </h3>
            <p className="text-[11px] text-[var(--foreground-muted,#8f919a)]">
              Identidad soberana para usuarios y agentes de la organización
            </p>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-3 pt-1">
          <div>
            <label className="block text-[11px] font-medium text-[var(--foreground-muted,#8f919a)] mb-1">
              Alias de Socio / Organización
            </label>
            <input
              type="text"
              placeholder="ej. socio_principal_empresa"
              value={partnerAlias}
              onChange={(e) => onChangePartnerAlias(e.target.value)}
              className="w-full px-3 py-1.5 rounded-lg bg-[var(--surface-card,#1b1c20)] border border-[var(--border-subtle,rgba(255,255,255,0.08))] text-xs text-[var(--foreground,#f3f3f5)] placeholder:text-[var(--foreground-muted,#8f919a)]/40 focus:outline-none focus:border-blue-500 transition-colors font-mono"
            />
          </div>

          <div>
            <label className="block text-[11px] font-medium text-[var(--foreground-muted,#8f919a)] mb-1">
              Compartimento / Rol Inicial
            </label>
            <div className="px-3 py-1.5 rounded-lg bg-[var(--surface-card,#1b1c20)] border border-[var(--border-subtle,rgba(255,255,255,0.08))] text-xs text-[var(--foreground-muted,#8f919a)] flex items-center justify-between">
              <span>default_internal + secure</span>
              <span className="text-[10px] text-emerald-500 font-mono">E2EE ACTIVO</span>
            </div>
          </div>
        </div>
      </div>

      {/* 3. MCP & Private Mesh Sync */}
      <div className="p-4 rounded-xl bg-[var(--surface,#141518)] border border-[var(--border-subtle,rgba(255,255,255,0.07))] space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <BorderedIcon size="sm">
              <Network className="w-3.5 h-3.5 text-blue-500" />
            </BorderedIcon>
            <div>
              <h3 className="text-sm font-semibold text-[var(--foreground,#f3f3f5)]">
                3. Private Mesh & MCP Session
              </h3>
              <p className="text-[11px] text-[var(--foreground-muted,#8f919a)]">
                Sincronización P2P Iroh con el nodo central y acceso a herramientas RAG
              </p>
            </div>
          </div>

          <label className="relative inline-flex items-center cursor-pointer">
            <input
              type="checkbox"
              checked={mcpEnabled}
              onChange={(e) => onChangeMcpEnabled(e.target.checked)}
              className="sr-only peer"
            />
            <div className="w-9 h-5 bg-[var(--surface-card,#1b1c20)] peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-blue-600 border border-[var(--border-subtle,rgba(255,255,255,0.1))]" />
          </label>
        </div>

        <div className="p-2.5 rounded-lg bg-[var(--background,#0d0e10)] border border-[var(--border-subtle,rgba(255,255,255,0.05))] text-[11px] text-[var(--foreground-muted,#8f919a)] flex items-center gap-2">
          <Bot className="w-4 h-4 text-emerald-500 shrink-0" />
          <span>
            Habilita el protocolo abierto MCP para que cualquier agente o arnés autónomo pueda consultar y crear memorias cognitivas en tiempo real.
          </span>
        </div>
      </div>

      {/* Botón de Continuación */}
      <div className="flex items-center justify-between pt-2">
        <span className="text-[11px] text-[var(--foreground-muted,#8f919a)] flex items-center gap-1">
          <Layers className="w-3.5 h-3.5 text-blue-500" /> Paso 2 de 4: Configuración de Nodo
        </span>
        <ThemedButton variant="primary" onClick={onNext}>
          Continuar a Integraciones →
        </ThemedButton>
      </div>
    </div>
  );
};
