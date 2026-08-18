import { KeyRound, X } from "lucide-react";
import React, { useEffect, useRef, useState } from "react";
import { obtainDeviceKeyViaWebAuthn } from "./webauthn";
import "./maloca.css";

type Props = {
  onClose?: () => void;
  /** Scaffold: treat local session as manager ACL (no vote weight). */
  isManager?: boolean;
};

export default function MalocaView({ onClose, isManager = true }: Props) {
  const [error, setError] = useState<string | null>(null);
  const [_isReady, setIsReady] = useState(false);
  const [deviceKey, setDeviceKey] = useState<string | null>(null);
  const [isWebAuthnLoading, setIsWebAuthnLoading] = useState(false);
  const panelRef = useRef<HTMLElement | null>(null);

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

  useEffect(() => {
    // Fallback dynamic import of the Custom Element if the sandbox lacks direct access
    import(/* @vite-ignore */ "@swal/maloca-embed")
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

  const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const xavierUrl = isTauri ? "http://127.0.0.1:8006" : (window.location.origin || "http://127.0.0.1:8006");

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
            className="maloca-btn maloca-close"
            onClick={onClose}
            aria-label="Cerrar Maloca"
          >
            <X size={16} />
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
              className="maloca-btn flex items-center gap-1.5 px-3 py-1.5 text-xs font-mono bg-slate-800 hover:bg-slate-700 text-slate-200 border border-slate-700 rounded transition-colors"
              onClick={handleObtainWebAuthnKey}
              disabled={isWebAuthnLoading}
              title="Obtener Clave de Dispositivo vía WebAuthn PRF"
            >
              <KeyRound size={14} />
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

        <div className="maloca-panel mt-4">
          <swal-maloca-panel
            ref={panelRef as any}
            app-id="xavier"
            xavier-url={xavierUrl}
          />
        </div>
      </div>
    </div>
  );
}
