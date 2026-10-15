import type React from "react";

interface SectionBlockProps {
	title?: string;
	description?: string;
	children: React.ReactNode;
}

export function SectionBlock({
	title,
	description,
	children,
}: SectionBlockProps) {
	return (
		<div
			className="openui-section-block flex flex-col gap-3 p-4 rounded-xl bg-black/40 border border-[#39ff14]/20 shadow-md"
			data-testid="openui-section-block"
		>
			{(title || description) && (
				<div className="border-b border-[#39ff14]/10 pb-2.5 mb-1">
					{title && (
						<h3 className="text-xs font-bold tracking-widest text-[#39ff14] uppercase font-mono">
							{title}
						</h3>
					)}
					{description && (
						<p className="text-[11px] text-white/60 mt-0.5 leading-relaxed">
							{description}
						</p>
					)}
				</div>
			)}
			<div className="flex flex-col gap-3">{children}</div>
		</div>
	);
}
