import {
  AlertTriangle,
  CheckCircle2,
  Key,
  RefreshCw,
  ShieldAlert,
  X,
} from "lucide-react";
import React, { useCallback, useEffect, useMemo, useState } from "react";
import { SeedPhraseDisplay } from "./SeedPhraseDisplay";

export interface RecoveryModalProps {
  isOpen: boolean;
  onClose: () => void;
  mnemonic: string;
  onRestore: (phrase: string) => void;
}

type TabType = "backup" | "restore";

export const RecoveryModal: React.FC<RecoveryModalProps> = ({
  isOpen,
  onClose,
  mnemonic,
  onRestore,
}) => {
  const [activeTab, setActiveTab] = useState<TabType>("backup");
  const words = useMemo(
    () => mnemonic.trim().split(/\s+/).filter(Boolean),
    [mnemonic]
  );

  // Quiz State
  const [quizIndices, setQuizIndices] = useState<[number, number]>([0, 1]);
  const [quizAnswers, setQuizAnswers] = useState<[string, string]>(["", ""]);
  const [quizSuccess, setQuizSuccess] = useState(false);

  // Generate random quiz indices
  const generateQuiz = useCallback(() => {
    if (words.length < 2) {
      setQuizIndices([0, 1]);
      return;
    }
    const idx1 = Math.floor(Math.random() * words.length);
    let idx2 = Math.floor(Math.random() * words.length);
    while (idx2 === idx1) {
      idx2 = Math.floor(Math.random() * words.length);
    }
    const sortedIndices: [number, number] =
      idx1 < idx2 ? [idx1, idx2] : [idx2, idx1];
    setQuizIndices(sortedIndices);
    setQuizAnswers(["", ""]);
    setQuizSuccess(false);
  }, [words]);

  useEffect(() => {
    if (isOpen && words.length >= 2) {
      generateQuiz();
    }
  }, [isOpen, words.length, generateQuiz]);

  // Validate Quiz Answers
  const isQuizPassed = useMemo(() => {
    if (words.length < 2) return false;
    const target1 = words[quizIndices[0]]?.toLowerCase();
    const target2 = words[quizIndices[1]]?.toLowerCase();
    const ans1 = quizAnswers[0].trim().toLowerCase();
    const ans2 = quizAnswers[1].trim().toLowerCase();
    return Boolean(target1 && target2 && ans1 === target1 && ans2 === target2);
  }, [words, quizIndices, quizAnswers]);

  // Restore State
  const [restoreText, setRestoreText] = useState("");
  const [restoreError, setRestoreError] = useState<string | null>(null);

  // Handle Restore Validation and Submission
  const handleRestoreSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const cleanWords = restoreText
      .trim()
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean);

    if (cleanWords.length !== 12 && cleanWords.length !== 24) {
      setRestoreError(
        `Invalid seed phrase length: ${cleanWords.length} words found. Must be exactly 12 or 24 words.`
      );
      return;
    }

    const invalidWords = cleanWords.filter((w) => !/^[a-z]+$/.test(w));
    if (invalidWords.length > 0) {
      setRestoreError(
        `Seed phrase contains invalid characters or words: "${invalidWords.slice(0, 3).join(", ")}"`
      );
      return;
    }

    setRestoreError(null);
    onRestore(cleanWords.join(" "));
  };

  // Close on Escape key
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isOpen) {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
      <div className="relative w-full max-w-2xl bg-[#0f1013] border border-white/10 rounded-2xl shadow-2xl overflow-hidden flex flex-col max-h-[90vh]">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-white/10 bg-black/40">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg bg-[#39ff14]/10 border border-[#39ff14]/30 text-[#39ff14]">
              <Key size={20} />
            </div>
            <div>
              <h2 className="text-lg font-semibold text-white tracking-wide">
                Disaster Recovery Wizard
              </h2>
              <p className="text-xs text-white/50">
                Sovereign Node Backup & Recovery Mnemonic
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Cerrar"
            className="p-1.5 rounded-lg text-white/50 hover:text-white hover:bg-white/10 transition-colors"
          >
            <X size={20} />
          </button>
        </div>

        {/* Tab Selection */}
        <div className="flex border-b border-white/10 bg-black/20 px-6 pt-2 gap-2">
          <button
            type="button"
            onClick={() => setActiveTab("backup")}
            className={`flex items-center gap-2 px-4 py-2.5 text-xs font-semibold tracking-wider uppercase border-b-2 transition-all ${
              activeTab === "backup"
                ? "border-[#39ff14] text-[#39ff14] bg-[#39ff14]/5"
                : "border-transparent text-white/40 hover:text-white/80"
            }`}
          >
            <ShieldAlert size={14} /> Backup Node
          </button>
          <button
            type="button"
            onClick={() => setActiveTab("restore")}
            className={`flex items-center gap-2 px-4 py-2.5 text-xs font-semibold tracking-wider uppercase border-b-2 transition-all ${
              activeTab === "restore"
                ? "border-[#39ff14] text-[#39ff14] bg-[#39ff14]/5"
                : "border-transparent text-white/40 hover:text-white/80"
            }`}
          >
            <RefreshCw size={14} /> Restore Node
          </button>
        </div>

        {/* Modal Body */}
        <div className="flex-1 overflow-y-auto p-6 space-y-6">
          {activeTab === "backup" ? (
            <div className="space-y-6">
              {/* Zero-Knowledge Custody Warning Banner */}
              <div className="p-4 rounded-xl bg-amber-500/10 border border-amber-500/30 flex items-start gap-3 text-amber-200 text-xs leading-relaxed">
                <AlertTriangle
                  size={18}
                  className="shrink-0 text-amber-400 mt-0.5"
                />
                <div>
                  <span className="font-bold block text-amber-300 uppercase tracking-wider mb-0.5">
                    Zero-Knowledge Sovereign Custody Notice
                  </span>
                  Your recovery phrase controls your node identity key pair directly. Neither Cloudflare nor Xavier servers hold copies of your private key. If you lose your recovery mnemonic, your node credentials cannot be recovered by any party.
                </div>
              </div>

              {/* Seed Phrase Display */}
              <div>
                <h3 className="text-xs uppercase tracking-widest text-white/50 mb-3 font-mono">
                  1. Save Your Mnemonic Words
                </h3>
                <SeedPhraseDisplay phrase={mnemonic} />
              </div>

              {/* Quiz Section */}
              <div className="p-4 rounded-xl bg-white/5 border border-white/10 space-y-4">
                <div className="flex items-center justify-between">
                  <h3 className="text-xs uppercase tracking-widest text-white/80 font-mono flex items-center gap-2">
                    <CheckCircle2 size={14} className="text-[#39ff14]" />
                    2. Backup Verification Quiz
                  </h3>
                  <button
                    type="button"
                    onClick={generateQuiz}
                    className="flex items-center gap-1 text-[11px] text-white/50 hover:text-white transition-colors"
                  >
                    <RefreshCw size={12} /> Refresh Quiz
                  </button>
                </div>
                <p className="text-xs text-white/60">
                  To confirm you have stored your phrase safely, please type word <span className="text-[#39ff14] font-bold font-mono">#{quizIndices[0] + 1}</span> and word <span className="text-[#39ff14] font-bold font-mono">#{quizIndices[1] + 1}</span>.
                </p>

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <div>
                    <label className="block text-[10px] uppercase font-mono text-white/40 mb-1">
                      Word #{quizIndices[0] + 1}
                    </label>
                    <input
                      type="text"
                      value={quizAnswers[0]}
                      onChange={(e) => {
                        setQuizAnswers([e.target.value, quizAnswers[1]]);
                        setQuizSuccess(false);
                      }}
                      placeholder={`Enter word #${quizIndices[0] + 1}`}
                      className="w-full bg-black/50 border border-white/15 rounded-lg px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-[#39ff14] transition-colors"
                    />
                  </div>
                  <div>
                    <label className="block text-[10px] uppercase font-mono text-white/40 mb-1">
                      Word #{quizIndices[1] + 1}
                    </label>
                    <input
                      type="text"
                      value={quizAnswers[1]}
                      onChange={(e) => {
                        setQuizAnswers([quizAnswers[0], e.target.value]);
                        setQuizSuccess(false);
                      }}
                      placeholder={`Enter word #${quizIndices[1] + 1}`}
                      className="w-full bg-black/50 border border-white/15 rounded-lg px-3 py-2 text-xs font-mono text-white focus:outline-none focus:border-[#39ff14] transition-colors"
                    />
                  </div>
                </div>

                {quizAnswers[0] || quizAnswers[1] ? (
                  isQuizPassed ? (
                    <div className="text-xs text-[#39ff14] flex items-center gap-1.5 font-mono">
                      <CheckCircle2 size={14} /> Word verification passed!
                    </div>
                  ) : (
                    <div className="text-xs text-red-400 font-mono">
                      Words do not match your backup mnemonic phrase.
                    </div>
                  )
                ) : null}
              </div>

              {/* Confirmation Action */}
              <button
                type="button"
                disabled={!isQuizPassed}
                onClick={() => {
                  setQuizSuccess(true);
                  onClose();
                }}
                className={`w-full py-3 rounded-xl font-mono text-xs uppercase tracking-widest font-bold transition-all flex items-center justify-center gap-2 ${
                  isQuizPassed
                    ? "bg-[#39ff14] text-black hover:bg-[#32e011] shadow-[0_0_20px_rgba(57,255,20,0.3)] cursor-pointer"
                    : "bg-white/10 text-white/30 cursor-not-allowed border border-white/5"
                }`}
              >
                <CheckCircle2 size={16} /> I have securely backed up my key
              </button>
            </div>
          ) : (
            /* Restore Tab */
            <form onSubmit={handleRestoreSubmit} className="space-y-6">
              <div className="p-4 rounded-xl bg-blue-500/10 border border-blue-500/30 text-blue-200 text-xs leading-relaxed flex items-start gap-3">
                <Key size={18} className="shrink-0 text-blue-400 mt-0.5" />
                <div>
                  <span className="font-bold block text-blue-300 uppercase tracking-wider mb-0.5">
                    Sovereign Node Reconstruction
                  </span>
                  Enter your 12 or 24 word BIP-39 mnemonic seed phrase below to re-derive your sovereign node identity and restore full mesh permissions.
                </div>
              </div>

              <div>
                <label className="block text-xs uppercase tracking-widest text-white/60 font-mono mb-2">
                  Enter 12 or 24 Word Mnemonic
                </label>
                <textarea
                  value={restoreText}
                  onChange={(e) => {
                    setRestoreText(e.target.value);
                    setRestoreError(null);
                  }}
                  rows={4}
                  placeholder="e.g. abandon amount abandon amount abandon amount abandon amount abandon amount abandon announce"
                  className="w-full bg-black/50 border border-white/15 rounded-xl p-3 text-xs font-mono text-white placeholder-white/20 focus:outline-none focus:border-[#39ff14] transition-colors resize-none"
                />
                <div className="flex items-center justify-between mt-2 text-[11px] font-mono text-white/40">
                  <span>
                    Word count:{" "}
                    <strong className="text-white/80">
                      {
                        restoreText
                          .trim()
                          .split(/\s+/)
                          .filter(Boolean).length
                      }
                    </strong>{" "}
                    / 12 or 24
                  </span>
                  <span>BIP-39 Standard Format</span>
                </div>
              </div>

              {restoreError && (
                <div className="p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-xs font-mono">
                  {restoreError}
                </div>
              )}

              <button
                type="submit"
                className="w-full py-3 rounded-xl bg-[#39ff14] text-black font-mono text-xs uppercase tracking-widest font-bold hover:bg-[#32e011] transition-all shadow-[0_0_20px_rgba(57,255,20,0.3)] flex items-center justify-center gap-2 cursor-pointer"
              >
                <RefreshCw size={16} /> Restore Sovereign Node Identity
              </button>
            </form>
          )}
        </div>
      </div>
    </div>
  );
};
