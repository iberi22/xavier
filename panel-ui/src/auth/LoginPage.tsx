import React, { useState } from "react";
import { motion } from "motion/react";
import { useAuth } from "../hooks/useAuth";
import { PasswordInput } from "../components/PasswordInput";
import { TwoFactorInput } from "../components/TwoFactorInput";
import ParticleBackground from "../components/ParticleBackground";

export const LoginPage: React.FC = () => {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [totpCode, setTotpCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  const { login, requires2FA } = useAuth();

  const handleSubmit = async (e: React.FormEvent) => {
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

        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          <div className="flex flex-col gap-1 w-full">
            <label htmlFor="email-input" className="text-xs text-white/60 uppercase tracking-widest">Email</label>
            <input
              id="email-input"
              className="w-full bg-white/5 border border-white/10 rounded-lg p-3 text-sm focus:border-[#39ff14] focus:outline-none transition-colors font-mono"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="operator@xavier.local"
              type="email"
              required
            />
          </div>

          <PasswordInput
            id="password-input"
            label="Password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
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
              <TwoFactorInput value={totpCode} onChange={setTotpCode} />
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
        </form>

        <div className="mt-8 flex flex-col gap-3">
          <button
            type="button"
            onClick={() => window.location.hash = "#/register"}
            className="text-[10px] text-white/40 hover:text-[#39ff14] transition-colors uppercase tracking-widest"
          >
            Register new operator
          </button>
          <button
             type="button"
             onClick={() => window.location.hash = "#/recovery"}
             className="text-[10px] text-white/40 hover:text-[#39ff14] transition-colors uppercase tracking-widest"
          >
            Forgot access credentials?
          </button>
        </div>
      </motion.div>
    </div>
  );
};
