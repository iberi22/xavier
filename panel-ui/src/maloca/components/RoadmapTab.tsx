import React, { useEffect, useState } from "react";
import { Map, ChevronDown, ChevronRight } from "lucide-react";
import { malocaApi, RoadmapResponse, RoadmapSummary, RoadmapWave } from "../api";

/** Waves in ascending order. The server already sorts them; the panel does not rely on it. */
export function sortWaves(waves: RoadmapWave[]): RoadmapWave[] {
  return [...waves].sort((a, b) => a.wave - b.wave);
}

/** True when the roadmap has no features at all (no waves and nothing unscheduled). */
export function isRoadmapEmpty(roadmap: RoadmapResponse): boolean {
  return roadmap.waves.length === 0 && roadmap.unscheduled.total === 0;
}

/** Clamps a percentage into the 0-100 range for the progress bar width. */
export function clampPct(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(100, Math.max(0, value));
}

interface RoadmapCardProps {
  title: string;
  summary: RoadmapSummary;
}

function RoadmapCard({ title, summary }: RoadmapCardProps) {
  const [expanded, setExpanded] = useState(false);
  const pct = clampPct(summary.progress_pct);
  const listId = `roadmap-features-${title.replace(/\s+/g, "-").toLowerCase()}`;

  return (
    <section className="glass-panel p-4 border border-white/5 rounded-xl bg-[#0a0a0a]" aria-label={title}>
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <h4 className="text-white font-medium text-sm">{title}</h4>
        <div className="flex flex-wrap gap-2 text-[11px] font-mono text-white/60">
          <span>{summary.total} total</span>
          <span className="text-emerald-400">{summary.stable} stable</span>
          <span className="text-amber-400">{summary.in_progress} in progress</span>
          <span className="text-cyan-400">{summary.planned} planned</span>
        </div>
      </div>

      <div className="mt-3 flex items-center gap-3">
        <div className="flex-1 h-1.5 bg-white/10 rounded-full overflow-hidden">
          <div
            className="h-full bg-emerald-400 rounded-full transition-all duration-500"
            style={{ width: `${pct}%` }}
            data-testid="roadmap-progress-bar"
          />
        </div>
        <span className="text-xs font-mono text-emerald-400 min-w-[3.5rem] text-right">
          {Math.round(pct)}%
        </span>
      </div>

      {summary.features.length > 0 && (
        <div className="mt-3">
          <button
            type="button"
            className="flex items-center gap-1 text-xs font-mono text-white/60 hover:text-white transition-colors"
            aria-expanded={expanded}
            aria-controls={listId}
            onClick={() => setExpanded((v) => !v)}
          >
            {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            {expanded ? "Hide features" : `Show ${summary.features.length} features`}
          </button>
          {expanded && (
            <ul id={listId} className="mt-2 space-y-1.5">
              {summary.features.map((feature) => (
                <li
                  key={feature.id}
                  className="flex items-center justify-between gap-3 text-xs border-t border-white/5 pt-1.5"
                >
                  <span className="text-white/80">
                    {feature.name}
                    <span className="ml-2 font-mono text-white/30">{feature.id}</span>
                  </span>
                  <span className="font-mono text-white/50 whitespace-nowrap">
                    {feature.status} · {Math.round(clampPct(feature.progress_pct))}%
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}

export function RoadmapTab() {
  const [roadmap, setRoadmap] = useState<RoadmapResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    malocaApi.getRoadmap()
      .then((data) => {
        if (mounted) setRoadmap(data);
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
        <div className="text-emerald-400 font-mono text-sm animate-pulse">Loading Roadmap...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-rose-950/30 border border-rose-800/50 rounded-lg text-rose-300 text-xs font-mono">
        Failed to fetch roadmap: {error}
      </div>
    );
  }

  if (!roadmap || isRoadmapEmpty(roadmap)) {
    return (
      <div className="text-center py-12 border border-dashed border-white/10 rounded-xl bg-[#0a0a0a]">
        <Map size={32} className="mx-auto text-white/10 mb-3" />
        <p className="text-white/40 text-sm">No roadmap features are available yet.</p>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div>
        <h3 className="text-lg font-semibold text-white flex items-center gap-2">
          <Map size={18} className="text-emerald-400" />
          Roadmap
        </h3>
        <p className="text-xs text-white/50 mt-1">Feature progress grouped by wave</p>
      </div>

      <div className="grid gap-3">
        {sortWaves(roadmap.waves).map((wave) => (
          <RoadmapCard key={wave.wave} title={`Wave ${wave.wave}`} summary={wave} />
        ))}
        <RoadmapCard title="Unscheduled" summary={roadmap.unscheduled} />
      </div>
    </div>
  );
}
