import React from 'react';
import { RefreshCw, TrendingUp } from 'lucide-react';

interface Quota {
  provider: string;
  tier: string;
  requests: string;
  tokens: string;
  reset: string;
  status: 'green' | 'yellow' | 'red';
}

interface QuotaTableProps {
  quotas: Quota[];
}

export function QuotaTable({ quotas }: QuotaTableProps) {
  return (
    <div className="w-full overflow-hidden border border-white/5 rounded-2xl bg-[#050505]/30">
      <table className="w-full text-left border-collapse">
        <thead>
          <tr className="bg-white/5 border-b border-white/5">
            <th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">Provider</th>
            <th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">Tier</th>
            <th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">Requests</th>
            <th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">Tokens</th>
            <th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">Reset</th>
            <th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold text-center">Status</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-white/5">
          {quotas.map((q) => (
            <tr key={q.provider} className="hover:bg-white/[0.02] transition-colors group">
              <td className="px-6 py-4">
                <div className="flex items-center gap-3">
                  <span className="text-sm font-medium capitalize">{q.provider}</span>
                </div>
              </td>
              <td className="px-6 py-4">
                <span className="text-xs text-white/60 bg-white/5 px-2 py-0.5 rounded border border-white/10 uppercase tracking-tighter">
                  {q.tier}
                </span>
              </td>
              <td className="px-6 py-4 font-mono text-xs text-white/80">{q.requests}</td>
              <td className="px-6 py-4 font-mono text-xs text-white/80">{q.tokens}</td>
              <td className="px-6 py-4">
                <div className="flex items-center gap-1.5 text-white/40 text-xs">
                  <RefreshCw className="w-3 h-3" />
                  {q.reset}
                </div>
              </td>
              <td className="px-6 py-4">
                <div className="flex justify-center">
                  <div className={`w-2 h-2 rounded-full shadow-[0_0_8px] ${
                    q.status === 'green' ? 'bg-[#39ff14] shadow-[#39ff14]/50' :
                    q.status === 'yellow' ? 'bg-yellow-400 shadow-yellow-400/50' :
                    'bg-red-500 shadow-red-500/50'
                  }`} />
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
