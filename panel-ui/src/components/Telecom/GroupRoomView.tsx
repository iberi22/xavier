import {
	ArrowLeft,
	Edit3,
	Flame,
	Hash,
	Info,
	KeyRound,
	Lock,
	RefreshCw,
	Search,
	Send,
	Shield,
	ShieldAlert,
	ShieldCheck,
	UserMinus,
	Users,
	X,
} from "lucide-react";
import React, { useCallback, useEffect, useMemo, useState } from "react";

/**
 * Clearance levels for Telecom group rooms.
 */
export type ClearanceLevel =
	| "Unclassified"
	| "Confidential"
	| "Secret"
	| "TopSecret";

/**
 * Roles for group room participants.
 */
export type MemberRole = "Admin" | "Participant" | "Observer";

/**
 * Represents a participant in a Telecom Group Room.
 */
export interface GroupMember {
	nodeId: string;
	alias: string;
	walletId?: string;
	role: MemberRole;
	joinedAt?: string;
	avatarUrl?: string;
	isOnline?: boolean;
	clearanceLevel?: ClearanceLevel;
}

/**
 * Represents a Telecom Group Room configuration.
 */
export interface GroupRoom {
	roomId: string;
	name: string;
	topic?: string;
	description?: string;
	minClearance: ClearanceLevel;
	epochId?: number;
	members: GroupMember[];
	createdAt?: string;
}

/**
 * Represents a chat message within a group room.
 */
export interface ChatMessage {
	id: string;
	senderId: string;
	senderAlias: string;
	content: string;
	timestamp: string;
	clearanceLevel?: ClearanceLevel;
	ephemeral?: boolean;
}

/**
 * Props for the GroupRoomView component.
 */
export interface GroupRoomViewProps {
	room?: GroupRoom;
	currentUserId?: string;
	messages?: ChatMessage[];
	onSendMessage?: (content: string, isEphemeral?: boolean) => void;
	onUpdateTopic?: (newTopic: string) => void;
	onEjectMember?: (nodeId: string) => void;
	onRotateEpoch?: () => void;
	onClose?: () => void;
	className?: string;
}

// Default fallback mock room data
const DEFAULT_ROOM: GroupRoom = {
	roomId: "grp-sec-alpha",
	name: "Strategic-Ops-Council",
	topic: "Multi-Node Mesh Consensus & Zero-Trust Protocol Synchronization v2.4",
	description:
		"Secure classified channel for node-to-node telemetry and group consensus coordination.",
	minClearance: "TopSecret",
	epochId: 14,
	createdAt: "2026-03-01T08:00:00Z",
	members: [
		{
			nodeId: "node-alpha-8f",
			alias: "Primary Gateway Node",
			walletId: "0x71C...9A32",
			role: "Admin",
			joinedAt: "10m ago",
			isOnline: true,
			clearanceLevel: "TopSecret",
		},
		{
			nodeId: "node-beta-3a",
			alias: "Storage Relay Edge",
			walletId: "0x892...B14E",
			role: "Participant",
			joinedAt: "8m ago",
			isOnline: true,
			clearanceLevel: "TopSecret",
		},
		{
			nodeId: "node-gamma-7c",
			alias: "Worker Compute Cluster",
			walletId: "0x3A1...C881",
			role: "Participant",
			joinedAt: "5m ago",
			isOnline: true,
			clearanceLevel: "Secret",
		},
		{
			nodeId: "node-delta-9e",
			alias: "Auditor Observer Node",
			walletId: "0x9F4...D002",
			role: "Observer",
			joinedAt: "2m ago",
			isOnline: false,
			clearanceLevel: "Confidential",
		},
	],
};

