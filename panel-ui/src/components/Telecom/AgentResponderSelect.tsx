import {
	Bot,
	Check,
	ChevronDown,
	Cpu,
	Power,
	Scale,
	Shield,
	Sparkles,
	Zap,
} from "lucide-react";
import React, {
	useCallback,
	useEffect,
	useId,
	useMemo,
	useRef,
	useState,
} from "react";

export type DispatchPolicyType =
	| "AlwaysRespond"
	| "MentionOnly"
	| "KeywordMatch"
	| "PrefixMatch";

export interface AgentResponderOption {
	id: string;
	name: string;
	role: string;
	model: string;
	description: string;
	capabilities: string[];
	avatarColor?: string;
	dispatchPolicy?: DispatchPolicyType;
	keywords?: string[];
	prefix?: string;
	online?: boolean;
}

export interface AgentResponderSelectProps {
	/** List of available cognitive responder agents */
	agents?: AgentResponderOption[];
	/** ID of the currently selected agent */
	selectedAgentId?: string;
	/** Callback fired when selected agent changes */
	onSelectAgent?: (agent: AgentResponderOption) => void;
	/** Whether auto-responder feature is toggled on */
	enabled?: boolean;
	/** Callback fired when auto-responder toggle is changed */
	onToggleEnabled?: (enabled: boolean) => void;
	/** Active dispatch policy */
	dispatchPolicy?: DispatchPolicyType;
	/** Callback fired when dispatch policy changes */
	onChangeDispatchPolicy?: (policy: DispatchPolicyType) => void;
	/** Optional custom class name */
	className?: string;
	/** Label for the component section */
	label?: string;
	/** Disabled state */
	disabled?: boolean;
}

export const DEFAULT_LOCAL_AGENTS: AgentResponderOption[] = [
	{
		id: "legal-ai",
		name: "Legal & Compliance Sentinel",
		role: "Legal AI Advisor",
		model: "claude-3-5-sonnet",
		description:
			"Evaluates incoming peer agreements, compliance constraints, and legal disclosures.",
		capabilities: ["Contracts", "GDPR", "Audit Trails", "IP Checks"],
		avatarColor: "text-emerald-400 bg-emerald-500/10 border-emerald-500/30",
		dispatchPolicy: "MentionOnly",
		online: true,
	},
	{
		id: "sentinel-ai",
		name: "Sentinel Threat Monitor",
		role: "Security & Access Monitor",
		model: "deepseek-r1-70b",
		description:
			"Inspects incoming encrypted node queries for zero-day payloads and anomaly signals.",
		capabilities: [
			"Threat Detection",
			"Access Validation",
			"Telemetry Scrubbing",
		],
		avatarColor: "text-cyan-400 bg-cyan-500/10 border-cyan-500/30",
		dispatchPolicy: "AlwaysRespond",
		online: true,
	},
	{
		id: "research-ai",
		name: "Research & Synthesis Copilot",
		role: "Research Specialist",
		model: "llama-3-70b",
		description:
			"Queries local vector memory spaces to answer deep technical and academic inquiries.",
		capabilities: [
			"Vector Search",
			"Paper Summaries",
			"Citations",
			"QMD Reader",
		],
		avatarColor: "text-purple-400 bg-purple-500/10 border-purple-500/30",
		dispatchPolicy: "KeywordMatch",
		keywords: ["research", "paper", "query", "vector"],
		online: true,
	},
	{
		id: "ops-ai",
		name: "Ops & Infrastructure Agent",
		role: "Node Operations Lead",
		model: "mistral-large",
		description:
			"Responds to node health, routing stats, and mesh cluster status requests.",
		capabilities: ["Peer Health", "Routing Tables", "Bandwidth Monitoring"],
		avatarColor: "text-amber-400 bg-amber-500/10 border-amber-500/30",
		dispatchPolicy: "PrefixMatch",
		prefix: "!ops",
		online: false,
	},
];

