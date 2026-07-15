import React, { useState, useEffect } from "react";
import { motion } from "motion/react";
import { Copy, Check } from "lucide-react";
import ParticleBackground from "../components/ParticleBackground";
import { authClient } from "../api/authClient";

export const BackupCodesPage: React.FC = () => {
  const [codes, setCodes] = useState<string[]>([]);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const fetchCodes = async () => {
        try {
            const response = await authClient.setup2FA();
            setCodes(response.backup_codes);
        } catch (e) {
            console.error("Failed to fetch backup codes", e);
        }
    };
    void fetchCodes();
  }, []);

  const handleCopy = () => {
    void navigator.clipboard.writeText(codes.join("\n"));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="w-full h-screen bg-[#050505] flex items-center justify-center text-white font-mono relative overflow-hidden">
      <ParticleBackground />
      <motion.div
        initial={{ opacity: 0, scale: 0.95 }}
        animate={{ opacity: 1, scale: 1 }}
        className="z-10 bg-black/60 backdrop-blur-md p-8 rounded-2xl border border-[#39ff14]/30 max-w-md w-full"
      >
        <h1 className="text-xl mb-4 font-bold tracking-widest text-[#39ff14] uppercase">
          Backup Access Codes
        </h1>
        <p className="text-xs opacity-70 mb-6 leading-relaxed uppercase">
          Save these codes in a secure location. Each code can be used <span className="text-[#39ff14]">ONCE</span> to bypass 2FA if you lose your device.
        </p>

        <div className="grid grid-cols-2 gap-2 mb-8">
          {codes.map((code, i) => (
            <div key={i} className="bg-white/5 border border-white/10 rounded-lg p-2 text-center font-mono text-sm tracking-widest text-white/80">
              {code}
            </div>
          ))}
        </div>

        <div className="flex flex-col gap-3">
            <button
                type="button"
                onClick={handleCopy}
                className="flex items-center justify-center gap-2 w-full py-3 border border-[#39ff14]/30 rounded-lg text-xs text-[#39ff14] hover:bg-[#39ff14]/10 transition-colors uppercase tracking-widest"
            >
                {copied ? <><Check size={16}/> Codes Copied</> : <><Copy size={16}/> Copy All Codes</>}
            </button>
            <button
                type="button"
                className="w-full bg-[#39ff14] text-black font-bold text-sm tracking-widest py-3 rounded-lg hover:shadow-[0_0_15px_rgba(57,255,20,0.5)] transition-all mt-2"
                onClick={() => window.location.hash = "#/"}
            >
                I HAVE SAVED MY CODES
            </button>
        </div>
      </motion.div>
    </div>
  );
};
