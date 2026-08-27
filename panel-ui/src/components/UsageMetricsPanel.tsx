import {
	Activity,
	AlertTriangle,
	Coins,
	FileText,
	Layers,
	RefreshCw,
	TrendingUp,
} from "lucide-react";
import type React from "react";
import { useCallback, useEffect, useMemo, useState } from "react";

const getApiUrl = (path: string) => {
	const isTauri =
		typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
	return isTauri ? `http://127.0.0.1:8006${path}` : path;
};

interface ProviderUsage {
	requests: number;
	tokens: number;
	errors: number;
	cost_usd: number;
}

interface UsageResponse {
	status: string;
	requests_used: number;
	total_tokens: number;
	total_errors: number;
	total_cost_usd: number;
	memory_fallback_hits: number;
	fallback_chain_hops: number;
	by_provider: Record<string, ProviderUsage>;
}

interface UsageMetricsPanelProps {
	token: string;
}

export default function UsageMetricsPanel({ token }: UsageMetricsPanelProps) {
	const [data, setData] = useState<UsageResponse | null>(null);
	const [loading, setLoading] = useState(true);
	const [refreshing, setRefreshing] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const fetchUsage = useCallback(
		async (isManual = false) => {
			if (isManual) {
				setRefreshing(true);
			}
			try {
				const response = await fetch(getApiUrl("/v1/account/usage"), {
					headers: {
						"Content-Type": "application/json",
						"X-Xavier-Token": token,
					},
				});

				if (!response.ok) {
					throw new Error(`Error: ${response.status} ${response.statusText}`);
				}

				const usageData = (await response.json()) as UsageResponse;
				setData(usageData);
				setError(null);
			} catch (err) {
				console.error("Failed to fetch usage metrics:", err);
				setError(
					err instanceof Error ? err.message : "Failed to load usage data",
				);
			} finally {
				setLoading(false);
				setRefreshing(false);
			}
		},
		[token],
	);

	useEffect(() => {
		fetchUsage();

		const interval = setInterval(() => {
			fetchUsage();
		}, 5000); // Polling every 5 seconds

		return () => clearInterval(interval);
	}, [fetchUsage]);

	const providerRows = useMemo(() => {
		const isLocalOrOllama = (name: string) => {
			const normalized = name.toLowerCase();
			return normalized.includes("local") || normalized.includes("ollama");
		};

		if (!data?.by_provider || Object.entries(data.by_provider).length === 0) {
			return (
				<tr>
					<td
						colSpan={6}
						className="px-6 py-8 text-center text-sm text-white/30"
					>
						No active provider traffic logs recorded yet.
					</td>
				</tr>
			);
		}

		return Object.entries(data.by_provider).map(([providerName, usage]) => {
			const isLocal = isLocalOrOllama(providerName);
			return (
				<tr
					key={providerName}
					className={`transition-colors group ${
						isLocal
							? "bg-[#39ff14]/5 hover:bg-[#39ff14]/10"
							: "hover:bg-white/[0.02]"
					}`}
				>
					<td className="px-6 py-4">
						<span
							className={`text-sm font-medium capitalize ${
								isLocal ? "text-[#39ff14] font-semibold" : "text-white"
							}`}
						>
							{providerName}
						</span>
					</td>
					<td className="px-6 py-4 font-mono text-xs text-white/80">
						{usage.requests}
					</td>
					<td className="px-6 py-4 font-mono text-xs text-white/80">
						{usage.tokens.toLocaleString()}
					</td>
					<td
						className={`px-6 py-4 font-mono text-xs ${
							usage.errors > 0 ? "text-red-400 font-bold" : "text-white/80"
						}`}
					>
						{usage.errors}
					</td>
					<td className="px-6 py-4 font-mono text-xs text-white/80">
						${usage.cost_usd.toFixed(4)}
					</td>
					<td className="px-6 py-4">
						<div className="flex justify-center">
							{isLocal ? (
								<span className="text-[10px] text-[#39ff14] bg-[#39ff14]/10 border border-[#39ff14]/30 px-2 py-0.5 rounded-full font-bold uppercase tracking-wider shadow-[0_0_8px_rgba(57,255,20,0.2)]">
									⚡ Local
								</span>
							) : (
								<span className="text-[10px] text-blue-400 bg-blue-500/10 border border-blue-500/20 px-2 py-0.5 rounded-full font-bold uppercase tracking-wider">
									☁️ Cloud
								</span>
							)}
						</div>
					</td>
				</tr>
			);
		});
	}, [data?.by_provider]);

	if (loading) {
		return (
			<div className="flex flex-col items-center justify-center h-full gap-3">
				<RefreshCw className="w-8 h-8 text-[#39ff14] animate-spin opacity-75" />
				<span className="text-sm text-white/50 font-mono">
					Loading telemetry...
				</span>
			</div>
		);
	}

	return (
		<div className="flex flex-col h-full p-8 text-white space-y-8 overflow-y-auto">
			{/* Header */}
			<div className="flex items-center justify-between">
				<div>
					<h2 className="text-3xl font-light tracking-tight">
						Usage & Telemetry
					</h2>
					<p className="text-sm text-white/40 mt-1">
						Real-time local vs cloud traffic audit logs and cost estimation.
					</p>
				</div>
				<button
					type="button"
					onClick={() => fetchUsage(true)}
					disabled={refreshing}
					className="flex items-center gap-2 px-4 py-2 bg-white/5 hover:bg-white/10 border border-white/10 hover:border-white/20 text-white/80 rounded-xl text-sm font-bold transition-all disabled:opacity-50"
					title="Manual refresh"
				>
					<RefreshCw
						className={`w-4 h-4 ${refreshing ? "animate-spin text-[#39ff14]" : ""}`}
					/>
					{refreshing ? "Refreshing..." : "Refresh"}
				</button>
			</div>

			{error && (
				<div className="p-4 rounded-xl bg-red-500/10 border border-red-500/20 text-red-400 text-sm flex items-center gap-2">
					<AlertTriangle className="w-4 h-4 shrink-0" />
					<span>{error}</span>
				</div>
			)}

			{data && (
				<>
					{/* Metrics Grid */}
					<div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4">
						<MetricCard
							title="Total Requests"
							value={data.requests_used}
							icon={<Activity className="w-4 h-4 text-cyan-400" />}
						/>
						<MetricCard
							title="Total Tokens"
							value={data.total_tokens.toLocaleString()}
							icon={<FileText className="w-4 h-4 text-purple-400" />}
						/>
						<MetricCard
							title="Total Cost"
							value={`$${data.total_cost_usd.toFixed(4)}`}
							icon={<Coins className="w-4 h-4 text-yellow-500" />}
						/>
						<MetricCard
							title="Total Errors"
							value={data.total_errors}
							icon={<AlertTriangle className="w-4 h-4 text-red-500" />}
							isAlert={data.total_errors > 0}
						/>
						<MetricCard
							title="Memory Hits"
							value={data.memory_fallback_hits}
							icon={<Layers className="w-4 h-4 text-emerald-400" />}
							isSuccess={data.memory_fallback_hits > 0}
						/>
						<MetricCard
							title="Chain Hops"
							value={data.fallback_chain_hops}
							icon={<TrendingUp className="w-4 h-4 text-orange-400" />}
						/>
					</div>

					{/* Providers Table */}
					<div className="space-y-4">
						<div className="flex items-center gap-2 px-2">
							<TrendingUp className="w-4 h-4 text-white/40" />
							<h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/40">
								Provider-Specific Telemetry
							</h3>
						</div>

						<div className="w-full overflow-hidden border border-white/5 rounded-2xl bg-[#050505]/30">
							<table className="w-full text-left border-collapse">
								<thead>
									<tr className="bg-white/5 border-b border-white/5">
										<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
											Provider
										</th>
										<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
											Requests
										</th>
										<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
											Tokens Used
										</th>
										<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
											Errors
										</th>
										<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
											Estimated Cost
										</th>
										<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold text-center">
											Mode
										</th>
									</tr>
								</thead>
								<tbody className="divide-y divide-white/5">
									{providerRows}
								</tbody>
							</table>
						</div>
					</div>
				</>
			)}
		</div>
	);
}

