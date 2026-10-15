import { ChevronDown } from "lucide-react";
import { useState } from "react";
import type { AccordionItem } from "../../utils/openuiParser";

interface AccordionProps {
	items: AccordionItem[];
}

export function Accordion({ items }: AccordionProps) {
	const [openMap, setOpenMap] = useState<Record<string, boolean>>({});

	const toggle = (id: string) => {
		setOpenMap((prev) => ({ ...prev, [id]: !prev[id] }));
	};

	if (!items || items.length === 0) return null;

	return (
		<div className="openui-accordion flex flex-col gap-1.5 w-full">
			{items.map((item) => {
				const isOpen = !!openMap[item.id];
				return (
					<div
						key={item.id}
						className="rounded-md border border-white/10 bg-black/30 overflow-hidden text-xs"
					>
						<button
							type="button"
							onClick={() => toggle(item.id)}
							aria-expanded={isOpen}
							className="flex items-center justify-between w-full px-3 py-2 text-left font-mono text-[11px] text-white/80 hover:bg-white/[0.04] transition-colors cursor-pointer"
						>
							<span className="font-medium truncate mr-2">{item.title}</span>
							<ChevronDown
								size={14}
								className={`text-white/50 transition-transform duration-200 shrink-0 ${isOpen ? "rotate-180" : ""}`}
							/>
						</button>
						{isOpen && (
							<div className="px-3 py-2 border-t border-white/5 text-[11px] leading-relaxed text-white/70 bg-black/40 font-mono whitespace-pre-wrap">
								{item.content}
							</div>
						)}
					</div>
				);
			})}
		</div>
	);
}
