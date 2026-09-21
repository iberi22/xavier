import {
	ArrowLeft,
	Check,
	CheckCheck,
	Clock,
	FileText,
	Flame,
	Lock,
	Paperclip,
	Search,
	Send,
	ShieldCheck,
	User,
	Wifi,
	X,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, {
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";

/** Receipt status for 1-to-1 direct chat messages */
export type ReceiptStatus = "sending" | "sent" | "delivered" | "read";

/** Security clearance levels for encrypted direct messages */
export type ClearanceLevel =
	| "Unclassified"
	| "Internal"
	| "Confidential"
	| "Secret"
	| "TopSecret";

/** Attachment metadata for telecom direct messages */
export interface ChatAttachment {
	name: string;
	size?: string;
	type?: string;
	url?: string;
}

/** 1-to-1 Encrypted Direct Chat Message entity */
export interface DirectChatMessage {
	id: string;
	roomId: string;
	senderId: string;
	senderAlias?: string;
	content: string;
	timestamp: string;
	clearanceLevel?: ClearanceLevel;
	ephemeral?: boolean;
	status: ReceiptStatus;
	isSelf: boolean;
	attachment?: ChatAttachment;
}

/** Peer node metadata for direct 1-to-1 session */
export interface DirectPeerInfo {
	nodeId: string;
	alias: string;
	online: boolean;
	clearanceLevel?: ClearanceLevel;
	lastSeen?: string;
}

/** Component props for DirectChatView */
export interface DirectChatViewProps {
	roomId?: string;
	peer?: DirectPeerInfo;
	messages?: DirectChatMessage[];
	currentUserId?: string;
	onSendMessage?: (
		content: string,
		options?: {
			ephemeral?: boolean;
			clearanceLevel?: ClearanceLevel;
			attachment?: File | null;
		},
	) => void;
	onMarkRead?: (messageId: string) => void;
	onBack?: () => void;
	className?: string;
}

const DEFAULT_PEER: DirectPeerInfo = {
	nodeId: "node-b82f91",
	alias: "Node Beta",
	online: true,
	clearanceLevel: "Secret",
	lastSeen: "Just now",
};

const DEFAULT_MESSAGES: DirectChatMessage[] = [
	{
		id: "msg-101",
		roomId: "room-beta-direct",
		senderId: "node-b82f91",
		senderAlias: "Node Beta",
		content:
			"Encrypted 1-to-1 channel initialized via ChaCha20-Poly1305 key exchange.",
		timestamp: "10:14 AM",
		clearanceLevel: "Secret",
		status: "read",
		isSelf: false,
	},
	{
		id: "msg-102",
		roomId: "room-beta-direct",
		senderId: "node-self",
		senderAlias: "Local Node",
		content:
			"Ack. Symmetric key derived. Ready for secure payload transmission.",
		timestamp: "10:15 AM",
		clearanceLevel: "Secret",
		status: "read",
		isSelf: true,
	},
	{
		id: "msg-103",
		roomId: "room-beta-direct",
		senderId: "node-self",
		senderAlias: "Local Node",
		content: "Sending vector segment checksum for validation.",
		timestamp: "10:16 AM",
		clearanceLevel: "Confidential",
		status: "delivered",
		isSelf: true,
	},
];

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Extracted message bubble item into MessageItem and wrapped in React.memo()
 * 🎯 Why: Typing in the message input box updates input state on every keystroke. Rendering
 *         the message list inline inside a .map() caused O(N) re-renders of all message bubbles.
 * 📊 Impact: Prevents O(N) re-renders of all chat message bubbles during typing interaction.
 */
export const MessageItem = React.memo(function MessageItem({
	msg,
	onMarkRead,
}: {
	msg: DirectChatMessage;
	onMarkRead?: (id: string) => void;
}) {
	useEffect(() => {
		if (!msg.isSelf && msg.status !== "read" && onMarkRead) {
			onMarkRead(msg.id);
		}
	}, [msg.isSelf, msg.status, msg.id, onMarkRead]);

	// Helper for receipt checkmark icons
	const renderReceiptIcon = () => {
		switch (msg.status) {
			case "sending":
				return (
					<span title="Sending...">
						<Clock className="w-3 h-3 text-zinc-500 animate-spin" />
					</span>
				);
			case "sent":
				return (
					<span title="Sent">
						<Check className="w-3.5 h-3.5 text-zinc-400" />
					</span>
				);
			case "delivered":
				return (
					<span title="Delivered">
						<CheckCheck className="w-3.5 h-3.5 text-zinc-400" />
					</span>
				);
			case "read":
				return (
					<span title="Read" aria-label="Read receipt confirmed">
						<CheckCheck className="w-3.5 h-3.5 text-emerald-400" />
					</span>
				);
			default:
				return null;
		}
	};

	// Helper for clearance badge colors
	const getClearanceBadgeClass = (level?: ClearanceLevel) => {
		switch (level) {
			case "TopSecret":
				return "bg-rose-500/20 text-rose-300 border-rose-500/40";
			case "Secret":
				return "bg-amber-500/20 text-amber-300 border-amber-500/40";
			case "Confidential":
				return "bg-cyan-500/20 text-cyan-300 border-cyan-500/40";
			case "Internal":
				return "bg-emerald-500/20 text-emerald-300 border-emerald-500/40";
			default:
				return "bg-zinc-800 text-zinc-400 border-zinc-700";
		}
	};

	return (
		<motion.div
			initial={{ opacity: 0, y: 8, scale: 0.98 }}
			animate={{ opacity: 1, y: 0, scale: 1 }}
			exit={{ opacity: 0, y: -8, scale: 0.98 }}
			transition={{ duration: 0.15, ease: "easeOut" }}
			className={`flex flex-col ${msg.isSelf ? "items-end" : "items-start"} mb-3`}
		>
			{/* Header Info Above Bubble */}
			<div className="flex items-center gap-2 mb-1 px-1 text-[10px] font-mono">
				<span className="font-semibold text-zinc-300">
					{msg.senderAlias || (msg.isSelf ? "You" : "Peer")}
				</span>

				{msg.clearanceLevel && (
					<span
						className={`px-1.5 py-0.2 rounded border uppercase text-[9px] ${getClearanceBadgeClass(
							msg.clearanceLevel,
						)}`}
					>
						{msg.clearanceLevel}
					</span>
				)}

				{msg.ephemeral && (
					<span className="flex items-center gap-1 text-amber-400/90 bg-amber-500/10 px-1.5 py-0.2 rounded border border-amber-500/30 text-[9px]">
						<Flame className="w-2.5 h-2.5" />
						<span>Off-The-Record</span>
					</span>
				)}

				<span className="text-zinc-500">{msg.timestamp}</span>
			</div>

			{/* Message Bubble Container */}
			<div
				className={`max-w-[88%] sm:max-w-[75%] rounded-2xl px-4 py-2.5 text-xs leading-relaxed shadow-lg ${
					msg.isSelf
						? "bg-emerald-600/20 border border-emerald-500/30 text-emerald-50 rounded-br-none"
						: "bg-zinc-800/90 border border-zinc-700/70 text-zinc-100 rounded-bl-none"
				}`}
			>
				<p className="whitespace-pre-wrap break-words">{msg.content}</p>

				{/* Attachment Box */}
				{msg.attachment && (
					<div className="mt-2.5 pt-2 border-t border-white/10 flex items-center gap-2 text-[11px]">
						<FileText className="w-4 h-4 text-emerald-400 flex-shrink-0" />
						<div className="truncate">
							<span className="font-mono truncate block text-zinc-200">
								{msg.attachment.name}
							</span>
							{msg.attachment.size && (
								<span className="text-[9px] text-zinc-400 block font-mono">
									{msg.attachment.size}
								</span>
							)}
						</div>
					</div>
				)}

				{/* Self Receipt Checkmark Footer */}
				{msg.isSelf && (
					<div className="flex items-center justify-end gap-1.5 mt-1 text-[10px] text-zinc-400 font-mono">
						<span className="text-[9px] opacity-70 capitalize">
							{msg.status}
						</span>
						{renderReceiptIcon()}
					</div>
				)}
			</div>
		</motion.div>
	);
});

/**
 * DirectChatView provides a 1-to-1 encrypted conversation window with receipt checkmarks
 * (Sent, Delivered, Read), off-the-record ephemeral support, and auto-scrolling stream.
 */
export const DirectChatView: React.FC<DirectChatViewProps> = ({
	roomId = "room-beta-direct",
	peer = DEFAULT_PEER,
	messages = DEFAULT_MESSAGES,
	onSendMessage,
	onMarkRead,
	onBack,
	className = "",
}) => {
	const [messageList, setMessageList] = useState<DirectChatMessage[]>(messages);
	const [inputText, setInputText] = useState("");
	const [selectedFile, setSelectedFile] = useState<File | null>(null);
	const [isEphemeral, setIsEphemeral] = useState(false);
	const [clearanceLevel, setClearanceLevel] =
		useState<ClearanceLevel>("Secret");
	const [searchQuery, setSearchQuery] = useState("");
	const [isSearchOpen, setIsSearchOpen] = useState(false);

	const fileInputRef = useRef<HTMLInputElement | null>(null);
	const messagesEndRef = useRef<HTMLDivElement | null>(null);

	// Sync external messages prop
	useEffect(() => {
		setMessageList(messages);
	}, [messages]);

	// Auto-scroll stream to bottom with AnimatePresence
	const scrollToBottom = useCallback(() => {
		if (
			messagesEndRef.current &&
			typeof messagesEndRef.current.scrollIntoView === "function"
		) {
			messagesEndRef.current.scrollIntoView({ behavior: "smooth" });
		}
	}, []);

	useEffect(() => {
		scrollToBottom();
	}, [scrollToBottom]);

	// Filter messages by search query if active
	const filteredMessages = useMemo(() => {
		if (!searchQuery.trim()) return messageList;
		const q = searchQuery.toLowerCase();
		return messageList.filter(
			(m) =>
				m.content.toLowerCase().includes(q) ||
				m.senderAlias?.toLowerCase().includes(q),
		);
	}, [messageList, searchQuery]);

	const handleSend = () => {
		const trimmed = inputText.trim();
		if (!trimmed && !selectedFile) return;

		const newMsg: DirectChatMessage = {
			id: `msg-${Date.now()}`,
			roomId,
			senderId: "node-self",
			senderAlias: "Local Node",
			content: trimmed,
			timestamp: new Date().toLocaleTimeString([], {
				hour: "2-digit",
				minute: "2-digit",
			}),
			clearanceLevel,
			ephemeral: isEphemeral,
			status: "sending",
			isSelf: true,
			attachment: selectedFile
				? {
						name: selectedFile.name,
						size: `${(selectedFile.size / 1024).toFixed(1)} KB`,
						type: selectedFile.type,
					}
				: undefined,
		};

		// Optimistically update message stream
		setMessageList((prev) => [...prev, newMsg]);

		// Simulate backend delivery & read receipts updates after short intervals
		setTimeout(() => {
			setMessageList((prev) =>
				prev.map((m) => (m.id === newMsg.id ? { ...m, status: "sent" } : m)),
			);
		}, 600);

		setTimeout(() => {
			setMessageList((prev) =>
				prev.map((m) =>
					m.id === newMsg.id ? { ...m, status: "delivered" } : m,
				),
			);
		}, 1400);

		setTimeout(() => {
			setMessageList((prev) =>
				prev.map((m) => (m.id === newMsg.id ? { ...m, status: "read" } : m)),
			);
		}, 2500);

		if (onSendMessage) {
			onSendMessage(trimmed, {
				ephemeral: isEphemeral,
				clearanceLevel,
				attachment: selectedFile,
			});
		}

		setInputText("");
		setSelectedFile(null);
	};

	const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
		if (e.key === "Enter" && !e.shiftKey) {
			e.preventDefault();
			handleSend();
		}
	};

	const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
		if (e.target.files?.[0]) {
			setSelectedFile(e.target.files[0]);
		}
	};

	return (
		<div
			className={`flex flex-col h-full w-full bg-[#08080a] border border-zinc-800 text-zinc-100 rounded-xl overflow-hidden shadow-2xl ${className}`}
		>
			{/* 1-to-1 Room Header */}
			<header className="px-4 py-3 border-b border-zinc-800/80 bg-[#0c0c0e] flex items-center justify-between gap-3">
				<div className="flex items-center gap-3 min-w-0">
					{onBack && (
						<button
							type="button"
							onClick={onBack}
							aria-label="Back to messages list"
							className="p-1.5 rounded-lg text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800/80 transition-colors"
						>
							<ArrowLeft className="w-4 h-4" aria-hidden="true" />
						</button>
					)}

					<div className="relative flex-shrink-0">
						<div className="w-9 h-9 rounded-full bg-emerald-500/10 border border-emerald-500/30 flex items-center justify-center text-emerald-400">
							<User className="w-4 h-4" />
						</div>
						<span
							className={`absolute bottom-0 right-0 w-2.5 h-2.5 rounded-full border-2 border-[#0c0c0e] ${
								peer.online ? "bg-emerald-400" : "bg-zinc-600"
							}`}
						/>
					</div>

					<div className="min-w-0">
						<div className="flex items-center gap-2">
							<h2 className="text-sm font-semibold text-zinc-100 truncate">
								{peer.alias}
							</h2>
							<span className="text-[10px] font-mono px-1.5 py-0.2 rounded bg-zinc-800/80 text-zinc-400 border border-zinc-700/60">
								{peer.nodeId}
							</span>
						</div>
						<p className="text-[10px] text-zinc-400 flex items-center gap-1 mt-0.5">
							<span
								className={peer.online ? "text-emerald-400" : "text-zinc-500"}
							>
								{peer.online
									? "Online · Direct Link Active"
									: `Offline · ${peer.lastSeen}`}
							</span>
						</p>
					</div>
				</div>

				{/* Security & Header Actions */}
				<div className="flex items-center gap-2 flex-shrink-0">
					<button
						type="button"
						onClick={() => setIsSearchOpen((prev) => !prev)}
						aria-label="Search conversation messages"
						className={`p-2 rounded-lg text-xs font-mono transition-colors ${
							isSearchOpen
								? "bg-zinc-800 text-zinc-100"
								: "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/60"
						}`}
					>
						<Search className="w-4 h-4" aria-hidden="true" />
					</button>

					<div className="hidden sm:flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-emerald-500/10 border border-emerald-500/20 text-emerald-400 text-[10px] font-mono">
						<ShieldCheck className="w-3.5 h-3.5" />
						<span>ChaCha20-Poly1305</span>
					</div>
				</div>
			</header>

			{/* Search Bar Collapsible */}
			{isSearchOpen && (
				<div className="px-4 py-2 border-b border-zinc-800 bg-zinc-900/60">
					<div className="relative flex items-center">
						<Search className="w-3.5 h-3.5 absolute left-3 text-zinc-500" />
						<input
							type="text"
							placeholder="Search in encrypted chat..."
							value={searchQuery}
							onChange={(e) => setSearchQuery(e.target.value)}
							className="w-full bg-zinc-950 border border-zinc-800 text-xs rounded-lg pl-8 pr-8 py-1.5 text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-emerald-500/50 font-mono"
						/>
						{searchQuery && (
							<button
								type="button"
								onClick={() => setSearchQuery("")}
								aria-label="Clear message search"
								className="absolute right-2 text-zinc-500 hover:text-zinc-300 p-1"
							>
								<X className="w-3.5 h-3.5" aria-hidden="true" />
							</button>
						)}
					</div>
				</div>
			)}

			{/* Message Stream with AnimatePresence */}
			<div className="flex-1 overflow-y-auto p-4 space-y-2">
				<AnimatePresence initial={false}>
					{filteredMessages.length === 0 ? (
						<motion.div
							initial={{ opacity: 0 }}
							animate={{ opacity: 1 }}
							exit={{ opacity: 0 }}
							className="h-full flex flex-col items-center justify-center text-zinc-500 text-xs my-8"
						>
							<Lock className="w-8 h-8 mb-2 text-zinc-600" />
							<p>1-to-1 Encrypted Direct Channel</p>
							<p className="text-[10px] text-zinc-600 mt-1">
								Messages are end-to-end encrypted with perfect forward secrecy.
							</p>
						</motion.div>
					) : (
						filteredMessages.map((msg) => (
							<MessageItem key={msg.id} msg={msg} onMarkRead={onMarkRead} />
						))
					)}
				</AnimatePresence>
				<div ref={messagesEndRef} />
			</div>

			{/* Attachment Preview Container */}
			{selectedFile && (
				<div className="px-4 py-2 bg-zinc-900/90 border-t border-zinc-800 flex items-center justify-between text-xs">
					<div className="flex items-center gap-2 text-zinc-300 truncate">
						<Paperclip className="w-3.5 h-3.5 text-emerald-400 flex-shrink-0" />
						<span className="font-mono truncate">{selectedFile.name}</span>
						<span className="text-[10px] text-zinc-500 font-mono">
							({(selectedFile.size / 1024).toFixed(1)} KB)
						</span>
					</div>
					<button
						type="button"
						onClick={() => setSelectedFile(null)}
						aria-label="Remove attached file"
						className="text-zinc-500 hover:text-zinc-300 p-1"
					>
						<X className="w-4 h-4" aria-hidden="true" />
					</button>
				</div>
			)}

			{/* Input Toolbar & Controls */}
			<footer className="p-3 border-t border-zinc-800/80 bg-[#0c0c0e]">
				{/* Ephemeral & Clearance Settings Row */}
				<div className="flex items-center justify-between mb-2 px-1 text-xs">
					<div className="flex items-center gap-2">
						{/* OTR Ephemeral Toggle Button */}
						<button
							type="button"
							onClick={() => setIsEphemeral((prev) => !prev)}
							aria-label="Toggle off-the-record ephemeral message mode"
							className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-[10px] font-mono transition-all ${
								isEphemeral
									? "bg-amber-500/20 text-amber-300 border border-amber-500/40"
									: "bg-zinc-900 text-zinc-400 border border-zinc-800 hover:text-zinc-200"
							}`}
						>
							<Flame
								className={`w-3 h-3 ${isEphemeral ? "text-amber-400" : ""}`}
							/>
							<span>{isEphemeral ? "OTR Ephemeral ON" : "Ephemeral Mode"}</span>
						</button>

						{/* Clearance Selector */}
						<select
							value={clearanceLevel}
							onChange={(e) =>
								setClearanceLevel(e.target.value as ClearanceLevel)
							}
							aria-label="Select security clearance level"
							className="bg-zinc-900 border border-zinc-800 text-[10px] font-mono text-zinc-300 px-2 py-1 rounded-lg focus:outline-none focus:border-emerald-500/50 cursor-pointer"
						>
							<option value="Unclassified">Unclassified</option>
							<option value="Internal">Internal</option>
							<option value="Confidential">Confidential</option>
							<option value="Secret">Secret</option>
							<option value="TopSecret">TopSecret</option>
						</select>
					</div>

					<span className="hidden sm:flex items-center gap-1 text-[9px] text-zinc-500 font-mono">
						<Wifi className="w-2.5 h-2.5 text-emerald-400" />
						<span>P2P Channel Active</span>
					</span>
				</div>

				{/* Input Bar */}
				<div className="flex items-center gap-2">
					{/* File Attachment Button */}
					<input
						type="file"
						ref={fileInputRef}
						onChange={handleFileChange}
						className="hidden"
					/>
					<button
						type="button"
						onClick={() => fileInputRef.current?.click()}
						aria-label="Attach file to direct message"
						className={`p-2.5 rounded-lg border transition-colors flex-shrink-0 ${
							selectedFile
								? "bg-emerald-500/20 border-emerald-500/40 text-emerald-400"
								: "bg-zinc-900 border-zinc-800 text-zinc-400 hover:text-zinc-200 hover:border-zinc-700"
						}`}
					>
						<Paperclip className="w-4 h-4" aria-hidden="true" />
					</button>

					{/* Text Input */}
					<input
						type="text"
						placeholder={`Send encrypted message to ${peer.alias}...`}
						value={inputText}
						onChange={(e) => setInputText(e.target.value)}
						onKeyDown={handleKeyDown}
						className="flex-1 bg-zinc-900 border border-zinc-800 text-xs rounded-lg px-3.5 py-2.5 text-zinc-100 placeholder-zinc-500 focus:outline-none focus:border-emerald-500/50 font-mono"
					/>

					{/* Send Button */}
					<button
						type="button"
						onClick={handleSend}
						disabled={!inputText.trim() && !selectedFile}
						aria-label="Send direct message"
						className="p-2.5 rounded-lg bg-emerald-500 text-zinc-950 hover:bg-emerald-400 disabled:opacity-40 disabled:hover:bg-emerald-500 transition-colors flex-shrink-0"
					>
						<Send className="w-4 h-4" aria-hidden="true" />
					</button>
				</div>
			</footer>
		</div>
	);
};

export default DirectChatView;
