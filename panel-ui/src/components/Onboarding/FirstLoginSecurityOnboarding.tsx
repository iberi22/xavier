import React, { useState } from "react";
import { Shield, KeyRound, ArrowRight } from "lucide-react";
import { useAuthStore } from "../../auth/AuthProvider";
import { TwoFactorSetup } from "../../auth/TwoFactorSetup";

interface Props {
  onComplete: () => void;
}

export const FirstLoginSecurityOnboarding: React.FC<Props> = ({ onComplete }) => {
  const user = useAuthStore((state) => state.user);
  const [step, setStep] = useState<"welcome" | "2fa_setup">("welcome");

  const handleFinish = () => {
    try {
      localStorage.setItem("xavier_security_onboarding_completed", "true");
    } catch {
      // storage unavailable
    }
    onComplete();
  };

  if (step === "2fa_setup") {
    return (
      <div className="fixed inset-0 z-50 bg-[#050505] flex flex-col items-center justify-center p-4">
        <div className="w-full max-w-md relative flex flex-col items-center">
          <TwoFactorSetup />
          <button
            onClick={handleFinish}
            data-testid="onboarding-abort-2fa-btn"
            className="mt-4 text-xs text-white/50 hover:text-white underline font-mono tracking-widest uppercase transition-colors"
          >
            Omitir por ahora y entrar al Dashboard
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 z-50 bg-[#050505]/95 backdrop-blur-md flex items-center justify-center p-4 font-mono text-white">
      <div className="w-full max-w-lg bg-[#0a0a0a] border border-emerald-500/30 rounded-2xl p-8 shadow-[0_0_30px_rgba(16,185,129,0.15)] flex flex-col gap-6 animate-in fade-in zoom-in-95 duration-200">
        <div className="flex items-center gap-3 border-b border-white/10 pb-4">
          <div className="p-3 bg-emerald-500/10 border border-emerald-500/30 rounded-xl text-emerald-400">
            <Shield className="w-6 h-6" />
          </div>
          <div>
            <h2 className="text-lg font-bold uppercase tracking-wider text-emerald-400">
              Bienvenido a Xavier OS
            </h2>
            <p className="text-xs text-white/60">
              Configuración Inicial de Seguridad y Operador
            </p>
          </div>
        </div>

        <div className="space-y-4 text-xs leading-relaxed text-white/80">
          <div className="bg-white/5 border border-white/10 rounded-xl p-4 space-y-2">
            <div className="flex justify-between items-center text-white/60">
              <span>Operador Autenticado:</span>
              <span className="font-bold text-white">{user?.email || "operador@swal.network"}</span>
            </div>
            <div className="flex justify-between items-center text-white/60">
              <span>Tenant / Red:</span>
              <span className="text-emerald-400 font-bold">{user?.id || "swal"}</span>
            </div>
            <div className="flex justify-between items-center text-white/60">
              <span>Motor de Embeddings:</span>
              <span className="text-emerald-400 font-bold">Gemma 2 768d (In-Process)</span>
            </div>
          </div>

          <div className="bg-emerald-950/20 border border-emerald-500/20 rounded-xl p-4 space-y-2">
            <div className="flex items-center gap-2 text-emerald-400 font-bold uppercase text-[11px] tracking-wider">
              <KeyRound className="w-4 h-4" /> Recomendación de Seguridad
            </div>
            <p className="text-[11px] text-white/70">
              Protege tu acceso y la soberanía de tu nodo activando la <strong>Autenticación en Dos Pasos (2FA TOTP)</strong>. Es compatible con Google Authenticator, 1Password y Authy.
            </p>
          </div>
        </div>

        <div className="flex flex-col sm:flex-row gap-3 pt-2">
          <button
            onClick={() => setStep("2fa_setup")}
            data-testid="onboarding-setup-2fa-btn"
            className="flex-1 bg-emerald-500 hover:bg-emerald-400 text-black font-bold py-3 px-4 rounded-xl text-xs uppercase tracking-wider transition-colors flex items-center justify-center gap-2 shadow-lg shadow-emerald-500/20"
          >
            <KeyRound className="w-4 h-4" /> Configurar 2FA Ahora
          </button>
          <button
            onClick={handleFinish}
            data-testid="onboarding-skip-2fa-btn"
            className="flex-1 bg-white/5 hover:bg-white/10 text-white/70 hover:text-white border border-white/10 font-bold py-3 px-4 rounded-xl text-xs uppercase tracking-wider transition-colors flex items-center justify-center gap-2"
          >
            Configurar Más Tarde <ArrowRight className="w-4 h-4" />
          </button>
        </div>
      </div>
    </div>
  );
};
