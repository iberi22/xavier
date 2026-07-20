import type React from "react";
import { useState } from "react";
import { authClient } from "../../api/authClient";

interface AuthStepProps {
  onSkip: () => void;
  onComplete: () => void;
}

export function AuthStep({ onSkip, onComplete }: AuthStepProps) {
  const [mode, setMode] = useState<"register" | "login" | "seed">("register");
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [seedPhrase, setSeedPhrase] = useState<string | null>(null);

  const handleRegister = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setIsLoading(true);
    try {
      const res = await authClient.register(email, name, password);
      setSeedPhrase(res.seed_phrase);
      setMode("seed");
    } catch (err: any) {
      setError(err?.message || "Registration failed");
    } finally {
      setIsLoading(false);
    }
  };

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setIsLoading(true);
    try {
      await authClient.login(email, password);
      onComplete();
    } catch (err: any) {
      setError(err?.message || "Login failed");
    } finally {
      setIsLoading(false);
    }
  };

  const handleSeedSaved = () => {
    setSeedPhrase(null);
    onComplete();
  };

  if (mode === "seed" && seedPhrase) {
    return (
      <div className="flex flex-col items-center gap-4">
        <h2 className="text-lg font-bold text-emerald-400 tracking-widest uppercase">
          Recovery Seed
        </h2>
        <p className="text-xs text-emerald-400/60 text-center max-w-md">
          Write down these words. This is the ONLY way to recover your account.
        </p>
        <div className="grid grid-cols-2 gap-2 bg-neutral-900 p-4 rounded-lg border border-emerald-800/30 max-w-md w-full">
          {seedPhrase.split(" ").map((word, i) => (
            <div
              key={i}
              className="flex gap-1 text-xs text-emerald-300 font-mono"
            >
              <span className="text-neutral-600 w-5 text-right">{i + 1}.</span>
              <span>{word}</span>
            </div>
          ))}
        </div>
        <button
          className="px-6 py-2 bg-emerald-600 hover:bg-emerald-500 text-black font-bold text-xs tracking-widest rounded-lg transition-all mt-4"
          onClick={handleSeedSaved}
        >
          I SAVED MY WORDS — CONTINUE
        </button>
      </div>
    );
  }

  if (mode === "register") {
    return (
      <div className="flex flex-col items-center gap-4 w-full max-w-sm">
        <h2 className="text-lg font-bold text-emerald-400 tracking-widest uppercase">
          Create Account
        </h2>
        <form onSubmit={handleRegister} className="flex flex-col gap-3 w-full">
          <input
            className="w-full bg-white/5 border border-neutral-700 rounded-lg p-3 text-sm text-white focus:border-emerald-500 focus:outline-none font-mono"
            placeholder="Name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
          />
          <input
            className="w-full bg-white/5 border border-neutral-700 rounded-lg p-3 text-sm text-white focus:border-emerald-500 focus:outline-none font-mono"
            placeholder="Email"
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
          />
          <input
            className="w-full bg-white/5 border border-neutral-700 rounded-lg p-3 text-sm text-white focus:border-emerald-500 focus:outline-none font-mono"
            placeholder="Password"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            minLength={8}
          />
          {error && (
            <div className="text-red-500 text-[10px] uppercase bg-red-500/10 border border-red-500/20 p-2 rounded-lg">
              {error}
            </div>
          )}
          <button
            type="submit"
            disabled={isLoading}
            className="w-full bg-emerald-600 hover:bg-emerald-500 text-black font-bold text-xs tracking-widest py-3 rounded-lg transition-all disabled:opacity-50"
          >
            {isLoading ? "REGISTERING..." : "REGISTER"}
          </button>
        </form>
        <div className="flex gap-4 mt-2">
          <button
            className="text-[10px] text-neutral-500 hover:text-emerald-400 transition-colors uppercase"
            onClick={() => setMode("login")}
          >
            Already have an account?
          </button>
          <button
            className="text-[10px] text-neutral-500 hover:text-neutral-300 transition-colors uppercase"
            onClick={onSkip}
          >
            Skip
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center gap-4 w-full max-w-sm">
      <h2 className="text-lg font-bold text-emerald-400 tracking-widest uppercase">
        Sign In
      </h2>
      <form onSubmit={handleLogin} className="flex flex-col gap-3 w-full">
        <input
          className="w-full bg-white/5 border border-neutral-700 rounded-lg p-3 text-sm text-white focus:border-emerald-500 focus:outline-none font-mono"
          placeholder="Email"
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
        />
        <input
          className="w-full bg-white/5 border border-neutral-700 rounded-lg p-3 text-sm text-white focus:border-emerald-500 focus:outline-none font-mono"
          placeholder="Password"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
        {error && (
          <div className="text-red-500 text-[10px] uppercase bg-red-500/10 border border-red-500/20 p-2 rounded-lg">
            {error}
          </div>
        )}
        <button
          type="submit"
          disabled={isLoading}
          className="w-full bg-emerald-600 hover:bg-emerald-500 text-black font-bold text-xs tracking-widest py-3 rounded-lg transition-all disabled:opacity-50"
        >
          {isLoading ? "SIGNING IN..." : "SIGN IN"}
        </button>
      </form>
      <div className="flex gap-4 mt-2">
        <button
          className="text-[10px] text-neutral-500 hover:text-emerald-400 transition-colors uppercase"
          onClick={() => setMode("register")}
        >
          Create account
        </button>
        <button
          className="text-[10px] text-neutral-500 hover:text-neutral-300 transition-colors uppercase"
          onClick={onSkip}
        >
          Skip
        </button>
      </div>
    </div>
  );
}
