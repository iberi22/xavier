import React from "react";
import { Copy, Check } from "lucide-react";

interface SeedPhraseDisplayProps {
  phrase: string;
}

export const SeedPhraseDisplay: React.FC<SeedPhraseDisplayProps> = ({ phrase }) => {
  const [copied, setCopied] = React.useState(false);
  const words = phrase.split(" ");

  const handleCopy = () => {
    void navigator.clipboard.writeText(phrase);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-3 gap-2">
        {words.map((word, i) => (
          <div
            key={i}
            className="bg-white/5 border border-white/10 rounded-lg p-2 flex items-center gap-2"
          >
            <span className="text-[10px] text-white/30 font-mono w-4">{i + 1}</span>
            <span className="text-sm font-mono text-white/80">{word}</span>
          </div>
        ))}
      </div>
      <button
        type="button"
        onClick={handleCopy}
        className="flex items-center justify-center gap-2 w-full py-2 border border-[#39ff14]/30 rounded-lg text-xs text-[#39ff14] hover:bg-[#39ff14]/10 transition-colors uppercase tracking-widest"
      >
        {copied ? (
          <>
            <Check size={14} /> Copied to Clipboard
          </>
        ) : (
          <>
            <Copy size={14} /> Copy Seed Phrase
          </>
        )}
      </button>
    </div>
  );
};
