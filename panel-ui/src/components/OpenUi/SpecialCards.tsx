import { Folder, GitBranch, HelpCircle } from "lucide-react";

export function QuestionCard({ question }: { question: string }) {
	return (
		<div className="openui-question-card flex items-start gap-3 p-3 rounded-lg bg-indigo-500/10 border border-indigo-500/20 text-indigo-300">
			<HelpCircle size={16} className="shrink-0 mt-0.5 text-indigo-400" />
			<div className="flex flex-col gap-1">
				<span className="text-[10px] font-mono uppercase tracking-wider text-indigo-400 font-semibold">
					Question
				</span>
				<p className="text-xs text-white/90 leading-relaxed font-sans">
					{question}
				</p>
			</div>
		</div>
	);
}

export function DecisionCard({ adr }: { adr: string }) {
	return (
		<div className="openui-decision-card flex items-start gap-3 p-3 rounded-lg bg-emerald-500/10 border border-emerald-500/20 text-emerald-300">
			<GitBranch size={16} className="shrink-0 mt-0.5 text-emerald-400" />
			<div className="flex flex-col gap-1">
				<span className="text-[10px] font-mono uppercase tracking-wider text-emerald-400 font-semibold">
					Architecture Decision (ADR)
				</span>
				<p className="text-xs text-white/90 leading-relaxed font-sans">{adr}</p>
			</div>
		</div>
	);
}

export function ProjectCard({ project }: { project: string }) {
	return (
		<div className="openui-project-card flex items-start gap-3 p-3 rounded-lg bg-purple-500/10 border border-purple-500/20 text-purple-300">
			<Folder size={16} className="shrink-0 mt-0.5 text-purple-400" />
			<div className="flex flex-col gap-1">
				<span className="text-[10px] font-mono uppercase tracking-wider text-purple-400 font-semibold">
					Project Context
				</span>
				<p className="text-xs text-white/90 leading-relaxed font-sans">
					{project}
				</p>
			</div>
		</div>
	);
}

export function InfoCard({ title, value }: { title: string; value: string }) {
	return (
		<div className="openui-info-card flex items-center justify-between p-2.5 rounded-lg bg-white/[0.03] border border-white/10">
			<span className="text-xs text-white/60 font-medium font-sans">
				{title}
			</span>
			<span className="text-xs font-mono font-semibold text-[#39ff14]">
				{value}
			</span>
		</div>
	);
}
