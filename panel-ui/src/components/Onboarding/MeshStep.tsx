import { Network, Plus, QrCode, ShieldCheck } from "lucide-react";
import { motion } from "motion/react";
import { useState } from "react";

interface MeshStepProps {
  onComplete: (data: { joinCode?: string }) => void;
}

export function MeshStep({ onComplete }: MeshStepProps) {
  const [joinCode, setJoinCode] = useState("");

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      className="space-y-6"
    >
      <div className="space-y-2">
        <h2 className="text-xl font-bold tracking-tight text-white flex items-center gap-2">
          <Network className="text-[#39ff14]" size={20} />
          P2P_MESH_UPLINK
        </h2>
        <p className="text-xs text-white/40 leading-relaxed uppercase tracking-wider">
          Xavier nodes can form a secure, decentralized mesh network to share
          memories and knowledge.
        </p>
      </div>

      <div className="grid grid-cols-1 gap-4">
        <div className="p-4 rounded-xl bg-white/5 border border-white/10 space-y-4">
          <div className="flex items-center gap-3">
            <QrCode className="text-blue-400" size={18} />
            <span className="text-xs font-bold text-white/80 uppercase">
              Join Existing Mesh
            </span>
          </div>
          <p className="text-[10px] text-white/40 leading-normal uppercase">
            If you have a pairing code from another Xavier node, enter it here
            to link your memories.
          </p>
          <input
            type="text"
            value={joinCode}
            onChange={(e) => setJoinCode(e.target.value)}
            placeholder="PASTE_PAIRING_CODE_HERE"
            className="w-full bg-black/40 border border-white/10 rounded-lg px-4 py-2 text-xs font-mono outline-none focus:border-[#39ff14]/30 text-white/80 placeholder:text-white/10"
          />
        </div>

        <div className="p-4 rounded-xl bg-[#39ff14]/5 border border-[#39ff14]/10 space-y-3">
          <div className="flex items-center gap-3">
            <ShieldCheck className="text-[#39ff14]" size={18} />
            <span className="text-xs font-bold text-[#39ff14] uppercase">
              Independent Node
            </span>
          </div>
          <p className="text-[10px] text-[#39ff14]/60 leading-normal uppercase">
            You can always pair nodes later from the security dashboard. Your
            identity is already secured.
          </p>
        </div>
      </div>

      <div className="pt-4 border-t border-white/5 flex gap-3">
        <button
          onClick={() => onComplete({ joinCode: joinCode.trim() || undefined })}
          className="flex-1 bg-[#39ff14] text-black font-bold text-[10px] tracking-widest py-3 rounded-lg hover:shadow-[0_0_15px_rgba(57,255,20,0.5)] transition-all uppercase"
        >
          {joinCode ? "LINK_AND_CONTINUE" : "CONTINUE_STANDALONE"}
        </button>
      </div>
    </motion.div>
  );
}
