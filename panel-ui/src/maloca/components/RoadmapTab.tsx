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
    <section className="glass-panel p-4 border border-[var(--border-subtle)] rounded-xl bg-[var(--surface)]" aria-label={title}>
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
        <h4 className="text-[var(--foreground)] font-medium text-sm">{title}</h4>
        <div className="flex flex-wrap gap-2 text-[11px] font-mono text-[color:color-mix(in_srgb,var(--foreground)_60%,transparent)]">
          <span>{summary.total} total</span>
          <span className="text-emerald-400">{summary.stable} stable</span>
          <span className="text-amber-400">{summary.in_progress} in progress</span>
          <span className="text-cyan-400">{summary.planned} planned</span>
        </div>
      </div>

      <div className="mt-3 flex items-center gap-3">
        <div className="flex-1 h-1.5 bg-[color:color-mix(in_srgb,var(--foreground)_10%,transparent)] rounded-full overflow-hidden">
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
            className="flex items-center gap-1 text-xs font-mono text-[color:color-mix(in_srgb,var(--foreground)_60%,transparent)] hover:text-[var(--foreground)] transition-colors"
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
                  className="flex items-center justify-between gap-3 text-xs border-t border-[var(--border-subtle)] pt-1.5"
                >
                  <span className="text-[color:color-mix(in_srgb,var(--foreground)_80%,transparent)]">
                    {feature.name}
                    <span className="ml-2 font-mono text-[color:color-mix(in_srgb,var(--foreground)_30%,transparent)]">{feature.id}</span>
                  </span>
                  <span className="font-mono text-[color:color-mix(in_srgb,var(--foreground)_50%,transparent)] whitespace-nowrap">
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
      <div className="text-center py-12 border border-dashed border-[var(--border-subtle)] rounded-xl bg-[var(--surface)]">
        <Map size={32} className="mx-auto text-[color:color-mix(in_srgb,var(--foreground)_10%,transparent)] mb-3" />
        <p className="text-[color:color-mix(in_srgb,var(--foreground)_40%,transparent)] text-sm">No roadmap features are available yet.</p>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div>
        <h3 className="text-lg font-semibold text-[var(--foreground)] flex items-center gap-2">
          <Map size={18} className="text-emerald-400" />
          Roadmap
        </h3>
        <p className="text-xs text-[color:color-mix(in_srgb,var(--foreground)_50%,transparent)] mt-1">Feature progress grouped by wave</p>
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
