import React from 'react';
import { MonitorOff, Hash } from 'lucide-react';

interface HeadlessToggleProps {
  enabled: boolean;
  port: number;
  onToggle: (enabled: boolean) => void;
  onPortChange: (port: number) => void;
}

export function HeadlessToggle({ enabled, port, onToggle, onPortChange }: HeadlessToggleProps) {
  return (
    <div className="bg-[#050505]/50 border border-white/5 rounded-2xl p-6">
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-4">
          <div className={`p-3 rounded-xl transition-colors ${enabled ? 'bg-[#39ff14]/10 text-[#39ff14]' : 'bg-white/5 text-white/30'}`}>
            <MonitorOff className="w-6 h-6" />
          </div>
          <div>
            <h3 className="text-lg font-bold tracking-tight">Headless Mode</h3>
            <p className="text-xs text-white/40">Expose Xavier as a background service API.</p>
          </div>
        </div>
        <button
          onClick={() => onToggle(!enabled)}
          className={`relative w-14 h-8 rounded-full transition-all duration-300 ${enabled ? 'bg-[#39ff14]' : 'bg-white/10'}`}
        >
          <div className={`absolute top-1 left-1 w-6 h-6 rounded-full bg-white transition-transform duration-300 shadow-lg ${enabled ? 'translate-x-6' : 'translate-x-0'}`} />
        </button>
      </div>

      <div className={`space-y-2 transition-all duration-300 ${enabled ? 'opacity-100' : 'opacity-30 pointer-events-none grayscale'}`}>
        <label className="text-[10px] uppercase text-white/50 tracking-widest flex items-center gap-2">
          <Hash className="w-3 h-3" />
          API Listener Port
        </label>
        <div className="relative">
          <input
            type="number"
            value={port}
            onChange={(e) => onPortChange(parseInt(e.target.value))}
            className="w-full bg-[#050505] border border-white/10 focus:border-[#39ff14]/50 rounded-xl px-4 py-3 text-sm font-mono transition-all outline-none"
            placeholder="8006"
          />
          <div className="absolute right-4 top-1/2 -translate-y-1/2 flex gap-1">
            <span className="w-1 h-1 rounded-full bg-[#39ff14]" />
            <span className="w-1 h-1 rounded-full bg-[#39ff14]/50" />
            <span className="w-1 h-1 rounded-full bg-[#39ff14]/20" />
          </div>
        </div>
        <p className="text-[10px] text-white/30 mt-2 italic">Requires restart to apply new port configuration.</p>
      </div>
    </div>
  );
}
