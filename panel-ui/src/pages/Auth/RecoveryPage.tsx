import type React from "react";
import { useState } from "react";

export const RecoveryPage: React.FC = () => {
  const [email, setEmail] = useState("");
  const [seedPhrase, setSeedPhrase] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [status, setStatus] = useState<{
    type: "success" | "error";
    message: string;
  } | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setStatus(null);

    try {
      const response = await fetch("/v1/auth/recover", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          email,
          seed_phrase: seedPhrase,
          new_password: newPassword,
        }),
      });

      const data = await response.json();

      if (response.ok) {
        setStatus({
          type: "success",
          message: "Password recovered successfully. You can now login.",
        });
      } else {
        setStatus({ type: "error", message: data.error || "Recovery failed" });
      }
    } catch (err) {
      setStatus({ type: "error", message: "Connection error" });
    }
  };

  return (
    <div className="flex flex-col items-center justify-center min-h-screen bg-black text-[#39ff14] font-mono">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-md p-8 border border-[#39ff14]/30 rounded-lg bg-black/50"
      >
        <h2 className="text-2xl mb-2 text-center tracking-tighter uppercase">
          TERMINAL RECOVERY
        </h2>
        <p className="text-xs text-center opacity-70 mb-6 uppercase">
          Use your 12-word seed phrase to reset access
        </p>

        {status && (
          <div
            className={`mb-4 text-sm border p-2 ${status.type === "success" ? "text-[#39ff14] border-[#39ff14]/30" : "text-red-500 border-red-500/30"}`}
          >
            {status.message}
          </div>
        )}

        <div className="mb-4">
          <label className="block text-xs uppercase mb-1">Email</label>
          <input
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            className="w-full bg-black border border-[#39ff14]/30 p-2 text-sm focus:outline-none focus:border-[#39ff14]"
            required
          />
        </div>
        <div className="mb-4">
          <label className="block text-xs uppercase mb-1">
            Seed Phrase (12 words)
          </label>
          <textarea
            value={seedPhrase}
            onChange={(e) => setSeedPhrase(e.target.value)}
            rows={3}
            className="w-full bg-black border border-[#39ff14]/30 p-2 text-sm focus:outline-none focus:border-[#39ff14] resize-none"
            placeholder="palabra1 palabra2 ..."
            required
          />
        </div>
        <div className="mb-6">
          <label className="block text-xs uppercase mb-1">New Password</label>
          <input
            type="password"
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
            className="w-full bg-black border border-[#39ff14]/30 p-2 text-sm focus:outline-none focus:border-[#39ff14]"
            required
          />
        </div>
        <button
          type="submit"
          className="w-full bg-[#39ff14] text-black font-bold py-2 uppercase tracking-widest hover:bg-[#39ff14]/80 transition-colors"
        >
          Reset Access
        </button>
      </form>
    </div>
  );
};