const DEFAULT_MESSAGES: ChatMessage[] = [
	{
		id: "msg-101",
		senderId: "node-alpha-8f",
		senderAlias: "Primary Gateway Node",
		content:
			"Group room epoch 14 key distribution verified across all online members.",
		timestamp: "10:14 AM",
		clearanceLevel: "TopSecret",
		ephemeral: false,
	},
	{
		id: "msg-102",
		senderId: "node-beta-3a",
		senderAlias: "Storage Relay Edge",
		content: "Ack. Vector index replication delta received and persisted.",
		timestamp: "10:15 AM",
		clearanceLevel: "TopSecret",
		ephemeral: false,
	},
	{
		id: "msg-103",
		senderId: "node-gamma-7c",
		senderAlias: "Worker Compute Cluster",
		content:
			"Ephemeral telemetry token verified. Preparing compute execution burst.",
		timestamp: "10:18 AM",
		clearanceLevel: "Secret",
		ephemeral: true,
	},
];

/**
 * Helper to get Tailwind color classes for clearance levels.
 */
function getClearanceBadgeStyles(level: ClearanceLevel) {
	switch (level) {
		case "TopSecret":
			return {
				bg: "bg-red-500/15 border-red-500/40 text-red-400",
				bannerBg:
					"bg-gradient-to-r from-red-950/80 via-rose-900/40 to-red-950/80 border-red-500/40 text-red-200",
				icon: ShieldAlert,
			};
		case "Secret":
			return {
				bg: "bg-amber-500/15 border-amber-500/40 text-amber-300",
				bannerBg:
					"bg-gradient-to-r from-amber-950/80 via-amber-900/40 to-amber-950/80 border-amber-500/40 text-amber-200",
				icon: Shield,
			};
		case "Confidential":
			return {
				bg: "bg-cyan-500/15 border-cyan-500/40 text-cyan-300",
				bannerBg:
					"bg-gradient-to-r from-cyan-950/80 via-cyan-900/40 to-cyan-950/80 border-cyan-500/40 text-cyan-200",
				icon: ShieldCheck,
			};
		default:
			return {
				bg: "bg-emerald-500/15 border-emerald-500/40 text-emerald-300",
				bannerBg:
					"bg-gradient-to-r from-emerald-950/80 via-emerald-900/40 to-emerald-950/80 border-emerald-500/40 text-emerald-200",
				icon: ShieldCheck,
			};
	}
}

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted clearance warning banner into ClearanceBanner component and wrapped in React.memo()
 * 🎯 Why: Prevents re-rendering the security clearance banner during chat message typing.
 * 📊 Impact: Eliminates DOM re-evaluation for static security banner on state changes.
 */
