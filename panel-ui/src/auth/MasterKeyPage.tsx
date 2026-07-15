import React, { useState } from "react";
import { motion } from "motion/react";
import { Download, Upload, Shield } from "lucide-react";
import ParticleBackground from "../components/ParticleBackground";

export const MasterKeyPage: React.FC = () => {
  const [isExporting, setIsExporting] = useState(false);
  const [isImporting, setIsImporting] = useState(false);

  const handleExport = () => {
    setIsExporting(true);
    // Simulate export logic
    setTimeout(() => {
        setIsExporting(false);
        alert("Master Key exported successfully.");
    }, 1500);
  };

  const handleImport = () => {
    setIsImporting(true);
    // Simulate import logic
    setTimeout(() => {
        setIsImporting(false);
        alert("Master Key imported successfully.");
    }, 1500);
  };

  return (
    <div className="w-full h-dvh bg-[#050505] flex items-center justify-center text-white font-mono relative overflow-hidden">
      <ParticleBackground />
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        className="z-10 bg-black/60 backdrop-blur-md p-8 rounded-2xl border border-white/10 max-w-md w-full"
      >
        <div className="flex items-center gap-3 mb-6">
            <div className="p-2 bg-[#39ff14]/10 rounded-lg">
                <Shield className="text-[#39ff14]" size={24} />
            </div>
            <h1 className="text-xl font-bold tracking-widest text-[#39ff14] uppercase">
                Master Key
            </h1>
        </div>

        <p className="text-xs opacity-60 mb-8 leading-relaxed uppercase tracking-widest">
          The master key encrypts your entire local memory. Lose it, and your data is gone forever.
        </p>

        <div className="flex flex-col gap-4">
            <div className="bg-white/5 border border-white/10 rounded-xl p-6 flex flex-col gap-4">
                <h3 className="text-xs font-bold uppercase tracking-widest text-white/80">Export Identity</h3>
                <p className="text-[10px] text-white/40 uppercase">Download a secure backup of your master key and local identity.</p>
                <button
                    onClick={handleExport}
                    disabled={isExporting}
                    className="flex items-center justify-center gap-2 w-full py-3 bg-white/10 hover:bg-white/20 rounded-lg text-xs font-bold transition-all uppercase tracking-widest disabled:opacity-50"
                >
                    <Download size={16} /> {isExporting ? "EXPORTING..." : "EXPORT MASTER KEY"}
                </button>
            </div>

            <div className="bg-white/5 border border-white/10 rounded-xl p-6 flex flex-col gap-4">
                <h3 className="text-xs font-bold uppercase tracking-widest text-white/80">Import Identity</h3>
                <p className="text-[10px] text-white/40 uppercase">Restore your session using a previously exported master key file.</p>
                <button
                    onClick={handleImport}
                    disabled={isImporting}
                    className="flex items-center justify-center gap-2 w-full py-3 border border-white/20 hover:border-[#39ff14]/50 rounded-lg text-xs font-bold transition-all uppercase tracking-widest disabled:opacity-50"
                >
                    <Upload size={16} /> {isImporting ? "IMPORTING..." : "IMPORT MASTER KEY"}
                </button>
            </div>
        </div>

        <div className="mt-8">
          <button
            type="button"
            onClick={() => window.location.hash = "#/"}
            className="text-[10px] text-white/40 hover:text-[#39ff14] transition-colors uppercase tracking-widest"
          >
            Return to dashboard
          </button>
        </div>
      </motion.div>
    </div>
  );
};
