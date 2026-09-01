import {
	Activity,
	AlertTriangle,
	Bot,
	Brain,
	CheckCheck,
	Clock,
	RefreshCw,
	X,
	Zap,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect, useMemo, useState } from "react";
import { getApiUrl } from "../api/client";

export interface Notification {
	id: string;
	islandId: IslandId;
	island_id?: IslandId; // Backend uses snake_case
	title: string;
	body: string;
	timestamp: Date | string;
	read: boolean;
	severity?: "info" | "warning" | "error" | "success";
}

type IslandId = "system" | "memory" | "agents" | "errors";

interface Island {
	id: IslandId;
	label: string;
	icon: React.ReactNode;
	color: string;
	bgColor: string;
	borderColor: string;
}

const ISLANDS: Island[] = [
	{
		id: "system",
		label: "System",
		icon: <Activity className="w-3 h-3" />,
		color: "text-cyan-400",
		bgColor: "bg-cyan-500/10",
		borderColor: "border-cyan-500/20",
	},
	{
		id: "memory",
		label: "Memory",
		icon: <Brain className="w-3 h-3" />,
		color: "text-[#39ff14]",
		bgColor: "bg-[#39ff14]/5",
		borderColor: "border-[#39ff14]/15",
	},
	{
		id: "agents",
		label: "Agents",
		icon: <Bot className="w-3 h-3" />,
		color: "text-purple-400",
		bgColor: "bg-purple-500/10",
		borderColor: "border-purple-500/20",
	},
	{
		id: "errors",
		label: "Errors",
		icon: <AlertTriangle className="w-3 h-3" />,
		color: "text-red-400",
		bgColor: "bg-red-500/10",
		borderColor: "border-red-500/20",
	},
];