const ClearanceBanner = React.memo(function ClearanceBanner({
	minClearance,
}: {
	minClearance: ClearanceLevel;
}) {
	const styles = getClearanceBadgeStyles(minClearance);
	const IconComponent = styles.icon;

	return (
		<section
			className={`px-4 py-2.5 rounded-xl border flex items-center justify-between gap-3 shadow-md ${styles.bannerBg}`}
			aria-label="Security Clearance Banner"
		>
			<div className="flex items-center gap-2.5 text-xs font-mono">
				<IconComponent
					className="w-4 h-4 flex-shrink-0 animate-pulse"
					aria-hidden="true"
				/>
				<div>
					<span className="font-bold tracking-wider uppercase block">
						MINIMUM REQUIRED CLEARANCE: {minClearance}
					</span>
					<span className="text-[10px] opacity-80 block">
						End-to-End ChaCha20-Poly1305 / AES-256-GCM Group Mesh Encryption
					</span>
				</div>
			</div>

			<div className="hidden sm:flex items-center gap-2">
				<span className="text-[10px] font-mono px-2 py-0.5 rounded bg-black/40 border border-white/10 text-white/80 uppercase">
					E2EE Active
				</span>
				<Lock className="w-3.5 h-3.5 text-emerald-400" aria-hidden="true" />
			</div>
		</section>
	);
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted overlapping avatar group into AvatarGroup component and wrapped in React.memo()
 * 🎯 Why: Re-renders of participant avatars during state changes in chat or search query were causing DOM churn.
 * 📊 Impact: Keeps roster head avatars cached unless members list changes.
 */
const AvatarGroup = React.memo(function AvatarGroup({
	members,
	maxDisplay = 4,
}: {
	members: GroupMember[];
	maxDisplay?: number;
}) {
	const visible = members.slice(0, maxDisplay);
	const overflow = members.length - maxDisplay;

	return (
		<section
			className="flex items-center -space-x-2 overflow-hidden"
			aria-label="Participant Avatars"
		>
			{visible.map((m) => (
				<div
					key={m.nodeId}
					className="relative inline-block w-7 h-7 rounded-full border-2 border-slate-900 bg-slate-800 flex items-center justify-center text-[10px] font-mono font-semibold text-emerald-400 ring-1 ring-white/10"
					title={`${m.alias} (${m.nodeId}) - ${m.role}`}
				>
					{m.avatarUrl ? (
						<img
							src={m.avatarUrl}
							alt={m.alias}
							className="w-full h-full rounded-full object-cover"
						/>
					) : (
						<span>{m.alias.substring(0, 2).toUpperCase()}</span>
					)}
					<span
						className={`absolute bottom-0 right-0 w-2 h-2 rounded-full border border-slate-900 ${
							m.isOnline ? "bg-emerald-400" : "bg-slate-500"
						}`}
					/>
				</div>
			))}
			{overflow > 0 && (
				<div className="w-7 h-7 rounded-full border-2 border-slate-900 bg-slate-800/90 text-slate-300 flex items-center justify-center text-[10px] font-mono font-bold ring-1 ring-white/10">
					+{overflow}
				</div>
			)}
		</section>
	);
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted individual member row into ParticipantRosterItem and wrapped in React.memo()
 * 🎯 Why: Filtering or typing in roster search re-rendered all roster items continuously.
 * 📊 Impact: Reduces roster render cost to O(1) for unchanged participant rows.
 */
const ParticipantRosterItem = React.memo(function ParticipantRosterItem({
	member,
	isAdmin,
	onEject,
}: {
	member: GroupMember;
	isAdmin: boolean;
	onEject?: (nodeId: string) => void;
}) {
	const badgeStyles = getClearanceBadgeStyles(
		member.clearanceLevel || "Unclassified",
	);

	return (
		<div className="p-3 rounded-xl bg-slate-900/60 border border-white/5 hover:border-white/10 transition-colors flex items-center justify-between gap-2">
			<div className="flex items-center gap-2.5 truncate">
				<div className="relative flex-shrink-0">
					<div className="w-8 h-8 rounded-lg bg-slate-800 border border-white/10 flex items-center justify-center text-xs font-mono font-bold text-emerald-400">
						{member.alias.substring(0, 2).toUpperCase()}
					</div>
					<span
						className={`absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5 rounded-full border-2 border-slate-950 ${
							member.isOnline ? "bg-emerald-400" : "bg-slate-600"
						}`}
					/>
				</div>

				<div className="truncate">
					<div className="flex items-center gap-1.5 truncate">
						<span className="text-xs font-medium text-white truncate">
							{member.alias}
						</span>
						{member.role === "Admin" && (
							<span className="text-[9px] px-1.5 py-0.2 rounded font-mono bg-purple-500/20 text-purple-300 border border-purple-500/30 uppercase">
								Admin
							</span>
						)}
						{member.role === "Observer" && (
							<span className="text-[9px] px-1.5 py-0.2 rounded font-mono bg-slate-700/50 text-slate-300 border border-slate-600/40 uppercase">
								Observer
							</span>
						)}
					</div>
					<div className="flex items-center gap-2 text-[10px] text-slate-400 font-mono mt-0.5 truncate">
						<span>{member.nodeId}</span>
						{member.clearanceLevel && (
							<span
								className={`px-1 py-0.2 rounded text-[9px] border ${badgeStyles.bg}`}
							>
								{member.clearanceLevel}
							</span>
						)}
					</div>
				</div>
			</div>

			{/* Admin Action Control: Eject Member */}
			{isAdmin && member.role !== "Admin" && onEject && (
				<button
					type="button"
					onClick={() => onEject(member.nodeId)}
					aria-label={`Eject participant ${member.alias}`}
					title="Eject member & auto-rotate epoch secret"
					className="p-1.5 rounded-lg bg-red-500/10 hover:bg-red-500/20 text-red-400 border border-red-500/30 transition-colors flex-shrink-0 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-400"
				>
					<UserMinus className="w-3.5 h-3.5" aria-hidden="true" />
				</button>
			)}
		</div>
	);
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted ParticipantRoster component and wrapped in React.memo()
 * 🎯 Why: Isolates participant roster state and searching from chat log re-renders.
 * 📊 Impact: Smooth typing and roster filtering.
 */
export const ParticipantRoster = React.memo(function ParticipantRoster({
	members,
	isAdmin = false,
	onEjectMember,
}: {
	members: GroupMember[];
	isAdmin?: boolean;
	onEjectMember?: (nodeId: string) => void;
}) {
	const [searchQuery, setSearchQuery] = useState("");

	const filteredMembers = useMemo(() => {
		const q = searchQuery.toLowerCase();
		return members.filter(
			(m) =>
				m.alias.toLowerCase().includes(q) ||
				m.nodeId.toLowerCase().includes(q) ||
				m.walletId?.toLowerCase().includes(q),
		);
	}, [members, searchQuery]);

	return (
		<div className="flex flex-col h-full bg-slate-900/40 border-l border-white/10 w-full lg:w-80 flex-shrink-0">
			{/* Roster Header */}
			<div className="p-4 border-b border-white/10 flex items-center justify-between">
				<div className="flex items-center gap-2">
					<Users className="w-4 h-4 text-emerald-400" aria-hidden="true" />
					<h3 className="text-xs font-semibold text-white uppercase tracking-wider">
						Member Roster
					</h3>
				</div>
				<span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-emerald-500/10 border border-emerald-500/30 text-emerald-400">
					{members.length} Active
				</span>
			</div>

			{/* Roster Search Bar */}
			<div className="p-3">
				<div className="relative">
					<Search
						className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2 text-slate-500"
						aria-hidden="true"
					/>
					<input
						type="text"
						placeholder="Search roster..."
						value={searchQuery}
						onChange={(e) => setSearchQuery(e.target.value)}
						className="w-full bg-slate-950/80 border border-white/10 text-xs rounded-lg pl-8 pr-3 py-1.5 text-white placeholder-slate-500 focus:outline-none focus:border-emerald-500/50 font-mono"
						aria-label="Search participant roster"
					/>
				</div>
			</div>

			{/* Roster Member List */}
			<div className="flex-1 overflow-y-auto p-3 space-y-2">
				{filteredMembers.length === 0 ? (
					<p className="text-xs text-slate-500 text-center py-6 font-mono">
						No members found matching "{searchQuery}"
					</p>
				) : (
					filteredMembers.map((member) => (
						<ParticipantRosterItem
							key={member.nodeId}
							member={member}
							isAdmin={isAdmin}
							onEject={onEjectMember}
						/>
					))
				)}
			</div>

			{/* Roster Footer Security Summary */}
			<div className="p-3 border-t border-white/10 bg-slate-950/40 text-[10px] text-slate-400 font-mono flex items-center gap-2">
				<KeyRound
					className="w-3.5 h-3.5 text-emerald-400 flex-shrink-0"
					aria-hidden="true"
				/>
				<span>Forward Secrecy: Epoch Key Sync Enabled</span>
			</div>
		</div>
	);
});

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted ChatMessageItem component and wrapped in React.memo()
 * 🎯 Why: Message list items do not need re-rendering when typing in message box.
 * 📊 Impact: Prevents O(N) re-renders during text typing in group room.
 */
const ChatMessageItem = React.memo(function ChatMessageItem({
	msg,
	currentUserId,
}: {
	msg: ChatMessage;
	currentUserId?: string;
}) {
	const isSelf =
		msg.senderId === currentUserId || msg.senderId === "node-alpha-8f";
	const badgeStyles = getClearanceBadgeStyles(
		msg.clearanceLevel || "Unclassified",
	);

	return (
		<div className={`flex flex-col ${isSelf ? "items-end" : "items-start"}`}>
			<div className="flex items-center gap-2 mb-1 px-1 text-[10px] font-mono text-slate-400">
				<span className="font-semibold text-slate-200">{msg.senderAlias}</span>
				<span className="text-slate-500">({msg.senderId})</span>
				<span>{msg.timestamp}</span>
				{msg.ephemeral && (
					<span
						className="flex items-center gap-1 text-amber-400"
						title="Off-The-Record Ephemeral Message"
					>
						<Flame className="w-3 h-3" aria-hidden="true" />
						<span className="text-[9px]">OTR</span>
					</span>
				)}
				{msg.clearanceLevel && (
					<span
						className={`px-1 py-0.2 rounded text-[9px] border ${badgeStyles.bg}`}
					>
						{msg.clearanceLevel}
					</span>
				)}
			</div>

			<div
				className={`max-w-[85%] rounded-2xl px-4 py-2.5 text-xs leading-relaxed shadow-sm ${
					isSelf
						? "bg-emerald-600/20 border border-emerald-500/30 text-emerald-100 rounded-br-none"
						: "bg-slate-800/80 border border-white/10 text-slate-200 rounded-bl-none"
				}`}
			>
				<p className="whitespace-pre-wrap break-words">{msg.content}</p>
			</div>
		</div>
	);
});

/**
 * Accessible Topic Edit Dialog Component.
 */
const TopicEditModal = React.memo(function TopicEditModal({
	isOpen,
	initialTopic,
	onSave,
	onClose,
}: {
	isOpen: boolean;
	initialTopic: string;
	onSave: (topic: string) => void;
	onClose: () => void;
}) {
	const [topicInput, setTopicInput] = useState(initialTopic);

	useEffect(() => {
		setTopicInput(initialTopic);
	}, [initialTopic]);

	useEffect(() => {
		const handleKeyDown = (e: KeyboardEvent) => {
			if (e.key === "Escape" && isOpen) {
				onClose();
			}
		};
		window.addEventListener("keydown", handleKeyDown);
		return () => window.removeEventListener("keydown", handleKeyDown);
	}, [isOpen, onClose]);

	if (!isOpen) return null;

	return (
		<div
			className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/70 backdrop-blur-sm"
			role="dialog"
			aria-modal="true"
			aria-labelledby="topic-modal-title"
		>
			<div className="bg-slate-900 border border-white/15 rounded-2xl max-w-lg w-full p-6 shadow-2xl space-y-4 relative">
				<div className="flex items-center justify-between border-b border-white/10 pb-3">
					<div className="flex items-center gap-2">
						<Edit3 className="w-4 h-4 text-emerald-400" aria-hidden="true" />
						<h3
							id="topic-modal-title"
							className="text-sm font-semibold text-white"
						>
							Update Room Topic & Objective
						</h3>
					</div>
					<button
						type="button"
						onClick={onClose}
						aria-label="Close topic modal"
						className="p-1 rounded-lg text-slate-400 hover:text-white transition-colors"
					>
						<X className="w-4 h-4" aria-hidden="true" />
					</button>
				</div>

				<div className="space-y-2">
					<label
						htmlFor="room-topic-input"
						className="text-xs text-slate-400 block font-mono"
					>
						Room Topic
					</label>
					<textarea
						id="room-topic-input"
						rows={3}
						value={topicInput}
						onChange={(e) => setTopicInput(e.target.value)}
						placeholder="Enter group room topic..."
						className="w-full bg-slate-950 border border-white/10 rounded-xl p-3 text-xs text-white placeholder-slate-500 focus:outline-none focus:border-emerald-500 font-mono"
					/>
				</div>

				<div className="flex justify-end gap-2 pt-2 border-t border-white/10">
					<button
						type="button"
						onClick={onClose}
						className="px-4 py-2 rounded-xl bg-slate-800 text-slate-300 text-xs font-medium hover:bg-slate-700 transition-colors"
					>
						Cancel
					</button>
					<button
						type="button"
						onClick={() => {
							onSave(topicInput);
							onClose();
						}}
						className="px-4 py-2 rounded-xl bg-emerald-500/20 text-emerald-400 border border-emerald-500/40 text-xs font-medium hover:bg-emerald-500/30 transition-colors"
					>
						Save Topic
					</button>
				</div>
			</div>
		</div>
	);
});

/**
 * GroupRoomView component provides a multi-participant telecom room view with
 * room name, description, topic display, accessible topic edit dialog, overlapping avatar group,
 * minimum clearance warning banner, member roster sidebar, and encrypted broadcast chat.
 */
export const GroupRoomView: React.FC<GroupRoomViewProps> = ({
	room = DEFAULT_ROOM,
	currentUserId = "node-alpha-8f",
	messages = DEFAULT_MESSAGES,
	onSendMessage,
	onUpdateTopic,
	onEjectMember,
	onRotateEpoch,
	onClose,
	className = "",
}) => {
	const [activeRoom, setActiveRoom] = useState<GroupRoom>(room);
	const [chatLog, setChatLog] = useState<ChatMessage[]>(messages);
	const [inputText, setInputText] = useState("");
	const [isEphemeral, setIsEphemeral] = useState(false);
	const [isTopicModalOpen, setIsTopicModalOpen] = useState(false);
	const [isRosterOpenMobile, setIsRosterOpenMobile] = useState(false);

	useEffect(() => {
		setActiveRoom(room);
	}, [room]);

	useEffect(() => {
		setChatLog(messages);
	}, [messages]);

	const isAdmin = useMemo(() => {
		const currentMember = activeRoom.members.find(
			(m) => m.nodeId === currentUserId,
		);
		return currentMember ? currentMember.role === "Admin" : true;
	}, [activeRoom.members, currentUserId]);

	const handleSend = useCallback(() => {
		const trimmed = inputText.trim();
		if (!trimmed) return;

		const newMsg: ChatMessage = {
			id: `msg-${Date.now()}`,
			senderId: currentUserId,
			senderAlias: "Local Node (You)",
			content: trimmed,
			timestamp: new Date().toLocaleTimeString([], {
				hour: "2-digit",
				minute: "2-digit",
			}),
			clearanceLevel: activeRoom.minClearance,
			ephemeral: isEphemeral,
		};

		setChatLog((prev) => [...prev, newMsg]);

		if (onSendMessage) {
			onSendMessage(trimmed, isEphemeral);
		}

		setInputText("");
	}, [
		inputText,
		currentUserId,
		activeRoom.minClearance,
		isEphemeral,
		onSendMessage,
	]);

	const handleRotateEpoch = useCallback(() => {
		setActiveRoom((prev) => ({
			...prev,
			epochId: (prev.epochId || 1) + 1,
		}));

		if (onRotateEpoch) {
			onRotateEpoch();
		}
	}, [onRotateEpoch]);

	const handleEjectMember = useCallback(
		(nodeId: string) => {
			setActiveRoom((prev) => ({
				...prev,
				epochId: (prev.epochId || 1) + 1,
				members: prev.members.filter((m) => m.nodeId !== nodeId),
			}));

			if (onEjectMember) {
				onEjectMember(nodeId);
			}
		},
		[onEjectMember],
	);

	const handleSaveTopic = useCallback(
		(newTopic: string) => {
			setActiveRoom((prev) => ({
				...prev,
				topic: newTopic,
			}));

			if (onUpdateTopic) {
				onUpdateTopic(newTopic);
			}
		},
		[onUpdateTopic],
	);

	return (
		<div
			className={`relative w-full h-screen font-sans bg-slate-950 text-white flex flex-col overflow-hidden ${className}`}
		>
			{/* Header Bar */}
			<header className="border-b border-white/10 bg-slate-900/80 backdrop-blur-md px-4 py-3 flex items-center justify-between gap-4">
				<div className="flex items-center gap-3 truncate">
					{onClose && (
						<button
							type="button"
							onClick={onClose}
							aria-label="Back to Telecom view"
							className="p-1.5 rounded-lg bg-white/5 hover:bg-white/10 text-white/70 hover:text-white transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400"
						>
							<ArrowLeft className="w-4 h-4" aria-hidden="true" />
						</button>
					)}

					<div className="w-9 h-9 rounded-xl bg-emerald-500/10 border border-emerald-500/30 flex items-center justify-center text-emerald-400 flex-shrink-0">
						<Hash className="w-5 h-5" aria-hidden="true" />
					</div>

					<div className="truncate">
						<div className="flex items-center gap-2 truncate">
							<h1 className="text-sm font-semibold tracking-wide text-white truncate">
								{activeRoom.name}
							</h1>
							<span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-slate-800 text-slate-300 border border-white/10 flex-shrink-0">
								Epoch {activeRoom.epochId || 1}
							</span>
						</div>
						{activeRoom.description && (
							<p className="text-[10px] text-slate-400 truncate mt-0.5">
								{activeRoom.description}
							</p>
						)}
					</div>
				</div>

				{/* Header Right Actions & Overlapping Avatars */}
				<div className="flex items-center gap-3 flex-shrink-0">
					<div className="hidden sm:block">
						<AvatarGroup members={activeRoom.members} />
					</div>

					{isAdmin && (
						<button
							type="button"
							onClick={handleRotateEpoch}
							aria-label="Rotate Epoch Secret Key"
							title="Manually rotate group epoch secret key"
							className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-xl bg-purple-500/10 border border-purple-500/30 text-purple-300 text-xs font-mono hover:bg-purple-500/20 transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-purple-400"
						>
							<RefreshCw className="w-3.5 h-3.5" aria-hidden="true" />
							<span className="hidden md:inline">Rotate Epoch</span>
						</button>
					)}

					<button
						type="button"
						onClick={() => setIsRosterOpenMobile((prev) => !prev)}
						aria-label="Toggle participant roster"
						className="lg:hidden p-2 rounded-xl bg-slate-800 border border-white/10 text-slate-300 hover:text-white"
					>
						<Users className="w-4 h-4" aria-hidden="true" />
					</button>
				</div>
			</header>

			{/* Main Container */}
			<div className="flex-1 flex min-h-0 relative">
				{/* Chat & Room Pane */}
				<main className="flex-1 flex flex-col min-w-0 bg-slate-950/60 p-4 space-y-4">
					{/* Minimum Required Clearance Banner */}
					<ClearanceBanner minClearance={activeRoom.minClearance} />

					{/* Topic & Objective Display */}
					<div className="p-3.5 rounded-xl bg-slate-900/60 border border-white/10 flex items-start justify-between gap-3">
						<div className="flex items-start gap-2.5">
							<Info
								className="w-4 h-4 text-emerald-400 mt-0.5 flex-shrink-0"
								aria-hidden="true"
							/>
							<div>
								<span className="text-[10px] font-mono text-slate-400 uppercase tracking-wider block">
									Current Room Topic / Mission Objective
								</span>
								<p className="text-xs text-slate-200 mt-0.5 leading-relaxed font-mono">
									{activeRoom.topic || "No specific topic set for this room."}
								</p>
							</div>
						</div>

						{isAdmin && (
							<button
								type="button"
								onClick={() => setIsTopicModalOpen(true)}
								aria-label="Edit room topic"
								className="p-1.5 rounded-lg bg-white/5 hover:bg-white/10 text-slate-300 hover:text-white border border-white/10 transition-colors flex-shrink-0"
							>
								<Edit3 className="w-3.5 h-3.5" aria-hidden="true" />
							</button>
						)}
					</div>

					{/* Encrypted Broadcast Chat Stream */}
					<div className="flex-1 overflow-y-auto p-3 space-y-3 bg-slate-900/30 rounded-2xl border border-white/5">
						{chatLog.map((msg) => (
							<ChatMessageItem
								key={msg.id}
								msg={msg}
								currentUserId={currentUserId}
							/>
						))}
					</div>

					{/* Message Input Footer Controls */}
					<footer className="p-3 rounded-2xl bg-slate-900/80 border border-white/10 space-y-2">
						<div className="flex items-center gap-2">
							<button
								type="button"
								onClick={() => setIsEphemeral((prev) => !prev)}
								aria-label={
									isEphemeral
										? "Disable ephemeral mode"
										: "Enable ephemeral OTR mode"
								}
								title={
									isEphemeral
										? "Off-the-Record Ephemeral mode active (not stored long-term)"
										: "Standard persistent encrypted message"
								}
								className={`p-2.5 rounded-xl border transition-colors flex-shrink-0 ${
									isEphemeral
										? "bg-amber-500/20 border-amber-500/40 text-amber-300"
										: "bg-slate-800 border-white/10 text-slate-400 hover:text-slate-200"
								}`}
							>
								<Flame className="w-4 h-4" aria-hidden="true" />
							</button>

							<input
								type="text"
								value={inputText}
								onChange={(e) => setInputText(e.target.value)}
								onKeyDown={(e) => e.key === "Enter" && handleSend()}
								placeholder={`Broadcast to #${activeRoom.name}... ${
									isEphemeral ? "(Off-the-Record Ephemeral)" : ""
								}`}
								className="flex-1 bg-slate-950 border border-white/10 text-xs rounded-xl px-3.5 py-2.5 text-white placeholder-slate-500 focus:outline-none focus:border-emerald-500/50 font-mono"
							/>

							<button
								type="button"
								onClick={handleSend}
								disabled={!inputText.trim()}
								aria-label="Send group message"
								className="px-4 py-2.5 rounded-xl bg-emerald-500 text-slate-950 hover:bg-emerald-400 disabled:opacity-40 font-medium text-xs transition-colors flex items-center gap-1.5 flex-shrink-0"
							>
								<Send className="w-3.5 h-3.5" aria-hidden="true" />
								<span className="hidden sm:inline">Send</span>
							</button>
						</div>

						<div className="flex items-center justify-between px-1 text-[9px] font-mono text-slate-500">
							<span>Press Enter to broadcast message</span>
							<span className="flex items-center gap-1">
								<Lock
									className="w-2.5 h-2.5 text-emerald-400"
									aria-hidden="true"
								/>
								<span>ChaCha20-Poly1305 / AES-256-GCM Encrypted</span>
							</span>
						</div>
					</footer>
				</main>

				{/* Participant Roster Sidebar */}
				<div
					className={`lg:static fixed inset-y-0 right-0 z-40 transition-transform duration-200 ${
						isRosterOpenMobile
							? "translate-x-0"
							: "translate-x-full lg:translate-x-0"
					}`}
				>
					<ParticipantRoster
						members={activeRoom.members}
						isAdmin={isAdmin}
						onEjectMember={handleEjectMember}
					/>
				</div>
			</div>

			{/* Accessible Topic Edit Modal */}
			<TopicEditModal
				isOpen={isTopicModalOpen}
				initialTopic={activeRoom.topic || ""}
				onSave={handleSaveTopic}
				onClose={() => setIsTopicModalOpen(false)}
			/>
		</div>
	);
};

export default GroupRoomView;
