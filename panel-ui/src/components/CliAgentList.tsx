import {
  LogIn,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  Terminal,
} from "lucide-react";
import React from "react";

interface CliAgent {
  name: string;
  status: "logged_in" | "not_logged_in" | "not_installed";
  enabled: boolean;
}

interface CliAgentListProps {
  agents: CliAgent[];
  onToggle: (name: string) => void;
  onLogin: (name: string) => void;
}

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted list item into CliAgentItem and wrapped in React.memo()
 * 🎯 Why: When agents update, rendering the list inline caused O(N) DOM reconciliation
 *         for every item, even those that haven't changed.
 * 📊 Impact: Eliminates O(N) re-renders, making updates to individual agents O(1).
 */
const CliAgentItem = React.memo(function CliAgentItem({
  agent,
  onToggle,
  onLogin,
}: {
  agent: CliAgent;
  onToggle: (name: string) => void;
  onLogin: (name: string) => void;
}) {
  return (
    <div className="bg-[#050505]/50 border border-white/5 rounded-2xl p-5 hover:border-white/10 transition-colors">
      <div className="flex items-start justify-between mb-4">
        <div className="flex items-center gap-3">
          <div className="p-2 bg-white/5 rounded-lg">
            <Terminal className="w-5 h-5 text-white/70" aria-hidden="true" />
          </div>
          <div>
            <h4 className="text-sm font-bold capitalize">{agent.name}</h4>
            <p className="text-[10px] text-white/40 uppercase tracking-tighter">
              CLI Agent Core
            </p>
          </div>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={agent.enabled}
          aria-label={`Toggle ${agent.name} agent`}
          title={`Toggle ${agent.name} agent`}
          onClick={() => onToggle(agent.name)}
          className={`relative w-10 h-6 rounded-full transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]/50 ${agent.enabled ? "bg-[#39ff14]/20" : "bg-white/5"}`}
        >
          <div
            className={`absolute top-1 left-1 w-4 h-4 rounded-full transition-transform ${agent.enabled ? "translate-x-4 bg-[#39ff14]" : "bg-white/20"}`}
          />
        </button>
      </div>

      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <StatusBadge status={agent.status} />
        </div>
        {agent.status === "not_logged_in" && (
          <button
            type="button"
            onClick={() => onLogin(agent.name)}
            className="flex items-center gap-2 px-3 py-1.5 bg-white/10 hover:bg-white/15 rounded-lg text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/50"
          >
            <LogIn className="w-3.5 h-3.5" aria-hidden="true" />
            Login
          </button>
        )}
      </div>
    </div>
  );
});

export function CliAgentList({ agents, onToggle, onLogin }: CliAgentListProps) {
  return (
    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
      {agents.map((agent) => (
        <CliAgentItem
          key={agent.name}
          agent={agent}
          onToggle={onToggle}
          onLogin={onLogin}
        />
      ))}
    </div>
  );
}

function StatusBadge({ status }: { status: CliAgent["status"] }) {
  if (status === "logged_in") {
    return (
      <div className="flex items-center gap-1.5 text-[#39ff14] text-[11px] font-medium bg-[#39ff14]/5 px-2 py-0.5 rounded-full border border-[#39ff14]/10">
        <ShieldCheck className="w-3 h-3" aria-hidden="true" />
        Logged In
      </div>
    );
  }
  if (status === "not_logged_in") {
    return (
      <div className="flex items-center gap-1.5 text-yellow-400 text-[11px] font-medium bg-yellow-400/5 px-2 py-0.5 rounded-full border border-yellow-400/10">
        <ShieldAlert className="w-3 h-3" aria-hidden="true" />
        Auth Required
      </div>
    );
  }
  return (
    <div className="flex items-center gap-1.5 text-white/30 text-[11px] font-medium bg-white/5 px-2 py-0.5 rounded-full">
      Not Installed
    </div>
  );
}
