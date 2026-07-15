import React, { useState } from "react";
import { motion } from "motion/react";
import { useAuth } from "../hooks/useAuth";
import { PasswordInput } from "../components/PasswordInput";
import { SeedPhraseDisplay } from "../components/SeedPhraseDisplay";
import ParticleBackground from "../components/ParticleBackground";
import { authClient } from "../api/authClient";

export const RegisterPage: React.FC = () => {
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  const getPasswordStrength = (pass: string) => {
    let score = 0;
    if (pass.length >= 8) score++;
    if (/[A-Z]/.test(pass)) score++;
    if (/[0-9]/.test(pass)) score++;
    if (/[^A-Za-z0-9]/.test(pass)) score++;
    return score;
  };

  const strength = getPasswordStrength(password);
  const [isLoading, setIsLoading] = useState(false);
  const [seedPhrase, setSeedPhrase] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (password !== confirmPassword) {
      setError("Passwords do not match");
      return;
    }

    if (password.length < 8 || !/[A-Z]/.test(password) || !/[0-9]/.test(password)) {
      setError("Password must be 8+ chars, with 1 uppercase & 1 number");
      return;
    }

    setIsLoading(true);
    setError(null);
    try {
      const response = await authClient.register(email, name, password);
      setSeedPhrase(response.seed_phrase);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Registration failed");
    } finally {
      setIsLoading(false);
    }
  };

  if (seedPhrase) {
    return (
      <div className="w-full h-screen bg-[#050505] flex items-center justify-center text-white font-mono relative overflow-hidden">
        <ParticleBackground />
        <motion.div
          initial={{ opacity: 0, scale: 0.95 }}
          animate={{ opacity: 1, scale: 1 }}
          className="z-10 bg-black/60 backdrop-blur-md p-8 rounded-2xl border border-[#39ff14]/30 max-w-md w-full"
        >
          <h1 className="text-xl mb-4 font-bold tracking-widest text-[#39ff14] uppercase">
            Emergency Recovery Seed
          </h1>
          <p className="text-xs opacity-70 mb-6 leading-relaxed">
            Write down these 12 words and keep them safe. This is the <span className="text-[#39ff14]">ONLY WAY</span> to recover your account if you lose access.
          </p>

          <SeedPhraseDisplay phrase={seedPhrase} />

          <button
            type="button"
            className="w-full bg-[#39ff14] text-black font-bold text-sm tracking-widest py-3 rounded-lg hover:shadow-[0_0_15px_rgba(57,255,20,0.5)] transition-all mt-8"
            onClick={() => window.location.hash = "#/2fa/setup"}
          >
            I HAVE SAVED MY WORDS
          </button>
        </motion.div>
      </div>
    );
  }

  return (
    <div className="w-full h-screen bg-[#050505] flex items-center justify-center text-white font-mono relative overflow-hidden">
      <ParticleBackground />
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="z-10 bg-black/60 backdrop-blur-md p-8 rounded-2xl border border-white/10 max-w-md w-full"
      >
        <h1 className="text-xl mb-2 font-bold tracking-widest text-[#39ff14]">
          REGISTER OPERATOR
        </h1>
        <p className="text-xs opacity-60 mb-8 leading-relaxed uppercase tracking-tighter">
          Join the local code graph collective
        </p>

        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          <div className="flex flex-col gap-1 w-full">
            <label className="text-xs text-white/60 uppercase tracking-widest">Name</label>
            <input
              className="w-full bg-white/5 border border-white/10 rounded-lg p-3 text-sm focus:border-[#39ff14] focus:outline-none transition-colors font-mono"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Case-01"
              required
            />
          </div>

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

          <div className="flex flex-col gap-2">
            <PasswordInput
              label="Password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
            {password && (
              <div className="flex gap-1 h-1">
                {[1, 2, 3, 4].map((step) => (
                  <div
                    key={step}
                    className={`flex-1 rounded-full transition-colors ${
                      strength >= step
                        ? step <= 2
                          ? "bg-red-500"
                          : step === 3
                          ? "bg-yellow-500"
                          : "bg-[#39ff14]"
                        : "bg-white/10"
                    }`}
                  />
                ))}
              </div>
            )}
          </div>

          <PasswordInput
            label="Confirm Password"
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
            {isLoading ? "PROVISIONING..." : "REGISTER ACCOUNT"}
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
