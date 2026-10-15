import type React from "react";

interface CardProps {
	title?: string;
	description?: string;
	children: React.ReactNode;
}

export function Card({ title, description, children }: CardProps) {
	return (
		<div
			className="openui-card p-3 rounded-lg bg-white/[0.025] border border-white/10 hover:border-[#39ff14]/30 transition-colors"
			data-testid="openui-card"
		>
			{(title || description) && (
				<div className="mb-2.5">
					{title && (
						<h4 className="text-xs font-semibold text-white/90 tracking-wide font-sans">
							{title}
						</h4>
					)}
					{description && (
						<p className="text-[11px] text-white/50 mt-0.5 leading-relaxed">
							{description}
						</p>
					)}
				</div>
			)}
			<div className="flex flex-col gap-2">{children}</div>
		</div>
	);
}
