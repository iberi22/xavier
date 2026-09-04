import React, { useEffect, useState } from "react";
import { malocaApi, MalocaPack } from "../api";
import { Activity, Database, GitMerge, FileCode, Workflow, Server } from "lucide-react";

export function OverviewTab() {
  const [pack, setPack] = useState<MalocaPack | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    malocaApi.getPack()
      .then((data) => {
        if (mounted) setPack(data);
      })
      .catch((err) => {
        if (mounted) setError(err.message);
      })
      .finally(() => {
        if (mounted) setLoading(false);
      });

    return () => { mounted = false; };
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-48">
        <div className="text-emerald-400 font-mono text-sm animate-pulse">Initializing Maloca Core Systems...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-rose-950/30 border border-rose-800/50 rounded-lg text-rose-300 text-xs font-mono">
        System Error: {error}
      </div>
    );
  }

  if (!pack) return null;

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        {/* Core Stats */}
        <div className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a]">
          <div className="flex items-center gap-2 mb-2 text-white/50">
            <FileCode size={14} className="text-emerald-400" />
            <span className="text-xs uppercase tracking-wider font-mono">Features</span>
          </div>
          <div className="text-2xl font-bold text-white flex items-baseline gap-2">
            {pack.features_total}
            <span className="text-xs text-white/40 font-normal tracking-wide">TOTAL</span>
          </div>
          <div className="mt-2 text-xs text-white/60">
            <span className="text-emerald-400">{pack.features_draft}</span> in draft state
          </div>
        </div>

        <div className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a]">
          <div className="flex items-center gap-2 mb-2 text-white/50">
            <Activity size={14} className="text-emerald-400" />
            <span className="text-xs uppercase tracking-wider font-mono">Support Queue</span>
          </div>
          <div className="text-2xl font-bold text-white flex items-baseline gap-2">
            {pack.support_open}
            <span className="text-xs text-white/40 font-normal tracking-wide">OPEN</span>
          </div>
        </div>

        <div className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a]">
          <div className="flex items-center gap-2 mb-2 text-white/50">
            <GitMerge size={14} className="text-emerald-400" />
            <span className="text-xs uppercase tracking-wider font-mono">Decisions</span>
          </div>
          <div className="text-2xl font-bold text-white flex items-baseline gap-2">
            {pack.decisions_count}
            <span className="text-xs text-white/40 font-normal tracking-wide">RECORDED</span>
          </div>
        </div>

        <div className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a]">
          <div className="flex items-center gap-2 mb-2 text-white/50">
            <Workflow size={14} className="text-emerald-400" />
            <span className="text-xs uppercase tracking-wider font-mono">Inbox Tasks</span>
          </div>
          <div className="text-2xl font-bold text-white flex items-baseline gap-2">
            {pack.inbox_open}
            <span className="text-xs text-white/40 font-normal tracking-wide">PENDING</span>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <div className="glass-panel border border-white/5 rounded-xl bg-[#0a0a0a] overflow-hidden">
          <div className="p-4 border-b border-white/5 bg-white/[0.02]">
            <h3 className="text-sm font-semibold text-white flex items-center gap-2">
              <Database size={14} className="text-cyan-400" />
              System Status
            </h3>
          </div>
          <div className="p-4 space-y-3 font-mono text-xs">
            <div className="flex justify-between items-center py-1 border-b border-white/5">
              <span className="text-white/50">Codegraph Indexed</span>
              <span className="text-emerald-400">{pack.codegraph_indexed_at ? new Date(pack.codegraph_indexed_at).toLocaleString() : 'N/A'}</span>
            </div>
            <div className="flex justify-between items-center py-1 border-b border-white/5">
              <span className="text-white/50">Generated At</span>
              <span className="text-emerald-400">{new Date(pack.generated_at).toLocaleString()}</span>
            </div>
            <div className="flex justify-between items-center py-1">
              <span className="text-white/50">Codegraph Head</span>
              <span className="text-cyan-400 truncate max-w-[200px]" title={pack.codegraph_head || 'Unknown'}>{pack.codegraph_head || 'Unknown'}</span>
            </div>
          </div>
        </div>

        <div className="glass-panel border border-white/5 rounded-xl bg-[#0a0a0a] overflow-hidden">
          <div className="p-4 border-b border-white/5 bg-white/[0.02] flex items-center justify-between">
            <h3 className="text-sm font-semibold text-white flex items-center gap-2">
              <Server size={14} className="text-amber-400" />
              Module Gaps (Zero Symbol)
            </h3>
            <span className="text-xs px-2 py-0.5 rounded-full bg-amber-500/20 text-amber-300 font-mono">
              {pack.gaps_zero_symbol_modules.length} modules
            </span>
          </div>
          <div className="p-4 max-h-[160px] overflow-y-auto custom-scrollbar">
            {pack.gaps_zero_symbol_modules.length > 0 ? (
               <ul className="space-y-1">
                 {pack.gaps_zero_symbol_modules.map((mod, i) => (
                   <li key={i} className="text-xs font-mono text-white/70 py-1 flex items-center gap-2">
                     <span className="w-1.5 h-1.5 rounded-full bg-amber-500/50"></span>
                     {mod}
                   </li>
                 ))}
               </ul>
            ) : (
               <div className="text-xs text-white/40 italic h-full flex items-center justify-center">
                 No gaps detected in symbolic index.
               </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
