import React, { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { CheckCircle2, Loader2, Info } from "lucide-react";
import { getApiUrl } from "../api/client";

export type AuthState = "disconnected" | "connecting" | "connected";

interface ColabAuthButtonProps {
  token?: string;
  onStatusChange?: (status: AuthState, email?: string) => void;
  className?: string;
}

function GoogleLogoSvg() {
  return (
    <svg className="w-4 h-4 shrink-0" viewBox="0 0 24 24" aria-hidden="true">
      <path
        fill="#4285F4"
        d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"
      />
      <path
        fill="#34A853"
        d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"
      />
      <path
        fill="#FBBC05"
        d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.06H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.94l2.85-2.22.81-.63z"
      />
      <path
        fill="#EA4335"
        d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.06l3.66 2.84c.87-2.6 3.3-4.52 6.16-4.52z"
      />
    </svg>
  );
}

export function ColabAuthButton({
  token,
  onStatusChange,
  className = "",
}: ColabAuthButtonProps) {
  const [status, setStatus] = useState<AuthState>("disconnected");
  const [userEmail, setUserEmail] = useState<string | null>(null);
  const [showTooltip, setShowTooltip] = useState<boolean>(false);

  const getHeaders = useCallback(() => {
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (token) {
      headers["X-Xavier-Token"] = token;
    }
    const activeWorkspace =
      typeof localStorage !== "undefined"
        ? localStorage.getItem("xavier_active_workspace") || "default"
        : "default";
    headers["X-Workspace-Id"] = activeWorkspace;
    return headers;
  }, [token]);

  const checkStatus = useCallback(async () => {
    try {
      const res = await fetch(getApiUrl("/auth/google/status"), {
        headers: getHeaders(),
      });
      if (!res.ok) {
        setStatus("disconnected");
        setUserEmail(null);
        onStatusChange?.("disconnected");
        return;
      }
      const data = await res.json();
      if (data.connected || data.status === "connected") {
        const email = data.email || data.user_email || "usuario@google.com";
        setStatus("connected");
        setUserEmail(email);
        onStatusChange?.("connected", email);
      } else {
        setStatus("disconnected");
        setUserEmail(null);
        onStatusChange?.("disconnected");
      }
    } catch {
      setStatus("disconnected");
      setUserEmail(null);
      onStatusChange?.("disconnected");
    }
  }, [getHeaders, onStatusChange]);

  useEffect(() => {
    void checkStatus();
  }, [checkStatus]);

  const handleConnect = async () => {
    setStatus("connecting");
    onStatusChange?.("connecting");
    try {
      const res = await fetch(getApiUrl("/auth/google/connect"), {
        headers: getHeaders(),
      });
      if (!res.ok) {
        throw new Error("API not available");
      }
      const data = await res.json();
      const authUrl = data.auth_url || data.url;
      if (authUrl) {
        const popup = window.open(
          authUrl,
          "google_oauth",
          "width=500,height=600,scrollbars=yes,resizable=yes",
        );

        const interval = setInterval(() => {
          if (!popup || popup.closed) {
            clearInterval(interval);
            void checkStatus();
          }
        }, 1000);
      } else {
        setStatus("disconnected");
        onStatusChange?.("disconnected");
      }
    } catch {
      setStatus("disconnected");
      onStatusChange?.("disconnected");
    }
  };

  const handleDisconnect = async () => {
    setStatus("connecting");
    try {
      await fetch(getApiUrl("/auth/google/disconnect"), {
        method: "DELETE",
        headers: getHeaders(),
      });
    } catch {
      // Ignore errors when calling disconnect on missing endpoint
    } finally {
      setStatus("disconnected");
      setUserEmail(null);
      onStatusChange?.("disconnected");
    }
  };

  return (
    <div className={`relative inline-flex flex-col gap-1.5 ${className}`}>
      <div className="flex items-center gap-2 p-2 rounded-xl bg-[#0d0d12] border border-white/8 text-xs font-mono">
        <AnimatePresence mode="wait">
          {status === "disconnected" && (
            <motion.div
              key="disconnected"
              initial={{ opacity: 0, y: -4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 4 }}
              transition={{ duration: 0.15 }}
              className="flex items-center gap-2"
            >
              <button
                type="button"
                onClick={handleConnect}
                className="flex items-center gap-2 px-3.5 py-1.5 text-xs font-semibold text-white/90 bg-emerald-500/10 hover:bg-emerald-500/20 border border-emerald-500/30 rounded-lg transition-all active:scale-95 cursor-pointer"
              >
                <GoogleLogoSvg />
                <span>Conectar con Google</span>
              </button>
            </motion.div>
          )}

          {status === "connecting" && (
            <motion.div
              key="connecting"
              initial={{ opacity: 0, y: -4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 4 }}
              transition={{ duration: 0.15 }}
              className="flex items-center gap-2 px-3.5 py-1.5 text-emerald-400 font-semibold"
            >
              <Loader2 className="w-4 h-4 animate-spin text-emerald-400" />
              <span>Conectando...</span>
            </motion.div>
          )}

          {status === "connected" && (
            <motion.div
              key="connected"
              initial={{ opacity: 0, y: -4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 4 }}
              transition={{ duration: 0.15 }}
              className="flex items-center gap-2 px-2 py-1"
            >
              <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
              <span className="text-white/80 font-medium truncate max-w-[180px]" title={userEmail || ""}>
                {userEmail || "Conectado"}
              </span>
              <button
                type="button"
                onClick={handleDisconnect}
                className="ml-2 text-xs text-white/40 hover:text-rose-400 underline transition-colors cursor-pointer"
              >
                Desconectar
              </button>
            </motion.div>
          )}
        </AnimatePresence>

        <div className="relative inline-block ml-auto">
          <button
            type="button"
            onMouseEnter={() => setShowTooltip(true)}
            onMouseLeave={() => setShowTooltip(false)}
            onClick={() => setShowTooltip((prev) => !prev)}
            className="p-1 text-white/40 hover:text-white/70 transition-colors focus:outline-none rounded"
            aria-label="Información de privacidad"
          >
            <Info className="w-3.5 h-3.5" />
          </button>

          <AnimatePresence>
            {showTooltip && (
              <motion.div
                initial={{ opacity: 0, scale: 0.95, y: 5 }}
                animate={{ opacity: 1, scale: 1, y: 0 }}
                exit={{ opacity: 0, scale: 0.95, y: 5 }}
                transition={{ duration: 0.15 }}
                className="absolute right-0 bottom-full mb-2 w-64 p-2.5 bg-[#14151a] border border-white/10 rounded-lg text-[11px] text-white/70 shadow-xl z-50 pointer-events-none leading-relaxed"
              >
                Tu cuenta de Google solo se usa para acceder a Colab. No almacenamos datos en Google.
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>
    </div>
  );
}

export default ColabAuthButton;
