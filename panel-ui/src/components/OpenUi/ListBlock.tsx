import type { ListItem } from "../../utils/openuiParser";

interface ListBlockProps {
	items: ListItem[];
}

export function ListBlock({ items }: ListBlockProps) {
	if (!items || items.length === 0) return null;

	return (
		<div className="openui-list-block flex flex-col gap-1.5 w-full">
			{items.map((item) => (
				<div
					key={item.id}
					className="flex items-start justify-between gap-3 px-3 py-2 rounded-md bg-black/30 border border-white/5 text-xs"
				>
					<div className="flex flex-col gap-0.5 min-w-0">
						{item.title && (
							<span className="font-semibold text-white/90 text-[11px] font-sans">
								{item.title}
							</span>
						)}
						{item.description && (
							<span className="text-white/60 text-[11px] leading-relaxed">
								{item.description}
							</span>
						)}
					</div>
					{item.meta && (
						<span className="shrink-0 px-2 py-0.5 rounded text-[10px] font-mono bg-[#39ff14]/10 text-[#39ff14] border border-[#39ff14]/20">
							{item.meta}
						</span>
					)}
				</div>
			))}
		</div>
	);
}
