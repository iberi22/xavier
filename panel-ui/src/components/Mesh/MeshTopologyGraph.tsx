import {
	AlertTriangle,
	Database,
	RefreshCw,
	Server,
	Trash2,
	User,
	X,
	Zap,
} from "lucide-react";
import React, { useMemo, useState } from "react";
import type { ClearanceLevel, MeshPeer, MeshRole } from "../../types";

export type NodeCategory = "master" | "storage" | "employee";

export interface TopologyNode {
	id: string;
	alias: string;
	category: NodeCategory;
	role: MeshRole | "master";
	clearance: ClearanceLevel;
	latencyMs: number;
	status: "online" | "degraded" | "offline";
	endpointUrl?: string;
	isLocal?: boolean;
}

export interface MeshTopologyGraphProps {
	localNodeId?: string;
	peers?: MeshPeer[];
	customNodes?: TopologyNode[];
	onDisconnectPeer?: (nodeId: string) => Promise<void> | void;
}

export function buildTopologyNodes(
	localNodeId: string = "local-master-01",
	peers: MeshPeer[] = [],
): TopologyNode[] {
	const masterNode: TopologyNode = {
		id: localNodeId,
		alias: "Master Host",
		category: "master",
		role: "master",
		clearance: "top_secret",
		latencyMs: 1,
		status: "online",
		isLocal: true,
	};

	const storageSubNode: TopologyNode = {
		id: "storage-node-vault",
		alias: "Private Storage Vault",
		category: "storage",
		role: "admin",
		clearance: "top_secret",
		latencyMs: 4,
		status: "online",
		endpointUrl: "http://storage-internal.vault:8080",
	};

	const mappedPeers: TopologyNode[] = peers.map((peer, idx) => {
		const isStorage =
			peer.alias?.toLowerCase().includes("storage") ||
			peer.node_id.toLowerCase().includes("storage");
		const category: NodeCategory = isStorage ? "storage" : "employee";

		const nowSecs = Date.now() / 1000;
		const diff = peer.last_seen_at ? nowSecs - peer.last_seen_at : 999;
		const status: "online" | "degraded" | "offline" =
			diff < 120 ? "online" : diff < 600 ? "degraded" : "offline";
		const latencyMs =
			status === "online" ? 15 + (idx % 5) * 8 : diff > 600 ? 999 : 140;

		return {
			id: peer.node_id,
			alias: peer.alias || `Node-${peer.node_id.slice(0, 8)}`,
			category,
			role: peer.role,
			clearance: peer.clearance,
			latencyMs,
			status,
			endpointUrl: peer.endpoint_url,
			isLocal: false,
		};
	});

	return [masterNode, storageSubNode, ...mappedPeers];
}

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Wrapped filtered nodes in useMemo() and MeshTopologyGraph in React.memo()
 * 🎯 Why: filtered nodes was re-calculated on every render of MeshTopologyGraph, doing O(N) filters.
 *         Additionally, MeshTopologyGraph itself was unmemoized, leading to unnecessary re-renders when parent state changed.
 * 📊 Impact: O(1) filtering on re-renders, preventing expensive filtering and DOM reconciliation on every un-related state change.
 */
