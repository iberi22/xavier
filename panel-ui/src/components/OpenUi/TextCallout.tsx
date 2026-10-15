import { AlertCircle, AlertTriangle, CheckCircle2, Info } from "lucide-react";

interface TextCalloutProps {
	variant?: string;
	title?: string;
	content: string;
}

export function TextCallout({
	variant = "info",
	title,
	content,
}: TextCalloutProps) {
	const lowerVariant = variant.toLowerCase();

	let borderClass = "border-cyan-500/20";
	let bgClass = "bg-cyan-500/10";
	let textClass = "text-cyan-400";
	let Icon = Info;

	if (lowerVariant === "warning") {
		borderClass = "border-amber-500/20";
		bgClass = "bg-amber-500/10";
		textClass = "text-amber-400";
		Icon = AlertTriangle;
	} else if (lowerVariant === "success") {
		borderClass = "border-[#39ff14]/20";
		bgClass = "bg-[#39ff14]/10";
		textClass = "text-[#39ff14]";
		Icon = CheckCircle2;
	} else if (lowerVariant === "error" || lowerVariant === "danger") {
		borderClass = "border-red-500/20";
		bgClass = "bg-red-500/10";
		textClass = "text-red-400";
		Icon = AlertCircle;
	}

	return (
		<div
			className={`openui-text-callout flex items-start gap-2.5 p-3 rounded-lg border ${borderClass} ${bgClass} text-xs`}
		>
			<Icon size={16} className={`shrink-0 mt-0.5 ${textClass}`} />
			<div className="flex flex-col gap-0.5">
				{title && (
					<span className={`font-semibold font-mono text-[11px] ${textClass}`}>
						{title}
					</span>
				)}
				<p className="text-white/80 text-[11px] leading-relaxed whitespace-pre-wrap font-sans">
					{content}
				</p>
			</div>
		</div>
	);
}
