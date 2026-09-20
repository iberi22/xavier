import {
	AlertTriangle,
	ArrowLeft,
	Ban,
	CheckCircle2,
	ChevronRight,
	Clock,
	Download,
	Eye,
	Filter,
	KeyRound,
	Lock,
	RefreshCw,
	Search,
	Shield,
	ShieldAlert,
	ShieldCheck,
	UserCheck,
	XCircle,
} from "lucide-react";
import React, { useCallback, useMemo, useState } from "react";

export type AuditEventSeverity = "info" | "warning" | "blocked" | "critical";

export type AuditEventType =
	| "clearance_check"
	| "blocked_attempt"
	| "wallet_signature_validation"
	| "token_revocation"
	| "channel_join"
	| "transmit_authorization";

export interface AuditEvent {
	id: string;
	timestamp: string;
	eventType: AuditEventType;
	severity: AuditEventSeverity;
	participantId: string;
	channelId: string;
	clearanceLevel: "Public" | "Internal" | "Confidential" | "Secret" | "TopSecret";
	action: string;
	outcome: "GRANTED" | "BLOCKED" | "VERIFIED" | "FAILED";
	details: string;
	signaturePreview?: string;
	ipAddress?: string;
	metadata?: Record<string, unknown>;
}

export interface AuditEventItemProps {
	event: AuditEvent;
	onSelectEvent?: (event: AuditEvent) => void;
	isSelected?: boolean;
}

export interface TelecomAuditTrailViewProps {
	token?: string;
	channelId?: string;
	onClose?: () => void;
	initialEvents?: AuditEvent[];
	autoRefresh?: boolean;
}

const MOCK_AUDIT_EVENTS: AuditEvent[] = [
	{
		id: "evt-sec-8801",
		timestamp: new Date(Date.now() - 1000 * 60 * 2).toISOString(),
		eventType: "wallet_signature_validation",
		severity: "info",
		participantId: "node-alice-prod",
		channelId: "legal-case-alpha",
		clearanceLevel: "TopSecret",
		action: "verify_wallet_signature",
		outcome: "VERIFIED",
		details: "Ed25519 wallet signature verified successfully against challenge hash.",
		signaturePreview: "0x8f3a...91bc (64 bytes Ed25519)",
		ipAddress: "10.240.0.12",
		metadata: {
			keyType: "Ed25519",
			challengeNonce: "0x4f128a",
			verificationTimeMs: 1.2,
		},
	},
	{
		id: "evt-sec-8802",
		timestamp: new Date(Date.now() - 1000 * 60 * 8).toISOString(),
		eventType: "blocked_attempt",
		severity: "blocked",
		participantId: "guest-untrusted-99",
		channelId: "legal-case-alpha",
		clearanceLevel: "Confidential",
		action: "authorize_join",
		outcome: "BLOCKED",
		details: "Join attempt rejected: Insufficient clearance level (Public < Confidential required).",
		ipAddress: "192.168.1.105",
		metadata: {
			requiredClearance: "Confidential",
			actualClearance: "Public",
			gateReason: "InsufficientClearance",
		},
	},
	{
		id: "evt-sec-8803",
		timestamp: new Date(Date.now() - 1000 * 60 * 15).toISOString(),
		eventType: "clearance_check",
		severity: "info",
		participantId: "agent-bob-sec",
		channelId: "ops-room-beta",
		clearanceLevel: "Secret",
		action: "authorize_transmit",
		outcome: "GRANTED",
		details: "Participant clearance level Secret validated for encrypted file payload transmission.",
		ipAddress: "10.240.0.18",
		metadata: {
			payloadType: "encrypted_file",
			payloadSize: "14.2 MB",
			rbacRole: "CaseOwner",
		},
	},
	{
		id: "evt-sec-8804",
		timestamp: new Date(Date.now() - 1000 * 60 * 28).toISOString(),
		eventType: "blocked_attempt",
		severity: "critical",
		participantId: "anon-node-77",
		channelId: "ops-room-beta",
		clearanceLevel: "Secret",
		action: "verify_wallet_signature",
		outcome: "FAILED",
		details: "Invalid signature payload: Challenge payload signature verification mismatch.",
		signaturePreview: "0x0011...dead (Corrupted)",
		ipAddress: "172.16.0.44",
		metadata: {
			error: "InvalidSignature('signature mismatch')",
			attempts: 3,
			autoBlockedIp: true,
		},
	},
	{
		id: "evt-sec-8805",
		timestamp: new Date(Date.now() - 1000 * 60 * 45).toISOString(),
		eventType: "token_revocation",
		severity: "warning",
		participantId: "intern-charlie-dev",
		channelId: "general-mesh-01",
		clearanceLevel: "Internal",
		action: "token_expiration_check",
		outcome: "BLOCKED",
		details: "Participant identity token expired at 1700000000 epoch timestamp.",
		ipAddress: "10.240.0.99",
		metadata: {
			expiresAt: 1700000000,
			currentTime: 1700000250,
			gateReason: "TokenInvalid",
		},
	},
];

/** ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted timeline event item into AuditEventItem and wrapped in React.memo()
 * 🎯 Why: Prevents re-rendering the entire audit event timeline when searching or filtering.
 * 📊 Impact: O(1) render updates when toggling selection or updating search text.
 */
export const AuditEventItem = React.memo(function AuditEventItem({
	event,
	onSelectEvent,
	isSelected = false,
}: AuditEventItemProps) {
	const getSeverityBadge = (severity: AuditEventSeverity) => {
		switch (severity) {
			case "blocked":
				return "bg-rose-500/20 text-rose-300 border-rose-500/40";
			case "critical":
				return "bg-red-500/20 text-red-300 border-red-500/40 animate-pulse";
			case "warning":
				return "bg-amber-500/20 text-amber-300 border-amber-500/40";
			default:
				return "bg-emerald-500/20 text-emerald-300 border-emerald-500/40";
		}
	};

	const getOutcomeIcon = () => {
		switch (event.outcome) {
			case "GRANTED":
			case "VERIFIED":
				return <ShieldCheck className="w-4 h-4 text-emerald-400" aria-hidden="true" />;
			case "BLOCKED":
				return <Ban className="w-4 h-4 text-rose-400" aria-hidden="true" />;
			case "FAILED":
				return <ShieldAlert className="w-4 h-4 text-red-400" aria-hidden="true" />;
			default:
				return <CheckCircle2 className="w-4 h-4 text-cyan-400" aria-hidden="true" />;
		}
	};

	const getClearanceBadgeColor = () => {
		switch (event.clearanceLevel) {
			case "TopSecret":
				return "bg-purple-500/20 text-purple-300 border-purple-500/40";
			case "Secret":
				return "bg-amber-500/20 text-amber-300 border-amber-500/40";
			case "Confidential":
				return "bg-blue-500/20 text-blue-300 border-blue-500/40";
			case "Internal":
				return "bg-cyan-500/20 text-cyan-300 border-cyan-500/40";
			default:
				return "bg-slate-500/20 text-slate-300 border-slate-500/40";
		}
	};

	const formattedTime = useMemo(() => {
		try {
			return new Date(event.timestamp).toLocaleTimeString([], {
				hour: "2-digit",
				minute: "2-digit",
				second: "2-digit",
			});
		} catch {
			return event.timestamp;
		}
	}, [event.timestamp]);

	return (
		<div
			className={`group relative pl-6 pb-6 transition-colors ${
				isSelected ? "opacity-100" : "opacity-90 hover:opacity-100"
			}`}
		>
			{/* Timeline vertical connector line */}
			<div
				className="absolute left-[11px] top-6 bottom-0 w-px bg-white/10 group-last:hidden"
				aria-hidden="true"
			/>

			{/* Timeline dot icon container */}
			<div
				className={`absolute left-0 top-1 w-6 h-6 rounded-full border flex items-center justify-center bg-slate-950 ${
					event.outcome === "BLOCKED" || event.outcome === "FAILED"
						? "border-rose-500/50 text-rose-400"
						: "border-emerald-500/50 text-emerald-400"
				}`}
			>
				{getOutcomeIcon()}
			</div>

			{/* Main event card */}
			<div
				className={`p-4 rounded-xl border transition-all ${
					isSelected
						? "bg-white/[0.08] border-[#39ff14]/50 shadow-[0_0_15px_rgba(57,255,20,0.1)]"
						: "bg-white/[0.02] border-white/10 hover:bg-white/[0.04] hover:border-white/20"
				}`}
			>
				<div className="flex flex-wrap items-center justify-between gap-2 mb-2">
					<div className="flex items-center gap-2">
						<span
							className={`text-[10px] font-mono uppercase px-2 py-0.5 rounded-full border ${getSeverityBadge(
								event.severity,
							)}`}
						>
							{event.outcome}
						</span>
						<span
							className={`text-[10px] font-mono uppercase px-2 py-0.5 rounded-full border ${getClearanceBadgeColor()}`}
						>
							{event.clearanceLevel}
						</span>
						<code className="text-xs font-mono text-[#39ff14] font-medium">
							{event.action}
						</code>
					</div>

					<div className="flex items-center gap-1.5 text-xs text-white/40 font-mono">
						<Clock className="w-3.5 h-3.5" aria-hidden="true" />
						<span>{formattedTime}</span>
					</div>
				</div>

				<p className="text-xs text-white/80 leading-relaxed font-sans mb-3">
					{event.details}
				</p>

				<div className="flex flex-wrap items-center justify-between gap-2 pt-2 border-t border-white/5 text-[11px] font-mono text-white/50">
					<div className="flex items-center gap-3">
						<span>Participant: <code className="text-white/80">{event.participantId}</code></span>
						<span>Channel: <code className="text-cyan-400">{event.channelId}</code></span>
					</div>

					{onSelectEvent && (
						<button
							type="button"
							onClick={() => onSelectEvent(event)}
							aria-label={`View security audit event details for ${event.id}`}
							className="flex items-center gap-1 text-[#39ff14] hover:underline focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] rounded px-1"
						>
							<span>Inspect Details</span>
							<ChevronRight className="w-3.5 h-3.5" aria-hidden="true" />
						</button>
					)}
				</div>
			</div>
		</div>
	);
});

