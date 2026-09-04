import React, { useEffect, useState } from "react";
import { malocaApi, SupportTicket } from "../api";
import { MessageSquare, Plus, Clock, Tag } from "lucide-react";

export function SupportTab() {
  const [tickets, setTickets] = useState<SupportTicket[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    malocaApi.getSupportTickets()
      .then((data) => {
        if (mounted) setTickets(data);
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
        <div className="text-emerald-400 font-mono text-sm animate-pulse">Loading Support Queue...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-rose-950/30 border border-rose-800/50 rounded-lg text-rose-300 text-xs font-mono">
        Error loading tickets: {error}
      </div>
    );
  }

  const getStatusColor = (status: string) => {
    switch (status.toLowerCase()) {
      case 'open': return 'text-emerald-400 bg-emerald-400/10 border-emerald-400/20';
      case 'closed': return 'text-white/40 bg-white/5 border-white/10';
      case 'in_progress': return 'text-cyan-400 bg-cyan-400/10 border-cyan-400/20';
      default: return 'text-amber-400 bg-amber-400/10 border-amber-400/20';
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center mb-6">
        <h3 className="text-lg font-semibold text-white flex items-center gap-2">
          <MessageSquare size={18} className="text-emerald-400" />
          Active Support Tickets
        </h3>
        <button
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-mono bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-400 border border-emerald-500/30 rounded transition-colors"
          onClick={() => alert("Create ticket functionality coming soon")}
        >
          <Plus size={14} />
          New Ticket
        </button>
      </div>

      {tickets.length === 0 ? (
        <div className="glass-panel p-12 flex flex-col items-center justify-center text-center border border-white/5 rounded-xl bg-[#0a0a0a]">
          <MessageSquare size={32} className="text-white/10 mb-4" />
          <h4 className="text-white/70 font-medium mb-1">Queue Empty</h4>
          <p className="text-white/40 text-sm">No support tickets currently require attention.</p>
        </div>
      ) : (
        <div className="grid gap-3">
          {tickets.map((ticket) => (
            <div key={ticket.id} className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a] hover:border-emerald-500/30 transition-colors group cursor-pointer">
              <div className="flex items-start justify-between gap-4">
                <div className="flex-1">
                  <div className="flex items-center gap-3 mb-2">
                    <span className="font-mono text-xs text-white/40">#{ticket.id.substring(0, 8)}</span>
                    <h4 className="text-white font-medium text-sm">{ticket.title}</h4>
                    <span className={`px-2 py-0.5 rounded text-[10px] uppercase font-mono border ${getStatusColor(ticket.status)}`}>
                      {ticket.status}
                    </span>
                  </div>
                  <p className="text-sm text-white/60 line-clamp-2">{ticket.body}</p>
                </div>
              </div>
              <div className="mt-4 flex items-center gap-4 text-xs text-white/40">
                <div className="flex items-center gap-1.5">
                  <Clock size={12} />
                  {new Date(ticket.created_at).toLocaleDateString()}
                </div>
                {ticket.feature_id && (
                  <div className="flex items-center gap-1.5">
                    <Tag size={12} className="text-cyan-400" />
                    <span className="font-mono text-cyan-400">{ticket.feature_id}</span>
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
