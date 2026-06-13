import {
  Activity,
  BarChart3,
  Brain,
  Database,
  Layers,
  LineChart,
  Network,
  RefreshCw,
  Zap,
} from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useEffect, useState } from "react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

interface Stats {
  status: string;
  memory: {
    documents: number;
    entities: number;
    relations: number;
    embedding_progress: number;
  };
  hormer?: {
    navigated_queries: number;
    non_navigated_queries: number;
    average_reward: number;
    score_histogram: number[];
  };
}

const getApiUrl = (path: string) => {
  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  return isTauri ? `http://127.0.0.1:8006${path}` : path;
};

export default function ObservabilityPage({ token }: { token: string }) {
  const [stats, setStats] = useState<Stats | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const fetchStats = useCallback(async () => {
    try {
      setIsLoading(true);
      const resp = await fetch(getApiUrl("/health"), {
        headers: { "X-Xavier-Token": token },
      });
      if (resp.ok) {
        setStats(await resp.json());
      }
    } catch (e) {
      console.error("Failed to fetch stats", e);
    } finally {
      setIsLoading(false);
    }
  }, [token]);

  useEffect(() => {
    fetchStats();
    const interval = setInterval(fetchStats, 10000);
    return () => clearInterval(interval);
  }, [fetchStats]);

  const histogramData = stats?.hormer?.score_histogram?.map((count, i) => ({
    bucket: `${(i / 10).toFixed(1)}`,
    count,
  })) || [];

  return (
    <div className="space-y-8 max-w-5xl">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-light text-white tracking-tight">System Observability</h2>
          <p className="text-sm text-white/40 mt-1">Real-time performance and memory quality metrics.</p>
        </div>
        <button
          onClick={fetchStats}
          className={`p-2 rounded-lg bg-white/5 hover:bg-white/10 transition-colors ${isLoading ? 'animate-spin' : ''}`}
        >
          <RefreshCw className="w-4 h-4 text-white/60" />
        </button>
      </div>

      {/* High-level Status Cards */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <StatusCard
          icon={<Database className="w-4 h-4" />}
          label="Documents"
          value={stats?.memory?.documents ?? 0}
          subValue="Indexed"
        />
        <StatusCard
          icon={<Brain className="w-4 h-4" />}
          label="Knowledge"
          value={(stats?.memory?.entities ?? 0) + (stats?.memory?.relations ?? 0)}
          subValue="Nodes & Edges"
        />
        <StatusCard
          icon={<Zap className="w-4 h-4" />}
          label="Avg Reward"
          value={stats?.hormer?.average_reward?.toFixed(3) ?? "0.000"}
          subValue="HORMER Policy"
          trend={stats?.hormer?.average_reward && stats.hormer.average_reward > 0.5 ? "positive" : "neutral"}
        />
        <StatusCard
          icon={<Layers className="w-4 h-4" />}
          label="Embeddings"
          value={`${((stats?.memory?.embedding_progress ?? 1) * 100).toFixed(0)}%`}
          subValue="Vector Quality"
        />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Retrieval Quality Distribution */}
        <section className="bg-white/[0.02] border border-white/[0.05] rounded-2xl p-6">
          <div className="flex items-center gap-2 mb-6">
            <BarChart3 className="w-4 h-4 text-[#39ff14]/70" />
            <h3 className="text-sm font-medium text-white/80">Retrieval Score Distribution</h3>
          </div>

          <div className="h-[250px] w-full">
            {histogramData.length > 0 ? (
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={histogramData}>
                  <defs>
                    <linearGradient id="colorCount" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="#39ff14" stopOpacity={0.3}/>
                      <stop offset="95%" stopColor="#39ff14" stopOpacity={0}/>
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" stroke="#ffffff05" vertical={false} />
                  <XAxis
                    dataKey="bucket"
                    stroke="#ffffff20"
                    fontSize={10}
                    tickLine={false}
                    axisLine={false}
                  />
                  <YAxis
                    stroke="#ffffff20"
                    fontSize={10}
                    tickLine={false}
                    axisLine={false}
                  />
                  <Tooltip
                    contentStyle={{ backgroundColor: '#0a0a0a', border: '1px solid #ffffff10', borderRadius: '8px', fontSize: '12px' }}
                    itemStyle={{ color: '#39ff14' }}
                  />
                  <Area
                    type="monotone"
                    dataKey="count"
                    stroke="#39ff14"
                    fillOpacity={1}
                    fill="url(#colorCount)"
                    strokeWidth={2}
                  />
                </AreaChart>
              </ResponsiveContainer>
            ) : (
              <div className="h-full flex items-center justify-center text-white/10 text-xs">
                Waiting for telemetry data...
              </div>
            )}
          </div>
          <p className="text-[10px] text-white/20 mt-4 italic text-center">
            Distribution of similarity scores across the last {stats?.hormer?.navigated_queries ?? 0} queries.
          </p>
        </section>

        {/* Navigation Success Rate */}
        <section className="bg-white/[0.02] border border-white/[0.05] rounded-2xl p-6">
          <div className="flex items-center gap-2 mb-6">
            <Network className="w-4 h-4 text-blue-400/70" />
            <h3 className="text-sm font-medium text-white/80">HORMER Navigation Stats</h3>
          </div>

          <div className="space-y-6">
            <StatRow
              label="Navigated Queries"
              value={stats?.hormer?.navigated_queries ?? 0}
              description="Queries that utilized guided graph search expansion."
            />
            <StatRow
              label="Direct Hits"
              value={stats?.hormer?.non_navigated_queries ?? 0}
              description="Queries satisfied by direct vector lookup."
            />
            <div className="pt-4">
              <div className="flex justify-between text-[10px] uppercase tracking-wider text-white/30 mb-2">
                <span>Success Rate</span>
                <span className="text-white/60">
                  {stats?.hormer ?
                    ((stats.hormer.navigated_queries / (stats.hormer.navigated_queries + stats.hormer.non_navigated_queries || 1)) * 100).toFixed(1) :
                    0}%
                </span>
              </div>
              <div className="h-1.5 w-full bg-white/5 rounded-full overflow-hidden">
                <motion.div
                  initial={{ width: 0 }}
                  animate={{ width: stats?.hormer ? `${(stats.hormer.navigated_queries / (stats.hormer.navigated_queries + stats.hormer.non_navigated_queries || 1)) * 100}%` : 0 }}
                  className="h-full bg-blue-500 shadow-[0_0_10px_rgba(59,130,246,0.5)]"
                />
              </div>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

function StatusCard({ icon, label, value, subValue, trend }: {
  icon: React.ReactNode,
  label: string,
  value: string | number,
  subValueText?: string,
  subValue: string,
  trend?: 'positive' | 'negative' | 'neutral'
}) {
  return (
    <div className="bg-white/[0.02] border border-white/[0.05] rounded-2xl p-5">
      <div className="flex items-center gap-2 text-white/40 mb-3">
        {icon}
        <span className="text-[10px] uppercase tracking-widest font-medium">{label}</span>
      </div>
      <div className="flex items-baseline gap-2">
        <div className="text-2xl font-mono text-white/90">{value}</div>
        {trend === 'positive' && <div className="text-[10px] text-green-500">↑</div>}
      </div>
      <div className="text-[10px] text-white/20 mt-1">{subValue}</div>
    </div>
  );
}

function StatRow({ label, value, description }: { label: string, value: number, description: string }) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div>
        <div className="text-xs text-white/70 font-medium">{label}</div>
        <div className="text-[10px] text-white/20 mt-0.5 leading-relaxed">{description}</div>
      </div>
      <div className="text-lg font-mono text-white/90">{value}</div>
    </div>
  );
}
