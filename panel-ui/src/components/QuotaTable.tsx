import { RefreshCw } from "lucide-react";
import React from "react";

interface Quota {
	provider: string;
	tier: string;
	requests: string;
	tokens: string;
	reset: string;
	status: "green" | "yellow" | "red";
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
						<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
							Provider
						</th>
						<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
							Tier
						</th>
						<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
							Requests
						</th>
						<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
							Tokens
						</th>
						<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold">
							Reset
						</th>
						<th className="px-6 py-4 text-[10px] uppercase tracking-widest text-white/40 font-bold text-center">
							Status
						</th>
					</tr>
				</thead>
				<tbody className="divide-y divide-white/5">
					{quotas.map((q) => (
						<QuotaRow key={q.provider} quota={q} />
					))}
				</tbody>
			</table>
		</div>
	);
}

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted table row into QuotaRow and wrapped in React.memo()
 * 🎯 Why: When rendering the quota table, the inline row mapping caused an O(N) re-render for all rows whenever the parent re-rendered or an individual quota updated.
 * 📊 Impact: Eliminates unnecessary re-renders of all rows, changing the update cost from O(N) to O(1) for un-modified quotas.
 */
const QuotaRow = React.memo(function QuotaRow({ quota }: { quota: Quota }) {
	return (
		<tr className="hover:bg-white/[0.02] transition-colors group">
			<td className="px-6 py-4">
				<div className="flex items-center gap-3">
					<span className="text-sm font-medium capitalize">
						{quota.provider}
					</span>
				</div>
			</td>
			<td className="px-6 py-4">
				<span className="text-xs text-white/60 bg-white/5 px-2 py-0.5 rounded border border-white/10 uppercase tracking-tighter">
					{quota.tier}
				</span>
			</td>
			<td className="px-6 py-4 font-mono text-xs text-white/80">
				{quota.requests}
			</td>
			<td className="px-6 py-4 font-mono text-xs text-white/80">
				{quota.tokens}
			</td>
			<td className="px-6 py-4">
				<div className="flex items-center gap-1.5 text-white/40 text-xs">
					<RefreshCw className="w-3 h-3" />
					{quota.reset}
				</div>
			</td>
			<td className="px-6 py-4">
				<div className="flex justify-center">
					<div
						className={`w-2 h-2 rounded-full shadow-[0_0_8px] ${
							quota.status === "green"
								? "bg-[#39ff14] shadow-[#39ff14]/50"
								: quota.status === "yellow"
									? "bg-yellow-400 shadow-yellow-400/50"
									: "bg-red-500 shadow-red-500/50"
						}`}
					/>
				</div>
			</td>
		</tr>
	);
});
