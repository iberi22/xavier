import type { OpenUiNode } from "../../utils/openuiParser";
import { Accordion } from "./Accordion";
import { Card } from "./Card";
import { ListBlock } from "./ListBlock";
import { SectionBlock } from "./SectionBlock";
import {
	DecisionCard,
	InfoCard,
	ProjectCard,
	QuestionCard,
} from "./SpecialCards";
import { TextCallout } from "./TextCallout";

interface RenderSurfaceProps {
	nodes: OpenUiNode[];
}

export function OpenUiNodeRenderer({ node }: { node: OpenUiNode }) {
	switch (node.type) {
		case "SectionBlock":
			return (
				<SectionBlock title={node.title} description={node.description}>
					{node.children.map((child) => (
						<OpenUiNodeRenderer key={child.id} node={child} />
					))}
				</SectionBlock>
			);
		case "Card":
			return (
				<Card title={node.title} description={node.description}>
					{node.children.map((child) => (
						<OpenUiNodeRenderer key={child.id} node={child} />
					))}
				</Card>
			);
		case "TextContent":
			return (
				<div className="openui-text-content text-sm leading-relaxed text-white/80 whitespace-pre-wrap font-sans">
					{node.content}
				</div>
			);
		case "ListBlock":
			return <ListBlock items={node.items} />;
		case "Accordion":
			return <Accordion items={node.items} />;
		case "TextCallout":
			return (
				<TextCallout
					variant={node.variant}
					title={node.title}
					content={node.content}
				/>
			);
		case "QuestionCard":
			return <QuestionCard question={node.question} />;
		case "DecisionCard":
			return <DecisionCard adr={node.adr} />;
		case "ProjectCard":
			return <ProjectCard project={node.project} />;
		case "InfoCard":
			return <InfoCard title={node.title} value={node.value} />;
		case "text":
			return (
				<p className="openui-text text-xs text-white/70 leading-relaxed font-sans whitespace-pre-wrap">
					{node.content}
				</p>
			);
		default:
			return null;
	}
}

export function RenderSurface({ nodes }: RenderSurfaceProps) {
	if (!nodes || nodes.length === 0) {
		return null;
	}

	return (
		<div
			className="render-surface w-full flex flex-col gap-3 my-2"
			data-testid="openui-render-surface"
		>
			<div className="flex items-center justify-between px-3 py-1.5 rounded-lg bg-[#39ff14]/[0.04] border border-[#39ff14]/15 text-[10px] uppercase font-mono tracking-wider text-[#39ff14]/70">
				<div className="flex items-center gap-2">
					<span className="w-1.5 h-1.5 rounded-full bg-[#39ff14] animate-pulse" />
					<span className="font-semibold">OpenUI Render Surface</span>
				</div>
			</div>
			<div className="flex flex-col gap-3 w-full">
				{nodes.map((node) => (
					<OpenUiNodeRenderer key={node.id} node={node} />
				))}
			</div>
		</div>
	);
}

export default RenderSurface;