function isTauriRuntime(): boolean {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function getToken(): string {
	return (
		(import.meta.env.VITE_XAVIER_API_TOKEN as string | undefined) ||
		(typeof localStorage !== "undefined"
			? localStorage.getItem("XAVIER_TOKEN") || ""
			: "")
	);
}

function formatRelativeTime(dateInput: Date | string): string {
	const date = typeof dateInput === "string" ? new Date(dateInput) : dateInput;
	const diff = (Date.now() - date.getTime()) / 1000;
	if (diff < 60) return "just now";
	if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
	if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
	return `${Math.floor(diff / 86400)}d ago`;
}

function NotificationItem({
	notif,
	onRead,
}: {
	notif: Notification;
	onRead: (id: string) => void;
}) {
	const severityDot = {
		success: "bg-green-400",
		info: "bg-cyan-400",
		warning: "bg-amber-400",
		error: "bg-red-400",
	}[notif.severity || "info"];

	return (
		<motion.div
			layout
			initial={{ opacity: 0, y: -4 }}
			animate={{ opacity: 1, y: 0 }}
			exit={{ opacity: 0, x: 20, height: 0 }}
			className={`flex gap-2.5 p-2.5 rounded-lg transition-colors cursor-default group ${
				notif.read ? "bg-transparent" : "bg-white/[0.025]"
			} hover:bg-white/[0.04]`}
		>
			<div className="flex-shrink-0 mt-0.5">
				<div
					className={`w-1.5 h-1.5 rounded-full ${severityDot} ${notif.read ? "opacity-30" : ""}`}
				/>
			</div>
			<div className="flex-1 min-w-0">
				<div className="flex items-center justify-between gap-2">
					<p
						className={`text-[11px] font-medium truncate ${notif.read ? "text-white/40" : "text-white/80"}`}
					>
						{notif.title}
					</p>
					<span className="flex-shrink-0 text-[9px] text-white/20 flex items-center gap-1">
						<Clock className="w-2.5 h-2.5" />
						{formatRelativeTime(notif.timestamp)}
					</span>
				</div>
				<p
					className={`text-[10px] mt-0.5 leading-relaxed ${notif.read ? "text-white/25" : "text-white/45"}`}
				>
					{notif.body}
				</p>
			</div>
			{!notif.read && (
				<button
					type="button"
					onClick={() => onRead(notif.id)}
					aria-label="Mark as read"
					className="flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity p-0.5 text-white/20 hover:text-white/50"
				>
					<X className="w-3 h-3" aria-hidden="true" />
				</button>
			)}
		</motion.div>
	);
}

interface NotificationsDropdownProps {
	onClose: () => void;
	anchorRef?: React.RefObject<HTMLElement>;
}

export default React.memo(function NotificationsDropdown({
	onClose,
}: NotificationsDropdownProps) {
	const [notifications, setNotifications] = useState<Notification[]>([]);
	const [activeIsland, setActiveIsland] = useState<IslandId | "all">("all");
	const [isLoading, setIsLoading] = useState<boolean>(true);

	useEffect(() => {
		let isMounted = true;

		// 1. Fetch notifications
		const fetchNotifications = async () => {
			try {
				const token = getToken();
				const response = await fetch(getApiUrl("/notifications"), {
					headers: { "X-Xavier-Token": token },
				});
				if (!response.ok) {
					if (response.status === 401) {
						if (isMounted) setIsLoading(false);
						return;
					}
					throw new Error(`HTTP ${response.status}`);
				}
				if (isMounted) {
					const data = await response.json();
					if (Array.isArray(data)) {
						setNotifications(data);
					}
				}
			} catch (err) {
				console.error("Failed to fetch notifications:", err);
			} finally {
				if (isMounted) setIsLoading(false);
			}
		};

		fetchNotifications();

		let unlistenFn: (() => void) | null = null;
		let intervalId: ReturnType<typeof setInterval> | null = null;

		if (isTauriRuntime()) {
			import("@tauri-apps/api/event")
				.then(({ listen }) => {
					return listen<Notification>("new-notification", (event) => {
						if (!isMounted) return;
						setNotifications((prev) => [event.payload, ...prev]);
					});
				})
				.then((unlisten) => {
					if (isMounted) unlistenFn = unlisten;
					else unlisten();
				})
				.catch(() => {
					// Ignore non-Tauri environment errors
				});
		} else {
			intervalId = setInterval(fetchNotifications, 30_000);
		}

		return () => {
			isMounted = false;
			if (unlistenFn) unlistenFn();
			if (intervalId) clearInterval(intervalId);
		};
	}, []);

	/**
	 * ⚡ Bolt Performance Optimization
	 *
	 * 💡 What: Wrapped filtered, unreadCount, and islandCounts in useMemo(), and NotificationsDropdown in React.memo().
	 *          Replaced O(M*N) islandCounts calculation with O(N) single-pass iteration.
	 * 🎯 Why: These derived computations were re-calculating on every render (e.g. when switching tabs), doing expensive array allocations and O(M*N) filtering.
	 * 📊 Impact: O(1) filtering on non-notification state changes (like tab switching). Prevents unnecessary re-renders of the dropdown when parent state changes.
	 */
	const unreadCount = useMemo(
		() => notifications.reduce((count, n) => count + (n.read ? 0 : 1), 0),
		[notifications],
	);

	const markRead = async (id: string) => {
		setNotifications((prev) =>
			prev.map((n) => (n.id === id ? { ...n, read: true } : n)),
		);

		try {
			const token = getToken();
			await fetch(getApiUrl(`/notifications/${id}/read`), {
				method: "PATCH",
				headers: { "X-Xavier-Token": token },
			});
		} catch (err) {
			console.error("Failed to mark notification as read:", err);
		}
	};

	const markAllRead = async () => {
		setNotifications((prev) => prev.map((n) => ({ ...n, read: true })));

		try {
			const token = getToken();
			await fetch(getApiUrl("/notifications/read-all"), {
				method: "PATCH",
				headers: { "X-Xavier-Token": token },
			});
		} catch (err) {
			console.error("Failed to mark all notifications as read:", err);
		}
	};

	const filtered = useMemo(() => {
		return activeIsland === "all"
			? notifications
			: notifications.filter(
					(n) => (n.island_id || n.islandId) === activeIsland,
				);
	}, [activeIsland, notifications]);

	const islandCounts = useMemo(() => {
		const counts = ISLANDS.reduce(
			(acc, island) => {
				acc[island.id] = 0;
				return acc;
			},
			{} as Record<IslandId, number>,
		);

		for (const n of notifications) {
			if (!n.read) {
				const id = (n.island_id || n.islandId) as IslandId;
				if (id && counts[id] !== undefined) {
					counts[id]++;
				}
			}
		}
		return counts;
	}, [notifications]);

	return (
		<>
			{/* Backdrop */}
			<div
				className="fixed inset-0 z-[65]"
				onClick={onClose}
				aria-hidden="true"
			/>

			<motion.div
				initial={{ opacity: 0, y: -8, scale: 0.97 }}
				animate={{ opacity: 1, y: 0, scale: 1 }}
				exit={{ opacity: 0, y: -8, scale: 0.97 }}
				transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}
				className="absolute right-0 top-full mt-2 w-80 max-h-[420px] bg-[#060606] border border-white/[0.07] rounded-xl shadow-2xl flex flex-col overflow-hidden z-[66]"
				style={{ top: "100%" }}
			>
				{/* Header */}
				<div className="flex items-center justify-between px-4 py-3 border-b border-white/[0.05]">
					<div className="flex items-center gap-2">
						<span className="text-xs font-semibold text-white/80 tracking-wide">
							Notifications
						</span>
						{unreadCount > 0 && (
							<span className="px-1.5 py-0.5 bg-[#39ff14]/15 border border-[#39ff14]/20 text-[#39ff14] text-[9px] font-bold rounded-full">
								{unreadCount}
							</span>
						)}
					</div>
					<div className="flex items-center gap-1">
						{unreadCount > 0 && (
							<button
								type="button"
								onClick={markAllRead}
								className="flex items-center gap-1 px-2 py-1 text-[9px] text-white/30 hover:text-white/60 hover:bg-white/5 rounded-lg transition-all"
							>
								<CheckCheck className="w-3 h-3" aria-hidden="true" />
								All read
							</button>
						)}
						<button
							type="button"
							onClick={onClose}
							aria-label="Close notifications"
							className="p-1 text-white/20 hover:text-white/50 hover:bg-white/5 rounded-lg transition-all"
						>
							<X className="w-3.5 h-3.5" aria-hidden="true" />
						</button>
					</div>
				</div>

				{/* Island filter tabs */}
				<div className="flex gap-1 px-3 py-2 border-b border-white/[0.04] overflow-x-auto">
					<button
						type="button"
						onClick={() => setActiveIsland("all")}
						className={`flex-shrink-0 flex items-center gap-1 px-2 py-1 rounded-md text-[9px] uppercase tracking-widest transition-all ${
							activeIsland === "all"
								? "bg-white/10 text-white/80"
								: "text-white/30 hover:text-white/60 hover:bg-white/5"
						}`}
					>
						<RefreshCw className="w-2.5 h-2.5" />
						All
					</button>
					{ISLANDS.map((island) => (
						<button
							type="button"
							key={island.id}
							onClick={() => setActiveIsland(island.id)}
							className={`flex-shrink-0 flex items-center gap-1 px-2 py-1 rounded-md text-[9px] uppercase tracking-widest transition-all relative ${
								activeIsland === island.id
									? `${island.bgColor} ${island.color} border ${island.borderColor}`
									: "text-white/30 hover:text-white/60 hover:bg-white/5"
							}`}
						>
							{island.icon}
							{island.label}
							{islandCounts[island.id] > 0 && (
								<span
									className={`ml-0.5 px-1 py-px rounded-full text-[8px] font-bold ${island.bgColor} ${island.color}`}
								>
									{islandCounts[island.id]}
								</span>
							)}
						</button>
					))}
				</div>

				{/* Notifications list */}
				<div className="flex-1 overflow-y-auto p-2">
					<AnimatePresence>
						{isLoading ? (
							<div className="space-y-2">
								<div className="h-12 animate-pulse bg-white/5 rounded-lg" />
								<div className="h-12 animate-pulse bg-white/5 rounded-lg" />
								<div className="h-12 animate-pulse bg-white/5 rounded-lg" />
							</div>
						) : filtered.length === 0 ? (
							<motion.div
								initial={{ opacity: 0 }}
								animate={{ opacity: 1 }}
								className="flex flex-col items-center justify-center py-8 text-white/20"
							>
								<Zap className="w-6 h-6 mb-2 opacity-30" />
								<p className="text-[11px]">No notifications</p>
							</motion.div>
						) : (
							filtered.map((n) => (
								<NotificationItem key={n.id} notif={n} onRead={markRead} />
							))
						)}
					</AnimatePresence>
				</div>

				{/* Footer */}
				<div className="px-4 py-2 border-t border-white/[0.04] bg-black/20">
					<p className="text-[9px] text-white/15 text-center">
						Persistent notification system active
					</p>
				</div>
			</motion.div>
		</>
	);
});
