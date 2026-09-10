import { Bot, RefreshCw } from "lucide-react";
import React, { useCallback, useEffect, useState } from "react";
import { ApiClient } from "../api/client";
import type { Agent } from "../types";

function StatusBadge({ status }: { status: Agent["status"] }) {
  const colors = {
    running: "text-[#39ff14] bg-[#39ff14]/10 border-[#39ff14]/20",
    stopped: "text-white/40 bg-white/5 border-white/10",
    error: "text-red-400 bg-red-400/10 border-red-400/20",
  };

  return (
    <div
      className={`px-3 py-1 rounded-full text-[10px] font-bold uppercase tracking-wider border ${colors[status] || colors.stopped}`}
    >
      {status}
    </div>
  );
}

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted inline Agent rendering into AgentItem and wrapped in React.memo()
 * 🎯 Why: When agents update, inline mapping caused O(N) DOM reconciliation for every agent.
 * 📊 Impact: O(1) rendering for individual agent updates, preventing unneeded re-renders.
 */
const AgentItem = React.memo(function AgentItem({ agent }: { agent: Agent }) {
  return (
    <div className="bg-white/5 border border-white/10 rounded-[24px] p-6 hover:border-[#39ff14]/30 transition-all group">
      <div className="flex items-start justify-between mb-6">
        <div className="p-3 bg-white/5 rounded-2xl group-hover:bg-[#39ff14]/10 group-hover:text-[#39ff14] transition-colors">
          <Bot size={24} />
        </div>
        <StatusBadge status={agent.status} />
      </div>

      <div className="space-y-1 mb-6">
        <h3 className="font-bold text-white tracking-tight">
          {agent.name}
        </h3>
        <p className="text-[10px] text-white/30 font-mono truncate">
          {agent.id}
        </p>
      </div>

      <div className="pt-6 border-t border-white/5 flex items-center justify-between">
        <div className="text-[10px] uppercase tracking-widest text-white/40">
          Last Active
        </div>
        <div className="text-[11px] font-mono text-white/60">
          {agent.last_seen
            ? new Date(agent.last_seen).toLocaleTimeString()
            : "Never"}
        </div>
      </div>
    </div>
  );
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Wrapped AgentsView in React.memo()
 * 🎯 Why: If parent state updates, it causes the entire list to needlessly re-render.
 * 📊 Impact: Prevents O(N) list re-rendering when unrelated parent state changes.
 */
export default React.memo(function AgentsView({ token }: { token: string }) {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadAgents = useCallback(() => {
    const api = new ApiClient(token);
    setLoading(true);
    api
      .getAgents()
      .then(setAgents)
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoading(false));
  }, [token]);

  useEffect(() => {
    loadAgents();
  }, [loadAgents]);

  return (
    <div className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-3xl font-light text-white tracking-tight">
            Agent Manager
          </h2>
          <p className="text-sm text-white/40 mt-1">
            Monitor and control active agents
          </p>
        </div>
        <button
          onClick={loadAgents}
          className="flex items-center gap-2 px-4 py-2 bg-white/5 hover:bg-white/10 border border-white/10 rounded-xl text-xs font-bold text-white transition-all"
        >
          <RefreshCw size={14} className={loading ? "animate-spin" : ""} />
          Refresh
        </button>
      </div>

      {error && (
        <div className="p-4 bg-red-500/10 border border-red-500/20 rounded-xl text-red-400 text-sm">
          Error: {error}
        </div>
      )}

      {loading && !agents.length ? (
        <div className="text-white/20 text-sm font-mono">Loading agents...</div>
      ) : agents.length === 0 ? (
        <div className="text-center py-20 text-white/20 bg-white/[0.02] border border-dashed border-white/10 rounded-[32px]">
          No active agents detected in the current workspace.
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {agents.map((agent) => (
            <AgentItem key={agent.id} agent={agent} />
          ))}
        </div>
      )}
    </div>
  );
});
