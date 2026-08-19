import { X } from "lucide-react";
import React, { useEffect, useRef, useState } from "react";
import "./maloca.css";

type Props = {
  onClose?: () => void;
  /** Scaffold: treat local session as manager ACL (no vote weight). */
  isManager?: boolean;
};

export default function MalocaView({ onClose, isManager = true }: Props) {
  const [error, setError] = useState<string | null>(null);
  const [_isReady, setIsReady] = useState(false);
  const panelRef = useRef<HTMLElement | null>(null);

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
        </header>

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
