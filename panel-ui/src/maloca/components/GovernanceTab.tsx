import React, { useEffect, useState } from "react";
import { malocaApi, Proposal } from "../api";
import { Landmark, Vote, Target, CheckCircle2, Clock, XCircle, BarChart3 } from "lucide-react";

export function GovernanceTab() {
  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    malocaApi.getProposals()
      .then((data) => {
        if (mounted) setProposals(data);
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
        <div className="text-cyan-400 font-mono text-sm animate-pulse">Synchronizing Consensus State...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-rose-950/30 border border-rose-800/50 rounded-lg text-rose-300 text-xs font-mono">
        Consensus Error: {error}
      </div>
    );
  }

  const getStatusIcon = (status: string) => {
    switch (status.toLowerCase()) {
      case 'open': return <Clock size={16} className="text-amber-400" />;
      case 'closed': return <CheckCircle2 size={16} className="text-emerald-400" />;
      case 'reconsidering': return <XCircle size={16} className="text-rose-400" />;
      case 'analyzing': return <BarChart3 size={16} className="text-cyan-400" />;
      default: return <Target size={16} className="text-white/50" />;
    }
  };

  const getStatusColor = (status: string) => {
    switch (status.toLowerCase()) {
      case 'open': return 'text-amber-400 border-amber-400/20 bg-amber-400/10';
      case 'closed': return 'text-emerald-400 border-emerald-400/20 bg-emerald-400/10';
      case 'reconsidering': return 'text-rose-400 border-rose-400/20 bg-rose-400/10';
      case 'analyzing': return 'text-cyan-400 border-cyan-400/20 bg-cyan-400/10';
      default: return 'text-white/50 border-white/10 bg-white/5';
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex justify-between items-center mb-2">
        <div>
          <h3 className="text-lg font-semibold text-white flex items-center gap-2">
            <Landmark size={18} className="text-cyan-400" />
            DAO Governance
          </h3>
          <p className="text-xs text-white/50 mt-1">Council node consensus & parameter voting</p>
        </div>
        <button
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-mono bg-cyan-500/10 hover:bg-cyan-500/20 text-cyan-400 border border-cyan-500/30 rounded transition-colors"
          onClick={() => alert("Submit proposal functionality coming soon")}
        >
          <Vote size={14} />
          Submit Proposal
        </button>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
        <div className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a]">
          <div className="text-white/40 text-xs font-mono mb-1">Active Proposals</div>
          <div className="text-2xl font-bold text-white">{proposals.filter(p => p.status === 'open').length}</div>
        </div>
        <div className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a]">
          <div className="text-white/40 text-xs font-mono mb-1">Total Passed</div>
          <div className="text-2xl font-bold text-emerald-400">{proposals.filter(p => p.status === 'closed').length}</div>
        </div>
        <div className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a]">
          <div className="text-white/40 text-xs font-mono mb-1">Participation Rate</div>
          <div className="text-2xl font-bold text-cyan-400">--%</div>
        </div>
      </div>

      <div className="space-y-3">
        <h4 className="text-sm font-medium text-white/70 mb-3 border-b border-white/5 pb-2">Recent Proposals</h4>
        {proposals.length === 0 ? (
          <div className="text-center py-8 text-xs text-white/40 italic border border-dashed border-white/10 rounded-lg">
            No proposals found in current era.
          </div>
        ) : (
          proposals.map(proposal => (
            <div key={proposal.id} className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a] hover:bg-white/[0.02] transition-colors">
              <div className="flex items-start justify-between gap-4">
                <div className="flex-1">
                  <div className="flex items-center gap-2 mb-1.5">
                    <span className="font-mono text-xs text-cyan-400/50">#{proposal.id.substring(0, 8)}</span>
                    <h5 className="text-white font-medium text-sm">{proposal.title}</h5>
                  </div>
                  <p className="text-xs text-white/60 line-clamp-2">{proposal.body}</p>
                </div>
                <div className={`flex items-center gap-1.5 px-2.5 py-1 rounded-full border text-[10px] uppercase font-mono tracking-wider ${getStatusColor(proposal.status)}`}>
                  {getStatusIcon(proposal.status)}
                  {proposal.status}
                </div>
              </div>
              <div className="mt-4 pt-3 border-t border-white/5 flex items-center justify-between text-xs text-white/40">
                <div className="flex gap-4">
                  <span className="flex items-center gap-1.5">
                    <Clock size={12} />
                    {new Date(proposal.created_at).toLocaleDateString()}
                  </span>
                  <span className="font-mono px-1.5 py-0.5 rounded bg-white/5">
                    Type: {proposal.type}
                  </span>
                </div>
                {proposal.status === 'open' && (
                  <button className="text-cyan-400 hover:text-cyan-300 font-mono transition-colors">
                    Cast Vote →
                  </button>
                )}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
