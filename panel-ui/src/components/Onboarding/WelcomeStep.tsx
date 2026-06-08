import { Terminal, Shield, Cpu } from 'lucide-react';

export function WelcomeStep({ onNext }: { onNext: () => void }) {
  return (
    <div className="space-y-6 animate-in fade-in slide-in-from-bottom-4 duration-700">
      <div className="text-center space-y-2">
        <h1 className="text-3xl font-bold tracking-tight text-white drop-shadow-[0_0_8px_rgba(16,185,129,0.5)]">
          INITIALIZING XAVIER_
        </h1>
        <p className="text-emerald-500/70 text-sm uppercase tracking-widest">
          Cognitive Core // Secure Neural Link
        </p>
      </div>

      <div className="grid gap-4 py-4">
        <div className="flex items-start gap-4 p-4 rounded bg-neutral-950/50 border border-emerald-900/30">
          <Shield className="w-6 h-6 text-emerald-400 mt-1" />
          <div>
            <h3 className="font-semibold text-emerald-300">Local Privacy</h3>
            <p className="text-sm text-neutral-400">Your data never leaves this machine unless explicitly requested.</p>
          </div>
        </div>
        <div className="flex items-start gap-4 p-4 rounded bg-neutral-950/50 border border-emerald-900/30">
          <Cpu className="w-6 h-6 text-emerald-400 mt-1" />
          <div>
            <h3 className="font-semibold text-emerald-300">Auto-Optimization</h3>
            <p className="text-sm text-neutral-400">Xavier will now scan your system to optimize neural models and detect sibling nodes (OpenClaw, Hermes).</p>
          </div>
        </div>
      </div>

      <div className="flex justify-end pt-4">
        <button
          onClick={onNext}
          className="group relative px-6 py-2 bg-emerald-950/50 hover:bg-emerald-900/80 text-emerald-400 border border-emerald-500/50 hover:border-emerald-400 rounded transition-all duration-300 overflow-hidden"
        >
          <span className="relative z-10 flex items-center gap-2">
            <Terminal className="w-4 h-4" /> BEGIN_SCAN
          </span>
          <div className="absolute inset-0 h-full w-0 bg-emerald-500/20 group-hover:w-full transition-all duration-300 ease-out" />
        </button>
      </div>
    </div>
  );
}