export const TelecomAuditTrailView: React.FC<TelecomAuditTrailViewProps> = ({
	token = "",
	channelId = "",
	onClose,
	initialEvents = MOCK_AUDIT_EVENTS,
	autoRefresh: initialAutoRefresh = false,
}) => {
	const [events, setEvents] = useState<AuditEvent[]>(initialEvents);
	const [searchQuery, setSearchQuery] = useState("");
	const [filterCategory, setFilterCategory] = useState<
		"all" | "clearance" | "signature" | "blocked"
	>("all");
	const [selectedEvent, setSelectedEvent] = useState<AuditEvent | null>(null);
	const [isAutoRefresh, setIsAutoRefresh] = useState(initialAutoRefresh);
	const [isRefreshing, setIsRefreshing] = useState(false);

	const handleRefresh = useCallback(() => {
		setIsRefreshing(true);
		setTimeout(() => {
			setIsRefreshing(false);
		}, 600);
	}, []);

	const filteredEvents = useMemo(() => {
		return events.filter((evt) => {
			if (channelId && evt.channelId !== channelId) {
				return false;
			}

			// Category filter
			if (filterCategory === "clearance" && evt.eventType !== "clearance_check") {
				return false;
			}
			if (
				filterCategory === "signature" &&
				evt.eventType !== "wallet_signature_validation"
			) {
				return false;
			}
			if (filterCategory === "blocked" && evt.outcome !== "BLOCKED" && evt.outcome !== "FAILED") {
				return false;
			}

			// Search query match
			if (searchQuery.trim()) {
				const query = searchQuery.toLowerCase();
				const matchParticipant = evt.participantId.toLowerCase().includes(query);
				const matchChannel = evt.channelId.toLowerCase().includes(query);
				const matchAction = evt.action.toLowerCase().includes(query);
				const matchDetails = evt.details.toLowerCase().includes(query);
				const matchId = evt.id.toLowerCase().includes(query);
				return matchParticipant || matchChannel || matchAction || matchDetails || matchId;
			}

			return true;
		});
	}, [events, channelId, filterCategory, searchQuery]);

	const stats = useMemo(() => {
		const total = events.length;
		const blocked = events.filter((e) => e.outcome === "BLOCKED" || e.outcome === "FAILED").length;
		const signatures = events.filter((e) => e.eventType === "wallet_signature_validation").length;
		const clearances = events.filter((e) => e.eventType === "clearance_check").length;
		return { total, blocked, signatures, clearances };
	}, [events]);

	const handleExportLogs = useCallback(() => {
		const dataStr = "data:text/json;charset=utf-8," + encodeURIComponent(JSON.stringify(events, null, 2));
		const downloadAnchor = document.createElement("a");
		downloadAnchor.setAttribute("href", dataStr);
		downloadAnchor.setAttribute("download", `telecom_security_audit_${Date.now()}.json`);
		document.body.appendChild(downloadAnchor);
		downloadAnchor.click();
		downloadAnchor.remove();
	}, [events]);

	return (
		<div className="relative w-full h-full min-h-[600px] font-sans bg-slate-950 text-white flex flex-col rounded-2xl border border-white/10 overflow-hidden">
			{/* Top Bar Header */}
			<header className="p-4 bg-slate-900/90 border-b border-white/10 flex flex-wrap items-center justify-between gap-3">
				<div className="flex items-center gap-3">
					{onClose && (
						<button
							type="button"
							onClick={onClose}
							aria-label="Back to previous view"
							className="p-1.5 rounded-lg bg-white/5 hover:bg-white/10 text-white/70 hover:text-white transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
						>
							<ArrowLeft className="w-4 h-4" aria-hidden="true" />
						</button>
					)}
					<div className="flex items-center gap-2">
						<div className="w-8 h-8 rounded-lg bg-[#39ff14]/10 border border-[#39ff14]/30 flex items-center justify-center">
							<Shield className="w-4 h-4 text-[#39ff14]" aria-hidden="true" />
						</div>
						<div>
							<h1 className="text-sm font-semibold tracking-wide text-white">
								Telecom Clearance & Security Audit Trail
							</h1>
							<p className="text-[10px] text-white/50 font-mono">
								Real-time Security Verification & Wallet Signature Log
							</p>
						</div>
					</div>
				</div>

				<div className="flex items-center gap-2">
					<button
						type="button"
						onClick={handleRefresh}
						aria-label="Refresh audit timeline"
						className="p-2 rounded-xl bg-white/5 border border-white/10 hover:bg-white/10 text-white/80 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
					>
						<RefreshCw
							className={`w-4 h-4 ${isRefreshing ? "animate-spin text-[#39ff14]" : ""}`}
							aria-hidden="true"
						/>
					</button>

					<button
						type="button"
						onClick={handleExportLogs}
						aria-label="Export security audit log JSON"
						className="flex items-center gap-1.5 px-3 py-1.5 bg-white/5 border border-white/10 rounded-xl text-xs font-medium text-white/80 hover:bg-white/10 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]"
					>
						<Download className="w-3.5 h-3.5" aria-hidden="true" />
						<span>Export Log</span>
					</button>

					<div className="flex items-center gap-1.5 px-3 py-1 rounded-full bg-emerald-500/10 border border-emerald-500/20 text-emerald-400 text-[11px] font-mono">
						<span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse" />
						<span>Clearance Gate Active</span>
					</div>
				</div>
			</header>

			{/* Summary Metrics Bar */}
			<section
				className="grid grid-cols-2 sm:grid-cols-4 gap-3 p-4 bg-black/40 border-b border-white/10"
				aria-label="Security Metrics Summary"
			>
				<div className="p-3 rounded-xl bg-white/[0.02] border border-white/10">
					<span className="text-[10px] uppercase font-mono text-white/40 block">Total Events</span>
					<span className="text-lg font-mono font-bold text-white">{stats.total}</span>
				</div>
				<div className="p-3 rounded-xl bg-white/[0.02] border border-white/10">
					<span className="text-[10px] uppercase font-mono text-white/40 block">Clearance Checks</span>
					<span className="text-lg font-mono font-bold text-emerald-400">{stats.clearances}</span>
				</div>
				<div className="p-3 rounded-xl bg-white/[0.02] border border-white/10">
					<span className="text-[10px] uppercase font-mono text-white/40 block">Signature Validations</span>
					<span className="text-lg font-mono font-bold text-cyan-400">{stats.signatures}</span>
				</div>
				<div className="p-3 rounded-xl bg-white/[0.02] border border-white/10">
					<span className="text-[10px] uppercase font-mono text-white/40 block">Blocked Attempts</span>
					<span className="text-lg font-mono font-bold text-rose-400">{stats.blocked}</span>
				</div>
			</section>

			{/* Controls Toolbar: Search & Category Filter Pills */}
			<div className="p-4 border-b border-white/10 bg-slate-900/40 flex flex-col md:flex-row gap-3 items-stretch md:items-center justify-between">
				{/* Search Input */}
				<div className="relative flex-1 max-w-md">
					<Search
						className="w-4 h-4 absolute left-3 top-2.5 text-white/40"
						aria-hidden="true"
					/>
					<input
						type="text"
						value={searchQuery}
						onChange={(e) => setSearchQuery(e.target.value)}
						placeholder="Filter by participant, channel, action, or details..."
						className="w-full pl-9 pr-3 py-1.5 bg-white/5 border border-white/10 rounded-xl text-xs text-white placeholder-white/40 focus:outline-none focus:border-[#39ff14]/50 font-mono"
					/>
				</div>

				{/* Category Filter Pills */}
				<div
					className="flex flex-wrap items-center gap-1.5 bg-black/40 p-1 rounded-xl border border-white/10"
					aria-label="Filter events by category"
				>
					<button
						type="button"
						onClick={() => setFilterCategory("all")}
						className={`px-3 py-1 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
							filterCategory === "all"
								? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
								: "text-white/60 hover:text-white"
						}`}
					>
						All ({events.length})
					</button>

					<button
						type="button"
						onClick={() => setFilterCategory("clearance")}
						className={`px-3 py-1 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
							filterCategory === "clearance"
								? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
								: "text-white/60 hover:text-white"
						}`}
					>
						Clearance Checks
					</button>

					<button
						type="button"
						onClick={() => setFilterCategory("signature")}
						className={`px-3 py-1 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
							filterCategory === "signature"
								? "bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/40"
								: "text-white/60 hover:text-white"
						}`}
					>
						Wallet Signatures
					</button>

					<button
						type="button"
						onClick={() => setFilterCategory("blocked")}
						className={`px-3 py-1 rounded-lg text-xs font-medium transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] ${
							filterCategory === "blocked"
								? "bg-rose-500/20 text-rose-300 border border-rose-500/40"
								: "text-white/60 hover:text-white"
						}`}
					>
						Blocked ({stats.blocked})
					</button>
				</div>
			</div>

			{/* Main Content View: Timeline & Selected Event Inspector */}
			<div className="flex-1 overflow-hidden grid grid-cols-1 lg:grid-cols-3 divide-y lg:divide-y-0 lg:divide-x divide-white/10">
				{/* Timeline Column */}
				<div
					className={`p-4 overflow-y-auto ${
						selectedEvent ? "lg:col-span-2" : "lg:col-span-3"
					}`}
					role="log"
					aria-live="polite"
					aria-label="Security verification timeline log"
				>
					{filteredEvents.length === 0 ? (
						<div className="p-8 text-center text-white/40 space-y-2">
							<ShieldAlert className="w-8 h-8 mx-auto text-white/20" aria-hidden="true" />
							<p className="text-xs">No matching security audit events found.</p>
						</div>
					) : (
						filteredEvents.map((evt) => (
							<AuditEventItem
								key={evt.id}
								event={evt}
								onSelectEvent={setSelectedEvent}
								isSelected={selectedEvent?.id === evt.id}
							/>
						))
					)}
				</div>

				{/* Event Inspector Drawer */}
				{selectedEvent && (
					<aside
						className="p-4 overflow-y-auto bg-black/40 space-y-4"
						aria-label="Selected Event Inspector"
					>
						<div className="flex items-center justify-between border-b border-white/10 pb-3">
							<div className="flex items-center gap-2">
								<KeyRound className="w-4 h-4 text-[#39ff14]" aria-hidden="true" />
								<h2 className="text-xs font-semibold font-mono text-white">
									Event Inspector ({selectedEvent.id})
								</h2>
							</div>
							<button
								type="button"
								onClick={() => setSelectedEvent(null)}
								aria-label="Close event inspector"
								className="text-xs text-white/40 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14] rounded px-1"
							>
								Close ✕
							</button>
						</div>

						<div className="space-y-3 text-xs font-mono">
							<div>
								<span className="text-white/40 block text-[10px] uppercase">Timestamp</span>
								<span className="text-white/90">{selectedEvent.timestamp}</span>
							</div>

							<div>
								<span className="text-white/40 block text-[10px] uppercase">Action</span>
								<span className="text-[#39ff14] font-bold">{selectedEvent.action}</span>
							</div>

							<div>
								<span className="text-white/40 block text-[10px] uppercase">Participant Identity</span>
								<span className="text-cyan-300">{selectedEvent.participantId}</span>
							</div>

							<div>
								<span className="text-white/40 block text-[10px] uppercase">Channel Target</span>
								<span className="text-cyan-300">{selectedEvent.channelId}</span>
							</div>

							<div>
								<span className="text-white/40 block text-[10px] uppercase">Clearance Level</span>
								<span className="text-purple-300">{selectedEvent.clearanceLevel}</span>
							</div>

							{selectedEvent.signaturePreview && (
								<div>
									<span className="text-white/40 block text-[10px] uppercase">
										Ed25519 Wallet Signature
									</span>
									<code className="block p-2 rounded bg-white/5 border border-white/10 text-[10px] text-emerald-300 break-all">
										{selectedEvent.signaturePreview}
									</code>
								</div>
							)}

							<div>
								<span className="text-white/40 block text-[10px] uppercase">Event Details</span>
								<p className="p-2.5 rounded bg-white/[0.02] border border-white/10 text-white/80 font-sans text-xs leading-relaxed">
									{selectedEvent.details}
								</p>
							</div>

							{selectedEvent.metadata && (
								<div>
									<span className="text-white/40 block text-[10px] uppercase mb-1">
										Metadata Context
									</span>
									<pre className="p-2.5 rounded bg-black/60 border border-white/10 text-[10px] text-white/70 overflow-x-auto">
										{JSON.stringify(selectedEvent.metadata, null, 2)}
									</pre>
								</div>
							)}
						</div>
					</aside>
				)}
			</div>
		</div>
	);
};

export default TelecomAuditTrailView;