interface MetricCardProps {
	title: string;
	value: React.ReactNode;
	icon: React.ReactNode;
	isAlert?: boolean;
	isSuccess?: boolean;
}

function MetricCard({
	title,
	value,
	icon,
	isAlert,
	isSuccess,
}: MetricCardProps) {
	let borderStyle = "border-white/5";
	let glowStyle = "";
	if (isAlert) {
		borderStyle = "border-red-500/20";
		glowStyle = "shadow-[0_0_12px_rgba(239,68,68,0.05)] bg-red-500/[0.02]";
	} else if (isSuccess) {
		borderStyle = "border-emerald-500/20";
		glowStyle = "shadow-[0_0_12px_rgba(16,185,129,0.05)] bg-emerald-500/[0.02]";
	}

	return (
		<div
			className={`p-5 bg-[#050505]/40 backdrop-blur-md rounded-2xl border ${borderStyle} ${glowStyle} flex flex-col justify-between h-28 hover:border-white/10 transition-colors`}
		>
			<div className="flex items-center justify-between gap-2">
				<span className="text-[9px] uppercase tracking-wider text-white/40 font-bold whitespace-nowrap">
					{title}
				</span>
				<div className="p-1.5 bg-white/5 rounded-lg shrink-0">{icon}</div>
			</div>
			<span className="text-xl font-semibold font-mono tracking-tight truncate text-white">
				{value}
			</span>
		</div>
	);
}
