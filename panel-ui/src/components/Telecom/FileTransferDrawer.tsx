import {
	AlertCircle,
	ArrowDownLeft,
	ArrowUpRight,
	Check,
	CheckCircle2,
	Copy,
	ExternalLink,
	FileText,
	Pause,
	Play,
	ShieldCheck,
	X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

export interface FileTransferDrawerProps {
	/** Whether the drawer modal is currently visible */
	isOpen: boolean;
	/** Callback fired when drawer close action is requested */
	onClose: () => void;
	/** Unique session ID for the file transfer */
	sessionId?: string;
	/** Name of the file being transferred */
	fileName?: string;
	/** Total file size in bytes */
	fileSize?: number;
	/** Transfer direction (upload or download) */
	direction?: "upload" | "download";
	/** Total number of chunks in the manifest */
	totalChunks?: number;
	/** Number of chunks successfully received/sent */
	receivedChunks?: number;
	/** Explicit progress percentage (0 - 100), calculated automatically if omitted */
	chunkProgress?: number;
	/** Real-time transfer rate in KB/s */
	transferRate?: number;
	/** Expected or computed SHA-256 verification hash */
	sha256Hash?: string;
	/** Current transfer state */
	status?:
		| "idle"
		| "transferring"
		| "verifying"
		| "completed"
		| "failed"
		| "paused";
	/** Callback to trigger opening the completed file */
	onOpenFile?: () => void;
	/** Callback to pause active transfer */
	onPause?: () => void;
	/** Callback to resume paused transfer */
	onResume?: () => void;
	/** Callback to cancel transfer session */
	onCancel?: () => void;
}

/** Formats byte counts into human-readable strings (e.g., 1.5 MB) */
function formatBytes(bytes?: number): string {
	if (bytes === undefined || bytes === null || Number.isNaN(bytes)) {
		return "0 B";
	}
	if (bytes === 0) return "0 B";
	const k = 1024;
	const sizes = ["B", "KB", "MB", "GB", "TB"];
	const i = Math.floor(Math.log(bytes) / Math.log(k));
	const num = bytes / k ** i;
	return `${num.toFixed(num < 10 && i > 0 ? 1 : 0)} ${sizes[i]}`;
}

/** Formats transfer speed rates in KB/s or MB/s */
function formatTransferRate(rateKbps?: number): string {
	if (rateKbps === undefined || rateKbps === null || rateKbps <= 0) {
		return "0 KB/s";
	}
	if (rateKbps >= 1024) {
		return `${(rateKbps / 1024).toFixed(1)} MB/s`;
	}
	return `${rateKbps.toFixed(1)} KB/s`;
}

/** Truncates hash strings for clean UI display */
function truncateHash(hash?: string, chars = 12): string {
	if (!hash) return "—";
	if (hash.length <= chars * 2) return hash;
	return `${hash.slice(0, chars)}...${hash.slice(-chars)}`;
}

export function FileTransferDrawer({
	isOpen,
	onClose,
	sessionId = "sess-000",
	fileName = "unnamed_file.bin",
	fileSize = 0,
	direction = "download",
	totalChunks = 1,
	receivedChunks = 0,
	chunkProgress,
	transferRate = 0,
	sha256Hash = "",
	status = "idle",
	onOpenFile,
	onPause,
	onResume,
	onCancel,
}: FileTransferDrawerProps) {
	const [copiedHash, setCopiedHash] = useState(false);

	// Keyboard navigation support (Escape closes drawer)
	useEffect(() => {
		if (!isOpen) return;

		const handleKeyDown = (e: KeyboardEvent) => {
			if (e.key === "Escape") {
				onClose();
			}
		};

		window.addEventListener("keydown", handleKeyDown);
		return () => window.removeEventListener("keydown", handleKeyDown);
	}, [isOpen, onClose]);

	// Compute percentage progress safely
	const computedChunkProgress = useMemo(() => {
		if (typeof chunkProgress === "number") {
			return Math.min(100, Math.max(0, chunkProgress));
		}
		if (status === "completed") {
			return 100;
		}
		if (totalChunks > 0) {
			return Math.min(
				100,
				Math.max(0, Math.round((receivedChunks / totalChunks) * 100)),
			);
		}
		return 0;
	}, [chunkProgress, status, totalChunks, receivedChunks]);

	// Copy SHA-256 hash to clipboard
	const handleCopyHash = useCallback(async () => {
		if (!sha256Hash) return;
		try {
			await navigator.clipboard.writeText(sha256Hash);
			setCopiedHash(true);
			setTimeout(() => setCopiedHash(false), 2000);
		} catch {
			// Fallback if clipboard API unavailable
			setCopiedHash(false);
		}
	}, [sha256Hash]);

	if (!isOpen) return null;

	const isCompleted = status === "completed";
	const isFailed = status === "failed";
	const isVerifying = status === "verifying";
	const isTransferring = status === "transferring";
	const isPaused = status === "paused";

	return (
		<div
			className="fixed inset-0 z-50 overflow-hidden bg-neutral-950/70 backdrop-blur-xs flex justify-end transition-opacity duration-200"
			role="dialog"
			aria-modal="true"
			aria-labelledby="file-transfer-drawer-title"
		>
			{/* Backdrop click to close */}
			<div className="absolute inset-0" onClick={onClose} aria-hidden="true" />

			{/* Main Drawer Container */}
			<div className="relative z-10 w-full max-w-md bg-neutral-900 text-neutral-100 border-l border-neutral-800 shadow-2xl flex flex-col h-full overflow-y-auto transform transition-transform duration-300 ease-in-out">
				{/* Header */}
				<div className="flex items-center justify-between px-6 py-4 border-b border-neutral-800 bg-neutral-900/90 backdrop-blur-md sticky top-0 z-20">
					<div className="flex items-center space-x-2">
						<ShieldCheck className="w-5 h-5 text-emerald-400" />
						<h2
							id="file-transfer-drawer-title"
							className="text-lg font-semibold text-neutral-100"
						>
							File Transfer Drawer
						</h2>
					</div>
					<button
						type="button"
						onClick={onClose}
						aria-label="Close transfer drawer"
						className="p-1.5 rounded-lg text-neutral-400 hover:text-neutral-100 hover:bg-neutral-800 transition-colors focus:outline-hidden focus:ring-2 focus:ring-emerald-500 cursor-pointer"
					>
						<X className="w-5 h-5" />
					</button>
				</div>

				{/* Body Content */}
				<div className="p-6 space-y-6 flex-1">
					{/* File Information Card */}
					<div className="bg-neutral-800/60 border border-neutral-700/60 rounded-xl p-4 space-y-3">
						<div className="flex items-start justify-between space-x-3">
							<div className="flex items-center space-x-3 min-w-0">
								<div className="p-2.5 bg-emerald-950/60 text-emerald-400 border border-emerald-800/50 rounded-lg shrink-0">
									<FileText className="w-6 h-6" />
								</div>
								<div className="min-w-0">
									<p
										className="font-medium text-neutral-100 truncate text-sm"
										title={fileName}
									>
										{fileName}
									</p>
									<p className="text-xs text-neutral-400 mt-0.5">
										{formatBytes(fileSize)} • Session: {sessionId}
									</p>
								</div>
							</div>

							{/* Direction Badge */}
							<span
								className={`inline-flex items-center px-2.5 py-1 rounded-full text-xs font-medium border shrink-0 ${
									direction === "upload"
										? "bg-indigo-950/70 text-indigo-300 border-indigo-800/60"
										: "bg-cyan-950/70 text-cyan-300 border-cyan-800/60"
								}`}
							>
								{direction === "upload" ? (
									<>
										<ArrowUpRight className="w-3.5 h-3.5 mr-1" />
										Upload
									</>
								) : (
									<>
										<ArrowDownLeft className="w-3.5 h-3.5 mr-1" />
										Download
									</>
								)}
							</span>
						</div>
					</div>

					{/* Chunk Transfer Progress Section */}
					<div className="bg-neutral-800/40 border border-neutral-800 rounded-xl p-4 space-y-3">
						<div className="flex items-center justify-between text-sm">
							<span className="font-medium text-neutral-200">
								Chunk Progress
							</span>
							<span className="font-semibold text-emerald-400">
								{computedChunkProgress}%
							</span>
						</div>

						{/* Progress Bar Track */}
						<div className="w-full bg-neutral-950/80 rounded-full h-3 overflow-hidden border border-neutral-800 p-0.5">
							<div
								className={`h-full rounded-full transition-all duration-300 ease-out ${
									isFailed
										? "bg-rose-500"
										: isCompleted
											? "bg-emerald-500"
											: isPaused
												? "bg-amber-500"
												: "bg-gradient-to-r from-cyan-500 to-emerald-500"
								}`}
								style={{ width: `${computedChunkProgress}%` }}
								role="progressbar"
								aria-valuenow={computedChunkProgress}
								aria-valuemin={0}
								aria-valuemax={100}
							/>
						</div>

						{/* Chunk Stats & Transfer Speed */}
						<div className="flex items-center justify-between text-xs text-neutral-400 pt-1">
							<span>
								{receivedChunks} / {totalChunks} chunks transferred
							</span>
							<span className="font-mono text-neutral-300">
								{formatTransferRate(transferRate)}
							</span>
						</div>
					</div>

					{/* SHA-256 Integrity Verification Section */}
					<div className="bg-neutral-800/40 border border-neutral-800 rounded-xl p-4 space-y-3">
						<div className="flex items-center justify-between">
							<span className="text-xs font-semibold uppercase tracking-wider text-neutral-400 flex items-center gap-1.5">
								<ShieldCheck className="w-4 h-4 text-emerald-400" />
								SHA-256 Checksum
							</span>

							{/* Integrity Status Badge / Checkmark Animation */}
							{isCompleted && (
								<span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-emerald-950/80 text-emerald-300 border border-emerald-700/60 animate-pulse">
									<CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />
									Verified Integrity
								</span>
							)}
							{isVerifying && (
								<span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-amber-950/80 text-amber-300 border border-amber-700/60">
									Verifying...
								</span>
							)}
							{isFailed && (
								<span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-xs font-medium bg-rose-950/80 text-rose-300 border border-rose-700/60">
									<AlertCircle className="w-3.5 h-3.5 text-rose-400" />
									Checksum Mismatch
								</span>
							)}
						</div>

						{/* Hash Display Box with Copy Action */}
						<div className="flex items-center justify-between bg-neutral-950/90 border border-neutral-800 rounded-lg p-2.5">
							<code
								className="text-xs font-mono text-neutral-300 truncate mr-2"
								title={sha256Hash || "Calculating hash..."}
							>
								{sha256Hash
									? truncateHash(sha256Hash, 14)
									: "Calculating SHA-256..."}
							</code>
							<button
								type="button"
								onClick={handleCopyHash}
								disabled={!sha256Hash}
								aria-label="Copy SHA-256 hash"
								className="p-1.5 text-neutral-400 hover:text-neutral-100 bg-neutral-900 hover:bg-neutral-800 border border-neutral-700/50 rounded-md transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed shrink-0"
							>
								{copiedHash ? (
									<Check className="w-3.5 h-3.5 text-emerald-400" />
								) : (
									<Copy className="w-3.5 h-3.5" />
								)}
							</button>
						</div>

						<p className="text-[11px] text-neutral-400">
							{isCompleted
								? " cryptographic SHA-256 checksum verified against chunk manifest."
								: isFailed
									? " Transfer failed checksum verification. The assembled binary does not match."
									: " SHA-256 integrity check will execute automatically upon chunk assembly."}
						</p>
					</div>

					{/* Transfer Control Actions (Pause / Resume / Cancel) */}
					{(isTransferring || isPaused) && (
						<div className="flex items-center space-x-3 pt-2">
							{isTransferring && onPause && (
								<button
									type="button"
									onClick={onPause}
									className="flex-1 inline-flex items-center justify-center px-4 py-2 text-xs font-medium rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-200 border border-neutral-700 transition-colors cursor-pointer"
								>
									<Pause className="w-3.5 h-3.5 mr-1.5 text-amber-400" />
									Pause Transfer
								</button>
							)}
							{isPaused && onResume && (
								<button
									type="button"
									onClick={onResume}
									className="flex-1 inline-flex items-center justify-center px-4 py-2 text-xs font-medium rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-200 border border-neutral-700 transition-colors cursor-pointer"
								>
									<Play className="w-3.5 h-3.5 mr-1.5 text-emerald-400" />
									Resume Transfer
								</button>
							)}
							{onCancel && (
								<button
									type="button"
									onClick={onCancel}
									className="inline-flex items-center justify-center px-4 py-2 text-xs font-medium rounded-lg bg-rose-950/50 hover:bg-rose-900/60 text-rose-300 border border-rose-800/60 transition-colors cursor-pointer"
								>
									Cancel
								</button>
							)}
						</div>
					)}
				</div>

				{/* Footer with Primary Open File Trigger */}
				<div className="p-6 border-t border-neutral-800 bg-neutral-900/90 backdrop-blur-md sticky bottom-0 z-20">
					<button
						type="button"
						onClick={onOpenFile}
						disabled={!isCompleted || !onOpenFile}
						className="w-full inline-flex items-center justify-center px-4 py-3 text-sm font-semibold rounded-xl bg-emerald-600 hover:bg-emerald-500 text-neutral-950 shadow-lg shadow-emerald-950/50 transition-all cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed disabled:bg-neutral-800 disabled:text-neutral-500 disabled:shadow-none"
					>
						<ExternalLink className="w-4 h-4 mr-2" />
						{isCompleted ? "Open Transferred File" : "Awaiting Completion..."}
					</button>
				</div>
			</div>
		</div>
	);
}

export default FileTransferDrawer;
