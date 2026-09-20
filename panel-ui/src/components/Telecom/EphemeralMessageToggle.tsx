import { ShieldAlert, Timer } from "lucide-react";
import { useCallback, useId } from "react";

export type EphemeralTTL = "30s" | "5m" | "1h";

export interface EphemeralMessageToggleProps {
	/** Whether off-the-record ephemeral autodestruct mode is enabled */
	enabled: boolean;
	/** Active time-to-live setting for messages */
	ttl?: EphemeralTTL;
	/** Callback fired when switch state is toggled */
	onToggle: (enabled: boolean) => void;
	/** Callback fired when TTL selection changes */
	onTtlChange?: (ttl: EphemeralTTL) => void;
	/** Optional disabled state */
	disabled?: boolean;
	/** Optional additional CSS classes */
	className?: string;
}

export const TTL_OPTIONS: {
	value: EphemeralTTL;
	label: string;
	description: string;
}[] = [
	{ value: "30s", label: "30s", description: "30 seconds" },
	{ value: "5m", label: "5m", description: "5 minutes" },
	{ value: "1h", label: "1h", description: "1 hour" },
];

export function EphemeralMessageToggle({
	enabled,
	ttl = "5m",
	onToggle,
	onTtlChange,
	disabled = false,
	className = "",
}: EphemeralMessageToggleProps) {
	const switchId = useId();
	const labelId = useId();
	const descriptionId = useId();

	const handleToggle = useCallback(() => {
		if (!disabled) {
			onToggle(!enabled);
		}
	}, [disabled, enabled, onToggle]);

	const handleTtlSelect = useCallback(
		(selectedTtl: EphemeralTTL) => {
			if (!disabled && onTtlChange) {
				onTtlChange(selectedTtl);
			}
		},
		[disabled, onTtlChange],
	);

	return (
		<div
			className={`rounded-xl border border-white/10 bg-[#08080a] p-4 transition-colors ${
				disabled ? "opacity-50 cursor-not-allowed" : ""
			} ${className}`}
		>
			<div className="flex items-center justify-between gap-3">
				<div className="flex items-center gap-3">
					<div
						className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
							enabled
								? "bg-amber-500/10 text-amber-400 border border-amber-500/20"
								: "bg-white/5 text-white/40 border border-white/5"
						}`}
					>
						<ShieldAlert className="h-5 w-5" aria-hidden="true" />
					</div>
					<div>
						<div className="flex items-center gap-2">
							<span id={labelId} className="text-sm font-semibold text-white">
								Ephemeral Messaging (OTR)
							</span>
							{enabled && (
								<span className="inline-flex items-center rounded-full bg-amber-500/10 px-2 py-0.5 text-[10px] font-medium text-amber-400 border border-amber-500/20">
									OTR Active
								</span>
							)}
						</div>
						<p id={descriptionId} className="text-xs text-white/50">
							Auto-destruct messages after configured TTL duration
						</p>
					</div>
				</div>

				<button
					id={switchId}
					type="button"
					role="switch"
					aria-checked={enabled}
					aria-labelledby={labelId}
					aria-describedby={descriptionId}
					aria-label="Toggle Ephemeral Messaging"
					disabled={disabled}
					onClick={handleToggle}
					className={`relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-[#08080a] disabled:cursor-not-allowed disabled:opacity-50 ${
						enabled ? "bg-amber-500" : "bg-white/15"
					}`}
				>
					<span className="sr-only">Toggle Ephemeral Messaging</span>
					<span
						aria-hidden="true"
						className={`pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow-lg ring-0 transition duration-200 ease-in-out ${
							enabled ? "translate-x-5" : "translate-x-0"
						}`}
					/>
				</button>
			</div>

			{enabled && (
				<div className="mt-4 pt-3 border-t border-white/5 flex items-center justify-between gap-3">
					<div className="flex items-center gap-1.5 text-xs text-white/60">
						<Timer className="h-3.5 w-3.5 text-amber-400" aria-hidden="true" />
						<span>Message TTL:</span>
					</div>

					<div className="flex items-center gap-1 rounded-lg bg-white/5 p-1 border border-white/5">
						{TTL_OPTIONS.map((option) => {
							const isSelected = ttl === option.value;
							return (
								<button
									key={option.value}
									type="button"
									aria-pressed={isSelected}
									aria-label={`TTL ${option.description}`}
									disabled={disabled}
									onClick={() => handleTtlSelect(option.value)}
									className={`px-2.5 py-1 text-xs font-medium rounded-md transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/50 ${
										isSelected
											? "bg-amber-500/20 text-amber-300 border border-amber-500/30 shadow-xs"
											: "text-white/60 hover:text-white hover:bg-white/5"
									} disabled:cursor-not-allowed disabled:opacity-50`}
								>
									{option.label}
								</button>
							);
						})}
					</div>
				</div>
			)}
		</div>
	);
}
