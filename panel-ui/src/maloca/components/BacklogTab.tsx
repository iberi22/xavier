import React, { useEffect, useState } from "react";
import { malocaApi, BacklogItem } from "../api";
import { ListTodo, Code2, CheckCircle2, CircleDashed } from "lucide-react";

export function BacklogTab() {
  const [items, setItems] = useState<BacklogItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    malocaApi.getBacklog()
      .then((data) => {
        if (mounted) setItems(data.items);
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
        <div className="text-emerald-400 font-mono text-sm animate-pulse">Loading Global Backlog...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-rose-950/30 border border-rose-800/50 rounded-lg text-rose-300 text-xs font-mono">
        Failed to fetch backlog: {error}
      </div>
    );
  }

  const getStatusIcon = (status: string) => {
    if (status.toLowerCase().includes('done') || status.toLowerCase().includes('complete')) {
      return <CheckCircle2 size={16} className="text-emerald-400" />;
    }
    return <CircleDashed size={16} className="text-amber-400" />;
  };

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center mb-4">
        <div>
          <h3 className="text-lg font-semibold text-white flex items-center gap-2">
            <ListTodo size={18} className="text-emerald-400" />
            Global Backlog
          </h3>
          <p className="text-xs text-white/50 mt-1">Unified task queue & work-item scheduling</p>
        </div>
      </div>

      <div className="grid gap-3">
        {items.length === 0 ? (
          <div className="text-center py-12 border border-dashed border-white/10 rounded-xl bg-[#0a0a0a]">
            <ListTodo size={32} className="mx-auto text-white/10 mb-3" />
            <p className="text-white/40 text-sm">No backlog items currently available.</p>
          </div>
        ) : (
          items.map((item) => (
            <div key={item.id} className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a] hover:bg-white/[0.02] transition-colors">
              <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-4">
                <div className="flex-1">
                  <div className="flex items-center gap-2 mb-2">
                    {getStatusIcon(item.status)}
                    <h4 className="text-white font-medium text-sm">{item.title}</h4>
                    <span className="ml-2 px-2 py-0.5 rounded text-[10px] font-mono bg-white/5 text-white/50 border border-white/10 uppercase">
                      {item.status}
                    </span>
                  </div>
                  {item.notes && (
                    <p className="text-xs text-white/60 mb-3">{item.notes}</p>
                  )}
                  <div className="flex items-center gap-3 text-xs">
                    <span className="flex items-center gap-1 text-cyan-400 font-mono">
                      <Code2 size={12} />
                      {item.repo_name}
                    </span>
                    <span className="text-white/30 font-mono">ID: {item.id}</span>
                  </div>
                </div>

                <div className="flex flex-col items-end gap-2 min-w-[120px]">
                  <div className="text-xs font-mono text-emerald-400 flex items-center gap-2">
                    <span>{item.progress_pct.toFixed(0)}% Complete</span>
                  </div>
                  <div className="w-full h-1.5 bg-white/10 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-emerald-400 rounded-full transition-all duration-500 shadow-[0_0_8px_rgba(52,211,153,0.5)]"
                      style={{ width: `${Math.max(5, item.progress_pct)}%` }}
                    />
                  </div>
                </div>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
