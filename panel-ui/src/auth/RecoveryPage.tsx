import React, { useState } from "react";
import { motion } from "motion/react";
import { PasswordInput } from "../components/PasswordInput";
import ParticleBackground from "../components/ParticleBackground";
import { authClient } from "../api/authClient";

export const RecoveryPage: React.FC = () => {
  const [email, setEmail] = useState("");
  const [seedPhrase, setSeedPhrase] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [isLoading, setIsLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (newPassword !== confirmPassword) {
      setError("Passwords do not match");
      return;
    }

    setIsLoading(true);
    setError(null);
    try {
      await authClient.recover(email, seedPhrase.trim(), newPassword);
      setSuccess(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Recovery failed");
    } finally {
      setIsLoading(false);
    }
  };

  if (success) {
    return (
      <div className="w-full h-dvh bg-[#050505] flex items-center justify-center text-white font-mono relative overflow-hidden">
        <ParticleBackground />
        <motion.div
          initial={{ opacity: 0, scale: 0.95 }}
          animate={{ opacity: 1, scale: 1 }}
          className="z-10 bg-black/60 backdrop-blur-md p-8 rounded-2xl border border-[#39ff14]/30 max-w-md w-full text-center"
        >
          <h1 className="text-xl mb-4 font-bold tracking-widest text-[#39ff14] uppercase">
            Access Restored
          </h1>
          <p className="text-xs opacity-70 mb-8 leading-relaxed uppercase">
            Your credentials have been reset. You can now log in with your new password.
          </p>
          <button
            type="button"
            className="w-full bg-[#39ff14] text-black font-bold text-sm tracking-widest py-3 rounded-lg hover:shadow-[0_0_15px_rgba(57,255,20,0.5)] transition-all"
            onClick={() => window.location.hash = "#/login"}
          >
            BACK TO LOGIN
          </button>
        </motion.div>
      </div>
    );
  }

  return (
    <div className="w-full h-dvh bg-[#050505] flex items-center justify-center text-white font-mono relative overflow-hidden">
      <ParticleBackground />
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="z-10 bg-black/60 backdrop-blur-md p-8 rounded-2xl border border-white/10 max-w-md w-full"
      >
        <h1 className="text-xl mb-2 font-bold tracking-widest text-[#39ff14] uppercase">
          Account Recovery
        </h1>
        <p className="text-[10px] opacity-60 mb-8 leading-relaxed uppercase tracking-widest">
          Use your 12-word seed phrase to reset access
        </p>

        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          <div className="flex flex-col gap-1 w-full">
            <label className="text-xs text-white/60 uppercase tracking-widest">Email</label>
            <input
              className="w-full bg-white/5 border border-white/10 rounded-lg p-3 text-sm focus:border-[#39ff14] focus:outline-none transition-colors font-mono"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="operator@xavier.local"
              type="email"
              required
            />
          </div>

          <div className="flex flex-col gap-1 w-full">
            <label className="text-xs text-white/60 uppercase tracking-widest">Seed Phrase</label>
            <textarea
              className="w-full bg-white/5 border border-white/10 rounded-lg p-3 text-sm focus:border-[#39ff14] focus:outline-none transition-colors font-mono min-h-[100px] resize-none"
              value={seedPhrase}
              onChange={(e) => setSeedPhrase(e.target.value)}
              placeholder="Enter your 12 words here..."
              required
            />
          </div>

          <PasswordInput
            label="New Password"
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
            required
          />

          <PasswordInput
            label="Confirm New Password"
            value={confirmPassword}
            onChange={(e) => setConfirmPassword(e.target.value)}
            required
          />

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
            {isLoading ? "RECOVERING..." : "RESET PASSWORD"}
          </button>
        </form>

        <div className="mt-8">
          <button
            type="button"
            onClick={() => window.location.hash = "#/login"}
            className="text-[10px] text-white/40 hover:text-[#39ff14] transition-colors uppercase tracking-widest"
          >
            Back to login
          </button>
        </div>
      </motion.div>
    </div>
  );
};
