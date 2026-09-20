import { Shield, ShieldAlert, ShieldCheck, Unlock } from "lucide-react";
import React from "react";

export type ClearanceLevel =
	| "TopSecret"
	| "Secret"
	| "Confidential"
	| "Unclassified"
	| "topsecret"
	| "secret"
	| "confidential"
	| "unclassified"
	| "TOPSECRET"
	| "SECRET"
	| "CONFIDENTIAL"
	| "UNCLASSIFIED"
	| string;

export interface ClearanceBadgeProps {
	level: ClearanceLevel;
	size?: "sm" | "md" | "lg";
	className?: string;
	showTooltip?: boolean;
	customTooltip?: string;
	showIcon?: boolean;
}

export interface ClearanceConfig {
	canonicalLevel: "TopSecret" | "Secret" | "Confidential" | "Unclassified";
	label: string;
	bgClass: string;
	textClass: string;
	borderClass: string;
	dotClass: string;
	icon: React.ComponentType<{ className?: string }>;
	description: string;
}

export function normalizeClearanceLevel(
	level: ClearanceLevel,
): "TopSecret" | "Secret" | "Confidential" | "Unclassified" {
	if (!level) return "Unclassified";
	const normalized = level
		.toString()
		.trim()
		.replaceAll(/[_\-\s]/g, "")
		.toLowerCase();

	if (normalized === "topsecret" || normalized === "ts") {
		return "TopSecret";
	}
	if (normalized === "secret") {
		return "Secret";
	}
	if (normalized === "confidential") {
		return "Confidential";
	}
	return "Unclassified";
}

export const CLEARANCE_CONFIGS: Record<
	"TopSecret" | "Secret" | "Confidential" | "Unclassified",
	ClearanceConfig
> = {
	TopSecret: {
		canonicalLevel: "TopSecret",
		label: "Top Secret",
		bgClass: "bg-red-500/10 hover:bg-red-500/20",
		textClass: "text-red-400",
		borderClass: "border-red-500/30",
		dotClass: "bg-red-500",
		icon: ShieldAlert,
		description:
			"Top Secret clearance required. Exceptional impact; strictly isolated cryptographic channels.",
	},
	Secret: {
		canonicalLevel: "Secret",
		label: "Secret",
		bgClass: "bg-amber-500/10 hover:bg-amber-500/20",
		textClass: "text-amber-400",
		borderClass: "border-amber-500/30",
		dotClass: "bg-amber-500",
		icon: Shield,
		description:
			"Secret clearance required. Substantial security controls and verified peer identity.",
	},
	Confidential: {
		canonicalLevel: "Confidential",
		label: "Confidential",
		bgClass: "bg-blue-500/10 hover:bg-blue-500/20",
		textClass: "text-blue-400",
		borderClass: "border-blue-500/30",
		dotClass: "bg-blue-500",
		icon: ShieldCheck,
		description:
			"Confidential clearance required. Protected telecom communication for internal operations.",
	},
	Unclassified: {
		canonicalLevel: "Unclassified",
		label: "Unclassified",
		bgClass: "bg-emerald-500/10 hover:bg-emerald-500/20",
		textClass: "text-emerald-400",
		borderClass: "border-emerald-500/30",
		dotClass: "bg-emerald-500",
		icon: Unlock,
		description:
			"Unclassified level. Standard open communication without restriction.",
	},
};

export const ClearanceBadge: React.FC<ClearanceBadgeProps> = React.memo(
	({
		level,
		size = "md",
		className = "",
		showTooltip = true,
		customTooltip,
		showIcon = true,
	}) => {
		const canonical = normalizeClearanceLevel(level);
		const config = CLEARANCE_CONFIGS[canonical];
		const IconComponent = config.icon;

		const sizeClasses = {
			sm: {
				badge: "px-2 py-0.5 text-[10px] gap-1",
				icon: "w-3 h-3",
				dot: "w-1 h-1",
			},
			md: {
				badge: "px-2.5 py-1 text-xs gap-1.5",
				icon: "w-3.5 h-3.5",
				dot: "w-1.5 h-1.5",
			},
			lg: {
				badge: "px-3 py-1.5 text-sm gap-2",
				icon: "w-4 h-4",
				dot: "w-2 h-2",
			},
		}[size];

		const tooltipText = customTooltip || config.description;

		return (
			<div
				className={`relative group inline-flex items-center ${className}`}
				role="status"
				aria-label={`Clearance level: ${config.label}`}
			>
				<span
					className={`inline-flex items-center font-mono font-medium border rounded-full transition-colors cursor-default ${config.bgClass} ${config.textClass} ${config.borderClass} ${sizeClasses.badge}`}
				>
					{/* Status Indicator Dot */}
					<span
						className={`rounded-full shrink-0 ${config.dotClass} ${sizeClasses.dot}`}
						aria-hidden="true"
					/>

					{/* Icon */}
					{showIcon && (
						<IconComponent
							className={`shrink-0 ${sizeClasses.icon}`}
							aria-hidden="true"
						/>
					)}

					{/* Label */}
					<span className="tracking-wide uppercase whitespace-nowrap">
						{config.label}
					</span>
				</span>

				{/* Accessible Tooltip */}
				{showTooltip && (
					<div
						role="tooltip"
						className="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 hidden group-hover:flex group-focus-within:flex flex-col bg-[#0a0a0a]/95 border border-white/10 p-2.5 rounded-lg shadow-2xl text-[11px] text-white/90 whitespace-normal min-w-[200px] max-w-[280px] gap-1 z-[100] backdrop-blur-md pointer-events-none transition-all duration-150"
					>
						<div className="font-bold flex items-center justify-between border-b border-white/10 pb-1">
							<span className="uppercase text-[10px] tracking-wider text-white/60">
								Security Clearance
							</span>
							<span className={`font-mono text-[10px] ${config.textClass}`}>
								{config.label}
							</span>
						</div>
						<p className="text-white/80 leading-snug">{tooltipText}</p>
					</div>
				)}
			</div>
		);
	},
);

ClearanceBadge.displayName = "ClearanceBadge";

export default ClearanceBadge;
