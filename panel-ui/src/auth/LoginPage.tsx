import { motion } from "motion/react";
import type React from "react";
import { useEffect, useRef, useState } from "react";
import ParticleBackground from "../components/ParticleBackground";
import { PasswordInput } from "../components/PasswordInput";
import { TwoFactorInput } from "../components/TwoFactorInput";
import { useAuth } from "../hooks/useAuth";
import { useAuthStore } from "./AuthProvider";

/** Response shape returned by the passwordless magic-link endpoint. */
interface MagicLinkResponse {
  ok: boolean;
  magic_url?: string;
  error?: string;
}

declare global {
  interface Window {
    turnstile?: {
      render: (
        container: string | HTMLElement,
        options: {
          sitekey: string;
          callback: (token: string) => void;
          "error-callback"?: () => void;
          "expired-callback"?: () => void;
          theme?: "dark" | "light" | "auto";
          size?: "normal" | "compact" | "invisible";
        },
      ) => string;
      reset: (widgetId: string) => void;
    };
  }
}

export const LoginPage: React.FC = () => {
  const [mode, setMode] = useState<"magic" | "password">("magic");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [totpCode, setTotpCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [turnstileToken, setTurnstileToken] = useState<string | null>(null);
  const turnstileContainerRef = useRef<HTMLDivElement>(null);
  const turnstileWidgetId = useRef<string | null>(null);

  const { login, requires2FA } = useAuth();

  useEffect(() => {
    const siteKey =
      import.meta.env?.VITE_TURNSTILE_SITE_KEY || "1x00000000000000000000AA"; // Cloudflare official test key

    const renderWidget = () => {
      if (
        window.turnstile &&
        turnstileContainerRef.current &&
        !turnstileWidgetId.current
      ) {
        try {
          turnstileWidgetId.current = window.turnstile.render(
            turnstileContainerRef.current,
            {
              sitekey: siteKey,
              theme: "dark",
              size: "normal",
              callback: (token: string) => setTurnstileToken(token),
              "expired-callback": () => setTurnstileToken(null),
              "error-callback": () => setTurnstileToken(null),
            },
          );
        } catch {}
      }
    };

    if (window.turnstile) {
      renderWidget();
    } else if (typeof document !== "undefined") {
      const existing = document.getElementById("cf-turnstile-script");
      if (!existing) {
        const script = document.createElement("script");
        script.id = "cf-turnstile-script";
        script.src =
          "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
        script.async = true;
        script.onload = () => renderWidget();
        document.head.appendChild(script);
      }
    }
  }, []);

  const handleMagicSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setStatus(null);
    setIsLoading(true);
    try {
      const res = await fetch(
        "https://xaviercloud.swal.network/auth/magic-link",
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            email,
            turnstile_token: turnstileToken || undefined,
          }),
        },
      );
      const data = (await res.json()) as MagicLinkResponse;
      if (res.ok && data.ok) {
        if (data.magic_url) {
          setStatus("MAGIC LINK GENERATED - REDIRECTING...");
          try {
            const url = new URL(data.magic_url, window.location.origin);
            const h = url.hash;
            if (h.startsWith("#/session")) {
              const q = h.indexOf("?");
              if (q >= 0) {
                const params = new URLSearchParams(h.slice(q + 1));
                const sessionToken = params.get("token");
                const mail = params.get("email");
                const tenant = params.get("tenant");
                if (sessionToken) {
                  useAuthStore.getState().setSession(sessionToken, mail || undefined, tenant || undefined);
                }
              }
            }
          } catch {}
          window.location.href = data.magic_url;
        } else {
          setError("Magic link generated but no redirect URL was returned");
        }
      } else {
        setError(data.error ?? "Failed to generate magic link");
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to generate magic link",
      );
    } finally {
      setIsLoading(false);
    }
  };

  const handlePasswordSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsLoading(true);
    setError(null);
    try {
      await login(email, password, totpCode || undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to sign in");
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="w-full h-screen bg-[#050505] flex items-center justify-center text-white font-mono relative overflow-hidden">
      <ParticleBackground />
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="z-10 bg-black/60 backdrop-blur-md p-8 rounded-2xl border border-white/10 max-w-md w-full"
      >
        <h1 className="text-xl mb-2 font-bold tracking-widest text-[#39ff14]">
          XAVIER LOGIN
        </h1>
        <p className="text-xs opacity-60 mb-8 leading-relaxed uppercase tracking-tighter">
          Connect to the secure cognitive core
        </p>

        {mode === "magic" ? (
          <form onSubmit={handleMagicSubmit} className="flex flex-col gap-4">
            <div className="flex flex-col gap-1 w-full">
              <label
                htmlFor="email-input"
                className="text-xs text-white/60 uppercase tracking-widest"
              >
                Email
              </label>
              <input
                id="email-input"
                className="w-full bg-white/5 border border-white/10 rounded-lg p-3 text-sm focus:border-[#39ff14] focus:outline-none transition-colors font-mono"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="operator@xavier.local"
                type="email"
                required
                autoComplete="email"
                disabled={isLoading}
              />
            </div>

            <div
              id="turnstile-container"
              ref={turnstileContainerRef}
              data-testid="turnstile-container"
              className="my-1 flex justify-center min-h-[65px]"
            />

            {error && (
              <div className="text-red-500 text-[10px] uppercase tracking-widest bg-red-500/10 border border-red-500/20 p-2 rounded-lg">
                {error}
              </div>
            )}

            {status && (
              <div className="text-[#39ff14] text-[10px] uppercase tracking-widest bg-[#39ff14]/10 border border-[#39ff14]/30 p-2 rounded-lg text-center">
                {status}
              </div>
            )}

            <button
              type="submit"
              disabled={isLoading}
              className="w-full bg-[#39ff14] text-black font-bold text-sm tracking-widest py-3 rounded-lg hover:shadow-[0_0_15px_rgba(57,255,20,0.5)] transition-all mt-4 disabled:opacity-50"
            >
              {isLoading ? "GENERATING..." : "INITIALIZE SESSION"}
            </button>

            <div className="mt-4 flex flex-col items-center">
              <button
                type="button"
                onClick={() => {
                  setError(null);
                  setStatus(null);
                  setMode("password");
                }}
                className="text-[11px] text-[#39ff14]/80 hover:text-[#39ff14] transition-colors underline uppercase tracking-wider"
              >
                Otros métodos de acceso (Contraseña / 2FA)
              </button>
            </div>
          </form>
        ) : (
          <form onSubmit={handlePasswordSubmit} className="flex flex-col gap-4">
            <div className="flex flex-col gap-1 w-full">
              <label
                htmlFor="email-input"
                className="text-xs text-white/60 uppercase tracking-widest"
              >
                Email
              </label>
              <input
                id="email-input"
                className="w-full bg-white/5 border border-white/10 rounded-lg p-3 text-sm focus:border-[#39ff14] focus:outline-none transition-colors font-mono"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="operator@xavier.local"
                type="email"
                required
                autoComplete="email"
                disabled={isLoading}
              />
            </div>

            <PasswordInput
              id="password-input"
              label="Password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              disabled={isLoading}
            />

            {requires2FA && (
              <motion.div
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: "auto" }}
                className="flex flex-col gap-2 mt-2"
              >
                <label className="text-xs text-[#39ff14] uppercase tracking-widest text-center mb-2">
                  Enter 2FA Code
                </label>
                <TwoFactorInput
                  value={totpCode}
                  onChange={setTotpCode}
                  length={8}
                />
                <p className="text-[10px] text-white/40 text-center uppercase tracking-widest">
                  6-digit authenticator code, or an 8-digit backup code
                </p>
              </motion.div>
            )}

            {error && (
              <div className="text-red-500 text-[10px] uppercase tracking-widest bg-red-500/10 border border-red-500/20 p-2 rounded-lg">
                {error}
              </div>
            )}

            <button
              type="submit"
              disabled={isLoading}
              className="w-full bg-[#39ff14] text-black font-bold text-sm tracking-widest py-3 rounded-lg hover:shadow-[0_0_15px_rgba(57,255,20,0.5)] transition-all mt-4 disabled:opacity-50"
            >
              {isLoading ? "VERIFYING..." : "INITIALIZE SESSION"}
            </button>

            <div className="mt-4 flex flex-col gap-3 items-center">
              <button
                type="button"
                onClick={() => {
                  setError(null);
                  setStatus(null);
                  setMode("magic");
                }}
                className="text-[11px] text-white/60 hover:text-white transition-colors underline uppercase tracking-wider"
              >
                ← Volver a acceso por Magic Link
              </button>
              <div className="flex gap-4 mt-2">
                <button
                  type="button"
                  onClick={() => (window.location.hash = "#/register")}
                  className="text-[10px] text-white/40 hover:text-[#39ff14] transition-colors uppercase tracking-widest"
                >
                  Register new operator
                </button>
                <button
                  type="button"
                  onClick={() => (window.location.hash = "#/recovery")}
                  className="text-[10px] text-white/40 hover:text-[#39ff14] transition-colors uppercase tracking-widest"
                >
                  Forgot access credentials?
                </button>
              </div>
            </div>
          </form>
        )}

        <p className="mt-8 text-[10px] text-white/40 text-center uppercase tracking-widest leading-relaxed">
          {mode === "magic"
            ? "No passwords. No accounts to recover. A one-time link is sent to your inbox."
            : "Protected by Argon2id cryptographic password hashing and RFC 6238 TOTP 2FA."}
        </p>
      </motion.div>
    </div>
  );
};
