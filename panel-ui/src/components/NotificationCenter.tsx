import {
	Activity,
	AlertTriangle,
	Bell,
	Bot,
	Brain,
	CheckCheck,
	Clock,
	RefreshCw,
	Trash2,
	X,
	Zap,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, { useEffect, useMemo, useState } from "react";
import { getApiUrl } from "../api/client";

export interface Notification {
	id: string;
	islandId?: IslandId;
	island_id?: IslandId; // Backend uses snake_case
	title: string;
	body: string;
	timestamp: Date | string;
	read: boolean;
	severity?: "info" | "warning" | "error" | "success";
}

export type IslandId = "system" | "memory" | "agents" | "errors";

export interface Island {
	id: IslandId;
	label: string;
	icon: React.ReactNode;
	color: string;
	bgColor: string;
	borderColor: string;
}

export const ISLANDS: Island[] = [
	{
		id: "system",
		label: "System",
		icon: <Activity className="w-3.5 h-3.5" />,
		color: "text-cyan-400",
		bgColor: "bg-cyan-500/10",
		borderColor: "border-cyan-500/20",
	},
	{
		id: "memory",
		label: "Memory",
		icon: <Brain className="w-3.5 h-3.5" />,
		color: "text-[#39ff14]",
		bgColor: "bg-[#39ff14]/5",
		borderColor: "border-[#39ff14]/15",
	},
	{
		id: "agents",
		label: "Agents",
		icon: <Bot className="w-3.5 h-3.5" />,
		color: "text-purple-400",
		bgColor: "bg-purple-500/10",
		borderColor: "border-purple-500/20",
	},
	{
		id: "errors",
		label: "Errors",
		icon: <AlertTriangle className="w-3.5 h-3.5" />,
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

export interface NotificationCenterProps {
	isOpen: boolean;
	onClose: () => void;
	initialNotifications?: Notification[];
	enableWebSocket?: boolean;
	wsUrl?: string;
}

export const NotificationItem = React.memo(function NotificationItem({
	notif,
	onRead,
	onDismiss,
}: {
	notif: Notification;
	onRead: (id: string) => void;
	onDismiss?: (id: string) => void;
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
			className={`flex gap-3 p-3 rounded-xl transition-all cursor-default group border ${
				notif.read
					? "bg-transparent border-transparent opacity-60 hover:opacity-100"
					: "bg-white/[0.03] border-white/[0.06] hover:bg-white/[0.06]"
			}`}
		>
			<div className="flex-shrink-0 mt-1">
				<div
					className={`w-2 h-2 rounded-full ${severityDot} ${
						notif.read ? "opacity-40" : "animate-pulse"
					}`}
				/>
			</div>
			<div className="flex-1 min-w-0">
				<div className="flex items-center justify-between gap-2">
					<p
						className={`text-xs font-semibold truncate ${
							notif.read ? "text-white/50" : "text-white/90"
						}`}
					>
						{notif.title}
					</p>
					<span className="flex-shrink-0 text-[10px] text-white/30 flex items-center gap-1 font-mono">
						<Clock className="w-3 h-3" />
						{formatRelativeTime(notif.timestamp)}
					</span>
				</div>
				<p
					className={`text-xs mt-1 leading-relaxed ${
						notif.read ? "text-white/30" : "text-white/60"
					}`}
				>
					{notif.body}
				</p>
			</div>
			<div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
				{!notif.read && (
					<button
						type="button"
						onClick={() => onRead(notif.id)}
						aria-label="Mark as read"
						className="p-1 text-white/30 hover:text-white/80 hover:bg-white/10 rounded-lg transition-all"
					>
						<CheckCheck className="w-3.5 h-3.5" aria-hidden="true" />
					</button>
				)}
				{onDismiss && (
					<button
						type="button"
						onClick={() => onDismiss(notif.id)}
						aria-label="Dismiss notification"
						className="p-1 text-white/30 hover:text-red-400 hover:bg-white/10 rounded-lg transition-all"
					>
						<X className="w-3.5 h-3.5" aria-hidden="true" />
					</button>
				)}
			</div>
		</motion.div>
	);
});

export const ToastBanner = React.memo(function ToastBanner({
	toast,
	onDismiss,
}: {
	toast: Notification;
	onDismiss: (id: string) => void;
}) {
	const severityColor = {
		success: "border-green-500/40 bg-green-950/80 text-green-200",
		info: "border-cyan-500/40 bg-cyan-950/80 text-cyan-200",
		warning: "border-amber-500/40 bg-amber-950/80 text-amber-200",
		error: "border-red-500/40 bg-red-950/80 text-red-200",
	}[toast.severity || "info"];

	return (
		<motion.div
			initial={{ opacity: 0, y: -20, scale: 0.95 }}
			animate={{ opacity: 1, y: 0, scale: 1 }}
			exit={{ opacity: 0, y: -10, scale: 0.95 }}
			className={`flex items-start gap-3 p-3.5 rounded-xl border backdrop-blur-md shadow-2xl max-w-sm w-full ${severityColor}`}
		>
			<Bell className="w-4 h-4 mt-0.5 flex-shrink-0 animate-bounce" />
			<div className="flex-1 min-w-0">
				<h4 className="text-xs font-semibold truncate">{toast.title}</h4>
				<p className="text-[11px] opacity-80 mt-0.5 line-clamp-2">
					{toast.body}
				</p>
			</div>
			<button
				type="button"
				onClick={() => onDismiss(toast.id)}
				aria-label="Close toast notification"
				className="p-1 hover:bg-white/10 rounded-lg transition-all text-current opacity-60 hover:opacity-100"
			>
				<X className="w-3.5 h-3.5" aria-hidden="true" />
			</button>
		</motion.div>
	);
});

export function NotificationCenter({
	isOpen,
	onClose,
	initialNotifications = [],
	enableWebSocket = true,
	wsUrl,
}: NotificationCenterProps) {
	const [notifications, setNotifications] = useState<Notification[]>(
		initialNotifications,
	);
	const [toasts, setToasts] = useState<Notification[]>([]);
	const [activeIsland, setActiveIsland] = useState<IslandId | "all">("all");
	const [isLoading, setIsLoading] = useState<boolean>(initialNotifications.length === 0);

	useEffect(() => {
		if (initialNotifications.length > 0) {
			setNotifications(initialNotifications);
			setIsLoading(false);
		}
	}, [initialNotifications]);

	useEffect(() => {
		let isMounted = true;

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
						setNotifications((prev) => {
							if (prev.length === 0) return data;
							const existingIds = new Set(prev.map((n) => n.id));
							const newItems = data.filter((n: Notification) => !existingIds.has(n.id));
							return [...newItems, ...prev];
						});
					}
				}
			} catch (err) {
				console.error("Failed to fetch notifications from API:", err);
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
						const newNotif = event.payload;
						setNotifications((prev) => [newNotif, ...prev]);
						setToasts((prev) => [newNotif, ...prev.slice(0, 4)]);
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

		// Real-time WebSocket Feed
		let ws: WebSocket | null = null;
		if (enableWebSocket) {
			try {
				const targetWsUrl =
					wsUrl ||
					getApiUrl("/v1/maloca/live-sync").replace(/^http/, "ws");
				ws = new WebSocket(targetWsUrl);

				ws.onmessage = (event) => {
					if (!isMounted) return;
					try {
						const raw = JSON.parse(event.data);
						if (raw && (raw.title || raw.body || raw.type === "notification")) {
							const newNotif: Notification = {
								id: raw.id || `ws-${Date.now()}-${Math.random().toString(36).substring(2, 7)}`,
								islandId: raw.islandId || raw.island_id || "system",
								title: raw.title || raw.event_type || "System Alert",
								body: raw.body || raw.message || JSON.stringify(raw),
								timestamp: raw.timestamp || new Date().toISOString(),
								read: false,
								severity: raw.severity || "info",
							};
							setNotifications((prev) => [newNotif, ...prev]);
							setToasts((prev) => [newNotif, ...prev.slice(0, 4)]);
						}
					} catch {
						// Non-JSON message ignored
					}
				};
			} catch (wsErr) {
				console.error("WebSocket notification connection error:", wsErr);
			}
		}

		return () => {
			isMounted = false;
			if (unlistenFn) unlistenFn();
			if (intervalId) clearInterval(intervalId);
			if (ws && ws.readyState === WebSocket.OPEN) {
				ws.close();
			}
		};
	}, [enableWebSocket, wsUrl]);

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

	const clearAll = async () => {
		setNotifications([]);

		try {
			const token = getToken();
			await fetch(getApiUrl("/notifications"), {
				method: "DELETE",
				headers: { "X-Xavier-Token": token },
			});
		} catch (err) {
			console.error("Failed to clear notifications:", err);
		}
	};

	const dismissNotification = (id: string) => {
		setNotifications((prev) => prev.filter((n) => n.id !== id));
	};

	const dismissToast = (id: string) => {
		setToasts((prev) => prev.filter((t) => t.id !== id));
	};

	const filteredNotifications = useMemo(() => {
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
			{/* Real-time Toast Overlay Container */}
			<div className="fixed top-5 right-5 z-[100] flex flex-col gap-2.5 pointer-events-none">
				<AnimatePresence>
					{toasts.map((toast) => (
						<div key={toast.id} className="pointer-events-auto">
							<ToastBanner toast={toast} onDismiss={dismissToast} />
						</div>
					))}
				</AnimatePresence>
			</div>

			{/* Drawer Slide-Over Panel */}
			<AnimatePresence>
				{isOpen && (
					<>
						{/* Backdrop */}
						<motion.div
							initial={{ opacity: 0 }}
							animate={{ opacity: 1 }}
							exit={{ opacity: 0 }}
							onClick={onClose}
							aria-hidden="true"
							className="fixed inset-0 bg-black/60 backdrop-blur-sm z-[80]"
						/>

						{/* Notification Drawer Container */}
						<motion.div
							initial={{ x: "100%" }}
							animate={{ x: 0 }}
							exit={{ x: "100%" }}
							transition={{ type: "spring", damping: 25, stiffness: 200 }}
							className="fixed top-0 right-0 bottom-0 w-full max-w-md bg-[#0a0a0c] border-l border-white/10 shadow-2xl z-[90] flex flex-col overflow-hidden"
						>
							{/* Drawer Header */}
							<div className="flex items-center justify-between px-5 py-4 border-b border-white/[0.08] bg-black/40">
								<div className="flex items-center gap-2.5">
									<Bell className="w-4 h-4 text-cyan-400" />
									<h2 className="text-sm font-bold text-white tracking-wide">
										Notification Center
									</h2>
									{unreadCount > 0 && (
										<span className="px-2 py-0.5 bg-cyan-500/20 border border-cyan-500/30 text-cyan-300 text-[10px] font-bold rounded-full">
											{unreadCount} new
										</span>
									)}
								</div>
								<div className="flex items-center gap-1.5">
									{unreadCount > 0 && (
										<button
											type="button"
											onClick={markAllRead}
											className="flex items-center gap-1 px-2.5 py-1 text-[11px] text-white/50 hover:text-white hover:bg-white/10 rounded-lg transition-all"
										>
											<CheckCheck className="w-3.5 h-3.5" aria-hidden="true" />
											Mark all read
										</button>
									)}
									{notifications.length > 0 && (
										<button
											type="button"
											onClick={clearAll}
											aria-label="Clear all notifications"
											className="p-1.5 text-white/40 hover:text-red-400 hover:bg-white/10 rounded-lg transition-all"
										>
											<Trash2 className="w-4 h-4" aria-hidden="true" />
										</button>
									)}
									<button
										type="button"
										onClick={onClose}
										aria-label="Close Notification Center"
										className="p-1.5 text-white/40 hover:text-white hover:bg-white/10 rounded-lg transition-all ml-1"
									>
										<X className="w-4 h-4" aria-hidden="true" />
									</button>
								</div>
							</div>

							{/* Island Filter Tabs */}
							<div className="flex items-center gap-1 px-4 py-2.5 border-b border-white/[0.06] bg-white/[0.01] overflow-x-auto">
								<button
									type="button"
									onClick={() => setActiveIsland("all")}
									className={`flex-shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium tracking-wide transition-all ${
										activeIsland === "all"
											? "bg-white/15 text-white border border-white/20"
											: "text-white/40 hover:text-white/80 hover:bg-white/5"
									}`}
								>
									<RefreshCw className="w-3 h-3" />
									All
								</button>
								{ISLANDS.map((island) => (
									<button
										type="button"
										key={island.id}
										onClick={() => setActiveIsland(island.id)}
										className={`flex-shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all ${
											activeIsland === island.id
												? `${island.bgColor} ${island.color} border ${island.borderColor}`
												: "text-white/40 hover:text-white/80 hover:bg-white/5"
										}`}
									>
										{island.icon}
										{island.label}
										{islandCounts[island.id] > 0 && (
											<span
												className={`ml-1 px-1.5 py-0.2 rounded-full text-[9px] font-bold ${island.bgColor} ${island.color}`}
											>
												{islandCounts[island.id]}
											</span>
										)}
									</button>
								))}
							</div>

							{/* Notifications Feed */}
							<div className="flex-1 overflow-y-auto p-4 space-y-2.5">
								<AnimatePresence>
									{isLoading ? (
										<div className="space-y-2">
											<div className="h-12 animate-pulse bg-white/5 rounded-lg" />
											<div className="h-12 animate-pulse bg-white/5 rounded-lg" />
											<div className="h-12 animate-pulse bg-white/5 rounded-lg" />
										</div>
									) : filteredNotifications.length === 0 ? (
										<motion.div
											initial={{ opacity: 0 }}
											animate={{ opacity: 1 }}
											className="flex flex-col items-center justify-center py-16 text-white/30 space-y-3"
										>
											<Zap className="w-8 h-8 opacity-40 text-cyan-400" />
											<p className="text-xs font-medium">
												No notifications found
											</p>
										</motion.div>
									) : (
										filteredNotifications.map((n) => (
											<NotificationItem
												key={n.id}
												notif={n}
												onRead={markRead}
												onDismiss={dismissNotification}
											/>
										))
									)}
								</AnimatePresence>
							</div>

							{/* Footer */}
							<div className="px-5 py-3 border-t border-white/[0.06] bg-black/40 flex items-center justify-between text-[10px] text-white/30 font-mono">
								<span>Real-time Alert Stream</span>
								<span className="flex items-center gap-1.5">
									<span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-ping" />
									Connected
								</span>
							</div>
						</motion.div>
					</>
				)}
			</AnimatePresence>
		</>
	);
}

export default NotificationCenter;