const DISPATCH_POLICIES: {
	type: DispatchPolicyType;
	label: string;
	description: string;
}[] = [
	{
		type: "AlwaysRespond",
		label: "Always Respond",
		description: "Answers all incoming peer messages automatically",
	},
	{
		type: "MentionOnly",
		label: "@Mention Only",
		description: "Responds only when explicitly tagged (e.g. @agent)",
	},
	{
		type: "KeywordMatch",
		label: "Keyword Match",
		description: "Triggers when specific domain keywords are present",
	},
	{
		type: "PrefixMatch",
		label: "Prefix Match",
		description: "Triggers on message command prefix (e.g. !agent)",
	},
];

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted inline AgentOptionCard component wrapped in React.memo()
 * 🎯 Why: Prevents O(N) re-renders of list items when dropdown state or active selection toggles.
 * 📊 Impact: O(1) rendering for option list interactions.
 */
const AgentOptionCard = React.memo(function AgentOptionCard({
	agent,
	isSelected,
	onSelect,
}: {
	agent: AgentResponderOption;
	isSelected: boolean;
	onSelect: (agent: AgentResponderOption) => void;
}) {
	const handleClick = useCallback(() => {
		onSelect(agent);
	}, [agent, onSelect]);

	const avatarClasses =
		agent.avatarColor || "text-[#39ff14] bg-[#39ff14]/10 border-[#39ff14]/30";

	return (
		<div
			role="option"
			aria-selected={isSelected}
			onClick={handleClick}
			onKeyDown={(e) => {
				if (e.key === "Enter" || e.key === " ") {
					e.preventDefault();
					onSelect(agent);
				}
			}}
			tabIndex={0}
			className={`group relative p-3.5 rounded-xl cursor-pointer transition-all border outline-none ${
				isSelected
					? "bg-[#39ff14]/10 border-[#39ff14]/40 text-white shadow-lg shadow-[#39ff14]/5"
					: "bg-white/[0.02] hover:bg-white/5 border-white/5 hover:border-white/15 text-white/80"
			}`}
		>
			<div className="flex items-start justify-between gap-3">
				<div className="flex items-start gap-3">
					<div
						className={`p-2 rounded-lg border flex items-center justify-center shrink-0 ${avatarClasses}`}
					>
						{agent.id.includes("legal") ? (
							<Scale size={18} />
						) : agent.id.includes("sentinel") ? (
							<Shield size={18} />
						) : agent.id.includes("research") ? (
							<Sparkles size={18} />
						) : (
							<Bot size={18} />
						)}
					</div>
					<div className="space-y-1">
						<div className="flex items-center gap-2">
							<span className="font-semibold text-sm text-white tracking-tight">
								{agent.name}
							</span>
							<span className="px-2 py-0.5 rounded-md text-[10px] font-mono uppercase bg-white/10 text-white/60 border border-white/10">
								{agent.model}
							</span>
						</div>
						<p className="text-xs text-white/50 leading-relaxed line-clamp-2">
							{agent.description}
						</p>
						<div className="flex flex-wrap gap-1.5 pt-1">
							{agent.capabilities.map((cap) => (
								<span
									key={cap}
									className="px-1.5 py-0.5 rounded text-[9px] font-mono bg-white/5 text-white/60 border border-white/5"
								>
									{cap}
								</span>
							))}
						</div>
					</div>
				</div>

				<div className="flex items-center gap-2 shrink-0 pt-0.5">
					{agent.online !== undefined && (
						<span
							className={`w-2 h-2 rounded-full ${
								agent.online
									? "bg-[#39ff14] shadow-[0_0_8px_#39ff14]"
									: "bg-white/20"
							}`}
							title={agent.online ? "Agent Active" : "Agent Standby"}
						/>
					)}
					{isSelected && (
						<div className="p-1 rounded-full bg-[#39ff14]/20 text-[#39ff14]">
							<Check size={14} />
						</div>
					)}
				</div>
			</div>
		</div>
	);
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: AgentResponderSelect component with memoized callbacks, accessibility attributes, and keyboard controls
 * 🎯 Why: Provides a high-performance local AI agent selector for automated node cognitive responders.
 * 📊 Impact: Zero wasted renders on parent component updates, fully keyboard accessible.
 */
export const AgentResponderSelect = React.memo(function AgentResponderSelect({
	agents = DEFAULT_LOCAL_AGENTS,
	selectedAgentId,
	onSelectAgent,
	enabled: initialEnabled = true,
	onToggleEnabled,
	dispatchPolicy: initialPolicy,
	onChangeDispatchPolicy,
	className = "",
	label = "Automated Cognitive Responder",
	disabled = false,
}: AgentResponderSelectProps) {
	const listboxId = useId();
	const dropdownRef = useRef<HTMLDivElement>(null);

	// Internal state for uncontrolled usage fallbacks
	const [isAutoEnabled, setIsAutoEnabled] = useState<boolean>(initialEnabled);
	const [isOpen, setIsOpen] = useState<boolean>(false);
	const [internalSelectedId, setInternalSelectedId] = useState<string>(
		selectedAgentId || agents[0]?.id || "legal-ai",
	);

	// Sync with controlled prop if provided
	const activeSelectedId =
		selectedAgentId !== undefined ? selectedAgentId : internalSelectedId;
	const activeEnabled =
		initialEnabled !== undefined ? initialEnabled : isAutoEnabled;

	const selectedAgent = useMemo(() => {
		return (
			agents.find((a) => a.id === activeSelectedId) ||
			agents[0] ||
			DEFAULT_LOCAL_AGENTS[0]
		);
	}, [agents, activeSelectedId]);

	const [activePolicy, setActivePolicy] = useState<DispatchPolicyType>(
		initialPolicy || selectedAgent?.dispatchPolicy || "AlwaysRespond",
	);

	useEffect(() => {
		if (initialEnabled !== undefined) {
			setIsAutoEnabled(initialEnabled);
		}
	}, [initialEnabled]);

	useEffect(() => {
		if (selectedAgentId !== undefined) {
			setInternalSelectedId(selectedAgentId);
		}
	}, [selectedAgentId]);

	useEffect(() => {
		if (initialPolicy !== undefined) {
			setActivePolicy(initialPolicy);
		} else if (selectedAgent?.dispatchPolicy) {
			setActivePolicy(selectedAgent.dispatchPolicy);
		}
	}, [initialPolicy, selectedAgent]);

	// Handle clicks outside the dropdown to close listbox
	useEffect(() => {
		function handleClickOutside(event: MouseEvent) {
			if (
				dropdownRef.current &&
				!dropdownRef.current.contains(event.target as Node)
			) {
				setIsOpen(false);
			}
		}

		if (isOpen) {
			document.addEventListener("mousedown", handleClickOutside);
		}
		return () => {
			document.removeEventListener("mousedown", handleClickOutside);
		};
	}, [isOpen]);

	const handleToggleMasterSwitch = useCallback(() => {
		if (disabled) return;
		const nextEnabled = !activeEnabled;
		setIsAutoEnabled(nextEnabled);
		onToggleEnabled?.(nextEnabled);
	}, [activeEnabled, disabled, onToggleEnabled]);

	const handleSelectAgent = useCallback(
		(agent: AgentResponderOption) => {
			if (disabled) return;
			setInternalSelectedId(agent.id);
			setIsOpen(false);
			onSelectAgent?.(agent);
			if (agent.dispatchPolicy && !initialPolicy) {
				setActivePolicy(agent.dispatchPolicy);
				onChangeDispatchPolicy?.(agent.dispatchPolicy);
			}
		},
		[disabled, initialPolicy, onChangeDispatchPolicy, onSelectAgent],
	);

	const handlePolicyChange = useCallback(
		(policy: DispatchPolicyType) => {
			if (disabled) return;
			setActivePolicy(policy);
			onChangeDispatchPolicy?.(policy);
		},
		[disabled, onChangeDispatchPolicy],
	);

	const handleKeyDown = useCallback(
		(e: React.KeyboardEvent) => {
			if (disabled) return;
			if (e.key === "Escape") {
				setIsOpen(false);
			} else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
				if (!isOpen) {
					setIsOpen(true);
					e.preventDefault();
				}
			}
		},
		[disabled, isOpen],
	);

	return (
		<div
			className={`bg-white/[0.03] border border-white/10 rounded-2xl p-5 space-y-5 transition-all ${
				disabled ? "opacity-50 pointer-events-none" : ""
			} ${className}`}
		>
			{/* Header with Master Switch Toggle */}
			<div className="flex items-center justify-between border-b border-white/5 pb-4">
				<div className="flex items-center gap-3">
					<div className="p-2.5 rounded-xl bg-[#39ff14]/10 text-[#39ff14] border border-[#39ff14]/20">
						<Cpu size={20} />
					</div>
					<div>
						<h3 className="text-base font-semibold text-white tracking-tight flex items-center gap-2">
							{label}
							<span
								className={`px-2 py-0.5 text-[10px] font-mono rounded-full uppercase border ${
									activeEnabled
										? "bg-[#39ff14]/10 text-[#39ff14] border-[#39ff14]/30"
										: "bg-white/5 text-white/40 border-white/10"
								}`}
							>
								{activeEnabled ? "Active" : "Disabled"}
							</span>
						</h3>
						<p className="text-xs text-white/40 mt-0.5">
							Local AI agent that automatically processes incoming peer
							telemetry & queries
						</p>
					</div>
				</div>

				<button
					type="button"
					role="switch"
					aria-checked={activeEnabled}
					aria-label="Toggle automated bot response"
					onClick={handleToggleMasterSwitch}
					disabled={disabled}
					className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-[#39ff14]/50 focus:ring-offset-2 focus:ring-offset-black ${
						activeEnabled ? "bg-[#39ff14]" : "bg-white/10"
					}`}
				>
					<span
						className={`pointer-events-none inline-block h-5 w-5 transform rounded-full bg-black shadow-lg ring-0 transition duration-200 ease-in-out flex items-center justify-center ${
							activeEnabled
								? "translate-x-5 text-[#39ff14]"
								: "translate-x-0 text-white/40"
						}`}
					>
						<Power size={11} className="stroke-[3]" />
					</span>
				</button>
			</div>

			{/* Selected Agent Dropdown Trigger */}
			<div className="space-y-2" ref={dropdownRef}>
				<span className="block text-xs font-medium text-white/60 tracking-wide uppercase">
					Select Active Cognitive Responder
				</span>

				<div className="relative">
					<button
						type="button"
						aria-haspopup="listbox"
						aria-expanded={isOpen}
						aria-controls={listboxId}
						aria-label="Select cognitive agent responder"
						disabled={disabled || !activeEnabled}
						onClick={() => setIsOpen((prev) => !prev)}
						onKeyDown={handleKeyDown}
						className={`w-full flex items-center justify-between p-3.5 rounded-xl border text-left transition-all ${
							!activeEnabled
								? "bg-white/[0.01] border-white/5 text-white/30 cursor-not-allowed"
								: isOpen
									? "bg-white/10 border-[#39ff14]/50 text-white shadow-lg shadow-[#39ff14]/5 ring-1 ring-[#39ff14]/30"
									: "bg-white/5 hover:bg-white/[0.08] border-white/10 text-white"
						}`}
					>
						{selectedAgent ? (
							<div className="flex items-center gap-3 min-w-0">
								<div
									className={`p-2 rounded-lg border shrink-0 ${
										selectedAgent.avatarColor ||
										"text-[#39ff14] bg-[#39ff14]/10 border-[#39ff14]/30"
									}`}
								>
									{selectedAgent.id.includes("legal") ? (
										<Scale size={18} />
									) : selectedAgent.id.includes("sentinel") ? (
										<Shield size={18} />
									) : selectedAgent.id.includes("research") ? (
										<Sparkles size={18} />
									) : (
										<Bot size={18} />
									)}
								</div>
								<div className="min-w-0">
									<div className="flex items-center gap-2">
										<span className="font-semibold text-sm text-white truncate">
											{selectedAgent.name}
										</span>
										<span className="px-1.5 py-0.5 rounded text-[10px] font-mono bg-white/10 text-white/70 border border-white/10">
											{selectedAgent.model}
										</span>
									</div>
									<p className="text-xs text-white/40 truncate mt-0.5">
										{selectedAgent.role} • {selectedAgent.capabilities.length}{" "}
										capabilities
									</p>
								</div>
							</div>
						) : (
							<span className="text-sm text-white/40">Select an agent...</span>
						)}

						<ChevronDown
							size={18}
							className={`text-white/50 shrink-0 transition-transform duration-200 ${
								isOpen ? "rotate-180 text-[#39ff14]" : ""
							}`}
						/>
					</button>

					{/* Dropdown Options Floating Menu */}
					{isOpen && activeEnabled && (
						<div className="absolute z-50 left-0 right-0 mt-2 p-2 bg-[#0d1117] border border-white/15 rounded-2xl shadow-2xl backdrop-blur-xl animate-in fade-in slide-in-from-top-2 duration-150">
							<div className="px-3 py-2 border-b border-white/5 flex items-center justify-between mb-2">
								<span className="text-[11px] font-semibold uppercase tracking-wider text-white/40">
									Available AI Responders ({agents.length})
								</span>
								<span className="text-[10px] font-mono text-[#39ff14]">
									Local P2P Isolation
								</span>
							</div>

							<div
								id={listboxId}
								role="listbox"
								aria-label="Available cognitive agents"
								className="max-h-72 overflow-y-auto space-y-1.5 pr-1 scrollbar-thin scrollbar-thumb-white/10"
							>
								{agents.map((agent) => (
									<AgentOptionCard
										key={agent.id}
										agent={agent}
										isSelected={agent.id === activeSelectedId}
										onSelect={handleSelectAgent}
									/>
								))}
							</div>
						</div>
					)}
				</div>
			</div>

			{/* Selected Agent Capabilities Summary */}
			{selectedAgent && activeEnabled && (
				<div className="p-3.5 bg-white/[0.02] border border-white/5 rounded-xl space-y-2">
					<div className="flex items-center justify-between text-xs text-white/60">
						<span className="font-semibold uppercase tracking-wider text-[10px] text-white/40 flex items-center gap-1.5">
							<Zap size={12} className="text-[#39ff14]" /> Active Capabilities
						</span>
						<span className="font-mono text-[10px] text-white/40">
							Model: {selectedAgent.model}
						</span>
					</div>
					<div className="flex flex-wrap gap-1.5">
						{selectedAgent.capabilities.map((cap) => (
							<span
								key={cap}
								className="px-2 py-0.5 rounded-md text-xs bg-[#39ff14]/5 text-[#39ff14] border border-[#39ff14]/20 font-mono"
							>
								{cap}
							</span>
						))}
					</div>
				</div>
			)}

			{/* Dispatch Policy Selection */}
			{activeEnabled && (
				<div className="space-y-2.5 pt-2 border-t border-white/5">
					<span className="block text-xs font-medium text-white/60 tracking-wide uppercase">
						Response Dispatch Policy
					</span>
					<div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
						{DISPATCH_POLICIES.map((pol) => {
							const isSelectedPolicy = activePolicy === pol.type;
							return (
								<button
									key={pol.type}
									type="button"
									disabled={disabled}
									onClick={() => handlePolicyChange(pol.type)}
									className={`p-3 rounded-xl border text-left transition-all ${
										isSelectedPolicy
											? "bg-[#39ff14]/10 border-[#39ff14]/40 text-white shadow-md shadow-[#39ff14]/5"
											: "bg-white/5 hover:bg-white/10 border-white/10 text-white/70"
									}`}
								>
									<div className="flex items-center justify-between mb-1">
										<span className="text-xs font-bold text-white tracking-tight">
											{pol.label}
										</span>
										{isSelectedPolicy && (
											<div className="w-2 h-2 rounded-full bg-[#39ff14]" />
										)}
									</div>
									<p className="text-[11px] text-white/40 leading-snug">
										{pol.description}
									</p>
								</button>
							);
						})}
					</div>
				</div>
			)}
		</div>
	);
});

export default AgentResponderSelect;