export const MeshTopologyGraph: React.FC<MeshTopologyGraphProps> = React.memo(
	({
		localNodeId = "local-master-01",
		peers = [],
		customNodes,
		onDisconnectPeer,
	}) => {
		const nodes = useMemo(() => {
			return customNodes || buildTopologyNodes(localNodeId, peers);
		}, [customNodes, localNodeId, peers]);

		const masterNodes = useMemo(
			() => nodes.filter((n) => n.category === "master"),
			[nodes],
		);
		const storageNodes = useMemo(
			() => nodes.filter((n) => n.category === "storage"),
			[nodes],
		);
		const employeeNodes = useMemo(
			() => nodes.filter((n) => n.category === "employee"),
			[nodes],
		);

		const [selectedNodeForRevoke, setSelectedNodeForRevoke] =
			useState<TopologyNode | null>(null);
		const [confirmInput, setConfirmInput] = useState("");
		const [isDisconnecting, setIsDisconnecting] = useState(false);
		const [errorMessage, setErrorMessage] = useState<string | null>(null);

		const handleOpenRevokeModal = (node: TopologyNode) => {
			setSelectedNodeForRevoke(node);
			setConfirmInput("");
			setErrorMessage(null);
		};

		const handleCloseRevokeModal = () => {
			setSelectedNodeForRevoke(null);
			setConfirmInput("");
			setErrorMessage(null);
		};

		const handleExecuteDisconnect = async () => {
			if (!selectedNodeForRevoke) return;
			if (confirmInput.trim().toUpperCase() !== "DISCONNECT") {
				setErrorMessage("Please type DISCONNECT to confirm revocation.");
				return;
			}

			try {
				setIsDisconnecting(true);
				setErrorMessage(null);
				if (onDisconnectPeer) {
					await onDisconnectPeer(selectedNodeForRevoke.id);
				}
				handleCloseRevokeModal();
			} catch (err) {
				setErrorMessage(
					err instanceof Error ? err.message : "Failed to disconnect node.",
				);
			} finally {
				setIsDisconnecting(false);
			}
		};

		const getClearanceBadgeClass = (level: ClearanceLevel) => {
			switch (level) {
				case "top_secret":
					return "bg-purple-500/20 text-purple-300 border-purple-500/30";
				case "secret":
					return "bg-rose-500/20 text-rose-300 border-rose-500/30";
				case "confidential":
					return "bg-amber-500/20 text-amber-300 border-amber-500/30";
				default:
					return "bg-slate-500/20 text-slate-300 border-slate-500/30";
			}
		};

		const getStatusDot = (status: "online" | "degraded" | "offline") => {
			switch (status) {
				case "online":
					return "bg-[#39ff14] shadow-[0_0_8px_#39ff14]";
				case "degraded":
					return "bg-amber-400 shadow-[0_0_8px_#fbbf24]";
				case "offline":
					return "bg-rose-500 shadow-[0_0_8px_#f43f5e]";
			}
		};

		return (
			<div className="space-y-6 bg-black/40 p-6 rounded-2xl border border-white/10 text-white">
				{/* Header */}
				<div className="flex items-center justify-between border-b border-white/10 pb-4">
					<div>
						<h3 className="text-lg font-semibold tracking-wide flex items-center gap-2">
							<Server className="w-5 h-5 text-[#39ff14]" />
							Visual Mesh Topology Graph
						</h3>
						<p className="text-xs text-white/50 mt-0.5">
							Active P2P node mesh hierarchy: Master Host, Storage Sub-Nodes,
							and Employee Nodes
						</p>
					</div>
					<div className="flex items-center gap-4 text-xs">
						<span className="flex items-center gap-1.5 text-white/70">
							<span className="w-2.5 h-2.5 rounded-full bg-[#39ff14]" /> Online
						</span>
						<span className="flex items-center gap-1.5 text-white/70">
							<span className="w-2.5 h-2.5 rounded-full bg-amber-400" />{" "}
							Degraded
						</span>
						<span className="flex items-center gap-1.5 text-white/70">
							<span className="w-2.5 h-2.5 rounded-full bg-rose-500" /> Offline
						</span>
					</div>
				</div>

				{/* Visual Topology Diagram */}
				<div className="relative py-8 px-4 rounded-xl bg-gradient-to-b from-white/[0.03] to-white/[0.01] border border-white/5 space-y-12 overflow-x-auto">
					{/* Tier 1: Master Host */}
					<div className="flex flex-col items-center">
						<div className="text-[10px] uppercase tracking-widest text-[#39ff14]/80 font-mono mb-3">
							Tier 1 — Master Host
						</div>
						<div className="flex justify-center gap-6">
							{masterNodes.map((node) => (
								<div
									key={node.id}
									className="relative p-4 rounded-xl bg-black/80 border border-[#39ff14]/40 shadow-[0_0_15px_rgba(57,255,20,0.15)] min-w-[240px] flex flex-col items-center space-y-2"
								>
									<div className="w-12 h-12 rounded-full bg-[#39ff14]/10 border border-[#39ff14]/30 flex items-center justify-center text-[#39ff14]">
										<Server className="w-6 h-6" />
									</div>
									<div className="text-center">
										<div className="text-sm font-medium text-white flex items-center justify-center gap-2">
											{node.alias}
											<span
												className={`w-2 h-2 rounded-full ${getStatusDot(node.status)}`}
											/>
										</div>
										<code className="text-[10px] text-white/40 font-mono block">
											{node.id}
										</code>
									</div>
									<div className="flex flex-wrap gap-1.5 justify-center pt-1">
										<span className="px-2 py-0.5 rounded text-[10px] font-mono uppercase bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/30">
											Master
										</span>
										<span
											className={`px-2 py-0.5 rounded text-[10px] font-mono uppercase border ${getClearanceBadgeClass(node.clearance)}`}
										>
											{node.clearance}
										</span>
										<span className="px-2 py-0.5 rounded text-[10px] font-mono bg-white/5 text-white/60 border border-white/10 flex items-center gap-1">
											<Zap className="w-3 h-3 text-[#39ff14]" />
											{node.latencyMs}ms
										</span>
									</div>
								</div>
							))}
						</div>
					</div>

					{/* Tier 2: Storage Sub-Nodes */}
					<div className="flex flex-col items-center relative">
						<div className="text-[10px] uppercase tracking-widest text-cyan-400/80 font-mono mb-3">
							Tier 2 — Storage Sub-Nodes
						</div>
						<div className="flex justify-center flex-wrap gap-6">
							{storageNodes.length === 0 ? (
								<p className="text-xs text-white/30 italic">
									No storage sub-nodes attached
								</p>
							) : (
								storageNodes.map((node) => (
									<div
										key={node.id}
										className="relative p-4 rounded-xl bg-black/70 border border-cyan-500/30 shadow-[0_0_12px_rgba(6,182,212,0.1)] min-w-[220px] flex flex-col items-center space-y-2"
									>
										<div className="w-10 h-10 rounded-full bg-cyan-500/10 border border-cyan-500/30 flex items-center justify-center text-cyan-400">
											<Database className="w-5 h-5" />
										</div>
										<div className="text-center">
											<div className="text-sm font-medium text-white flex items-center justify-center gap-2">
												{node.alias}
												<span
													className={`w-2 h-2 rounded-full ${getStatusDot(node.status)}`}
												/>
											</div>
											<code className="text-[10px] text-white/40 font-mono block">
												{node.id}
											</code>
										</div>
										<div className="flex flex-wrap gap-1.5 justify-center pt-1">
											<span className="px-2 py-0.5 rounded text-[10px] font-mono uppercase bg-cyan-500/20 text-cyan-300 border border-cyan-500/30">
												Storage
											</span>
											<span
												className={`px-2 py-0.5 rounded text-[10px] font-mono uppercase border ${getClearanceBadgeClass(node.clearance)}`}
											>
												{node.clearance}
											</span>
											<span className="px-2 py-0.5 rounded text-[10px] font-mono bg-white/5 text-white/60 border border-white/10 flex items-center gap-1">
												<Zap className="w-3 h-3 text-cyan-400" />
												{node.latencyMs}ms
											</span>
										</div>
									</div>
								))
							)}
						</div>
					</div>

					{/* Tier 3: Employee / Member Nodes */}
					<div className="flex flex-col items-center">
						<div className="text-[10px] uppercase tracking-widest text-amber-400/80 font-mono mb-3">
							Tier 3 — Employee & Member Nodes
						</div>
						<div className="flex justify-center flex-wrap gap-6">
							{employeeNodes.length === 0 ? (
								<div className="p-6 rounded-xl border border-dashed border-white/10 text-center">
									<p className="text-xs text-white/40">
										No employee nodes currently connected in mesh.
									</p>
								</div>
							) : (
								employeeNodes.map((node) => (
									<div
										key={node.id}
										className="relative p-4 rounded-xl bg-black/60 border border-white/10 hover:border-white/20 transition-all min-w-[230px] flex flex-col items-center space-y-3"
									>
										<div className="w-10 h-10 rounded-full bg-amber-500/10 border border-amber-500/30 flex items-center justify-center text-amber-400">
											<User className="w-5 h-5" />
										</div>
										<div className="text-center">
											<div className="text-sm font-medium text-white flex items-center justify-center gap-2">
												{node.alias}
												<span
													className={`w-2 h-2 rounded-full ${getStatusDot(node.status)}`}
												/>
											</div>
											<code className="text-[10px] text-white/40 font-mono block">
												{node.id}
											</code>
										</div>
										<div className="flex flex-wrap gap-1.5 justify-center">
											<span className="px-2 py-0.5 rounded text-[10px] font-mono uppercase bg-amber-500/20 text-amber-300 border border-amber-500/30">
												{node.role}
											</span>
											<span
												className={`px-2 py-0.5 rounded text-[10px] font-mono uppercase border ${getClearanceBadgeClass(node.clearance)}`}
											>
												{node.clearance}
											</span>
											<span className="px-2 py-0.5 rounded text-[10px] font-mono bg-white/5 text-white/60 border border-white/10 flex items-center gap-1">
												<Zap className="w-3 h-3 text-amber-400" />
												{node.latencyMs}ms
											</span>
										</div>

										{/* Offboard Disconnect Button */}
										<button
											type="button"
											onClick={() => handleOpenRevokeModal(node)}
											aria-label={`Disconnect & Purge ${node.alias}`}
											className="w-full mt-2 px-3 py-1.5 bg-rose-500/15 hover:bg-rose-500/30 border border-rose-500/40 text-rose-300 rounded-lg text-xs font-medium flex items-center justify-center gap-1.5 transition-all focus:ring-2 focus:ring-rose-500/50"
										>
											<Trash2 className="w-3.5 h-3.5" aria-hidden="true" />
											Disconnect & Purge
										</button>
									</div>
								))
							)}
						</div>
					</div>
				</div>

				{/* Revocation & Offboarding Confirmation Modal (Anti-Hallucination Guard) */}
				{selectedNodeForRevoke && (
					<div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
						<div className="bg-slate-900 border border-rose-500/40 rounded-2xl max-w-md w-full p-6 space-y-5 shadow-2xl relative">
							<button
								type="button"
								onClick={handleCloseRevokeModal}
								aria-label="Close modal"
								className="absolute top-4 right-4 text-white/40 hover:text-white transition-colors"
							>
								<X className="w-5 h-5" aria-hidden="true" />
							</button>

							<div className="flex items-center gap-3 text-rose-400">
								<div className="w-10 h-10 rounded-full bg-rose-500/20 border border-rose-500/40 flex items-center justify-center">
									<AlertTriangle className="w-6 h-6" />
								</div>
								<div>
									<h4 className="text-base font-semibold text-white">
										Confirm Offboarding & Node Revocation
									</h4>
									<p className="text-xs text-rose-300">
										Destructive Action Guard
									</p>
								</div>
							</div>

							<div className="p-4 rounded-xl bg-black/50 border border-white/10 space-y-2 text-xs">
								<p className="text-white/80">
									You are about to disconnect employee node{" "}
									<strong className="text-white font-mono">
										{selectedNodeForRevoke.alias}
									</strong>{" "}
									(
									<code className="text-white/60 font-mono">
										{selectedNodeForRevoke.id}
									</code>
									).
								</p>
								<p className="text-white/60">
									This action will revoke all session keys, purge active
									encryption tokens, and permanently remove peer synchronization
									rights from the network.
								</p>
							</div>

							<div className="space-y-2">
								<label
									htmlFor="disconnect-confirm-input"
									className="block text-xs uppercase tracking-wider text-white/70"
								>
									Type <strong className="text-rose-400">DISCONNECT</strong> to
									confirm:
								</label>
								<input
									id="disconnect-confirm-input"
									type="text"
									value={confirmInput}
									onChange={(e) => setConfirmInput(e.target.value)}
									placeholder="DISCONNECT"
									className="w-full bg-black/60 border border-white/20 rounded-lg px-3 py-2 text-xs text-white font-mono outline-none focus:border-rose-500 transition-colors"
								/>
							</div>

							{errorMessage && (
								<div className="p-3 bg-rose-500/20 border border-rose-500/40 rounded-lg text-xs text-rose-300 flex items-center gap-2">
									<AlertTriangle className="w-4 h-4 shrink-0" />
									{errorMessage}
								</div>
							)}

							<div className="flex justify-end gap-3 pt-2">
								<button
									type="button"
									onClick={handleCloseRevokeModal}
									disabled={isDisconnecting}
									className="px-4 py-2 rounded-lg bg-white/5 hover:bg-white/10 text-white/80 text-xs font-medium transition-all"
								>
									Cancel
								</button>
								<button
									type="button"
									onClick={handleExecuteDisconnect}
									disabled={
										isDisconnecting ||
										confirmInput.trim().toUpperCase() !== "DISCONNECT"
									}
									className="px-4 py-2 rounded-lg bg-rose-600 hover:bg-rose-500 disabled:opacity-40 text-white text-xs font-medium flex items-center gap-2 transition-all"
								>
									{isDisconnecting ? (
										<>
											<RefreshCw className="w-3.5 h-3.5 animate-spin" />
											Offboarding Node...
										</>
									) : (
										<>
											<Trash2 className="w-3.5 h-3.5" />
											Confirm & Revoke Node
										</>
									)}
								</button>
							</div>
						</div>
					</div>
				)}
			</div>
		);
	},
);

export default MeshTopologyGraph;
