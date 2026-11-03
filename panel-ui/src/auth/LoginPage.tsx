import { motion } from "motion/react";
import type React from "react";
import { useState } from "react";
import ParticleBackground from "../components/ParticleBackground";

/** Response shape returned by the passwordless magic-link endpoint. */
interface MagicLinkResponse {
  ok: boolean;
  magic_url?: string;
  error?: string;
}

export const LoginPage: React.FC = () => {
  const [email, setEmail] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
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
          body: JSON.stringify({ email }),
        },
      );
      const data = (await res.json()) as MagicLinkResponse;
      if (res.ok && data.ok) {
        if (data.magic_url) {
          setStatus("MAGIC LINK GENERATED - REDIRECTING...");
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
          Passwordless · zero-knowledge magic link
        </p>

        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
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
        </form>

        <p className="mt-8 text-[10px] text-white/40 text-center uppercase tracking-widest leading-relaxed">
          No passwords. No accounts to recover. A one-time link is sent to your
          inbox.
        </p>
      </motion.div>
    </div>
  );
};
