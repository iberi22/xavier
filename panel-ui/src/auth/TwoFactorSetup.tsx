import { motion } from "motion/react";
import type React from "react";
import { useEffect, useState } from "react";
import { authClient } from "../api/authClient";
import ParticleBackground from "../components/ParticleBackground";
import { QrCodeDisplay } from "../components/QrCodeDisplay";
import { TwoFactorInput } from "../components/TwoFactorInput";

export const TwoFactorSetup: React.FC = () => {
  const [qrCode, setQrCode] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    const init2FA = async () => {
      try {
        const response = await authClient.setup2FA();
        setQrCode(response.qr_code);
      } catch (err) {
        setError("Failed to initialize 2FA setup");
      }
    };
    void init2FA();
  }, []);

  const handleVerify = async () => {
    setIsLoading(true);
    setError(null);
    try {
      await authClient.verify2FA(code);
      window.location.hash = "#/2fa/backup";
    } catch (err) {
      setError("Invalid code. Please try again.");
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
        className="z-10 bg-black/60 backdrop-blur-md p-8 rounded-2xl border border-white/10 max-w-md w-full text-center"
      >
        <h1 className="text-xl mb-2 font-bold tracking-widest text-[#39ff14] uppercase">
          2FA Configuration
        </h1>
        <p className="text-[10px] opacity-60 mb-8 uppercase tracking-widest">
          Recommended for high clearance operators
        </p>

        {qrCode && <QrCodeDisplay svg={qrCode} />}

        <div className="text-left mt-8 mb-6 flex flex-col gap-2">
          <p className="text-[10px] text-white/50 uppercase">
            1. Open your authenticator app
          </p>
          <p className="text-[10px] text-white/50 uppercase">
            2. Scan the QR code above
          </p>
          <p className="text-[10px] text-white/50 uppercase">
            3. Enter the 6-digit verification code
          </p>
        </div>

        <TwoFactorInput value={code} onChange={setCode} />

        {error && (
          <div className="mt-4 text-red-500 text-[10px] uppercase tracking-widest bg-red-500/10 border border-red-500/20 p-2 rounded-lg">
            {error}
          </div>
        )}

        <div className="mt-8 flex flex-col gap-3">
          <button
            type="button"
            disabled={code.length !== 6 || isLoading}
            onClick={handleVerify}
            className="w-full bg-[#39ff14] text-black font-bold text-sm tracking-widest py-3 rounded-lg hover:shadow-[0_0_15px_rgba(57,255,20,0.5)] transition-all disabled:opacity-50"
          >
            {isLoading ? "VERIFYING..." : "ACTIVATE 2FA"}
          </button>
          <button
            type="button"
            onClick={() => (window.location.hash = "#/")}
            className="text-[10px] text-white/40 hover:text-white/80 transition-colors uppercase tracking-widest py-2"
          >
            Skip (Not Recommended)
          </button>
        </div>
      </motion.div>
    </div>
  );
};
