import {
	Check,
	CheckCircle2,
	Eye,
	Laptop,
	Layout,
	Moon,
	Sliders,
	Sparkles,
	Sun,
	Type,
} from "lucide-react";
import { motion } from "motion/react";
import React, { useEffect, useState } from "react";
import { useTheme } from "../../lib/theme/theme-provider";

export type ThemePreset = "studio-dark" | "studio-bone" | "cyberpunk" | "system";
export type TypographyStyle = "sans" | "mono";

export interface AppearanceSettings {
	preset: ThemePreset;
	typography: TypographyStyle;
	borderedIcons: boolean;
	hoverAttenuation: boolean;
}

const STORAGE_KEYS = {
	preset: "xavier_theme_preset",
	typography: "xavier_typography",
	borderedIcons: "xavier_bordered_icons",
	hoverAttenuation: "xavier_hover_attenuation",
};

export default function AppearancePage() {
	const { theme, setTheme } = useTheme();

	const [preset, setPreset] = useState<ThemePreset>(() => {
		const saved = localStorage.getItem(STORAGE_KEYS.preset);
		if (saved && ["studio-dark", "studio-bone", "cyberpunk", "system"].includes(saved)) {
			return saved as ThemePreset;
		}
		if (theme === "system") return "system";
		if (theme === "light") return "studio-bone";
		return "studio-dark";
	});

	const [typography, setTypography] = useState<TypographyStyle>(() => {
		const saved = localStorage.getItem(STORAGE_KEYS.typography);
		return saved === "mono" ? "mono" : "sans";
	});

	const [borderedIcons, setBorderedIcons] = useState<boolean>(() => {
		const saved = localStorage.getItem(STORAGE_KEYS.borderedIcons);
		return saved !== "false"; // default true
	});

	const [hoverAttenuation, setHoverAttenuation] = useState<boolean>(() => {
		const saved = localStorage.getItem(STORAGE_KEYS.hoverAttenuation);
		return saved === "true"; // default false
	});

	const [sampleInput, setSampleInput] = useState("xavier-node-01");

	// Handle theme preset selection
	const handleSelectPreset = (newPreset: ThemePreset) => {
		setPreset(newPreset);
		localStorage.setItem(STORAGE_KEYS.preset, newPreset);

		if (newPreset === "studio-bone") {
			setTheme("light");
		} else if (newPreset === "system") {
			setTheme("system");
		} else {
			// studio-dark or cyberpunk
			setTheme("dark");
		}
	};

	const handleToggleTypography = (style: TypographyStyle) => {
		setTypography(style);
		localStorage.setItem(STORAGE_KEYS.typography, style);
	};

	const handleToggleBorderedIcons = (val: boolean) => {
		setBorderedIcons(val);
		localStorage.setItem(STORAGE_KEYS.borderedIcons, String(val));
	};

	const handleToggleHoverAttenuation = (val: boolean) => {
		setHoverAttenuation(val);
		localStorage.setItem(STORAGE_KEYS.hoverAttenuation, String(val));
	};

	const presetsConfig: {
		id: ThemePreset;
		title: string;
		subtitle: string;
		description: string;
		badge: string;
		previewBg: string;
		previewBorder: string;
		previewText: string;
		previewAccent: string;
		icon: React.ReactNode;
	}[] = [
		{
			id: "studio-dark",
			title: "Studio Dark",
			subtitle: "Predeterminado",
			description:
				"Deep charcoal background, borderless surface, electric accent.",
			badge: "Default / Predeterminado",
			previewBg: "bg-[#090a0f]",
			previewBorder: "border-white/10",
			previewText: "text-white/90",
			previewAccent: "bg-[#39ff14]",
			icon: <Moon className="w-4 h-4 text-emerald-400" />,
		},
		{
			id: "studio-bone",
			title: "Bone White",
			subtitle: "Hueso Blanco",
			description:
				"Soft warm off-white parchment, high readability.",
			badge: "Soft Parchment",
			previewBg: "bg-[#f5f2eb]",
			previewBorder: "border-neutral-300",
			previewText: "text-neutral-900",
			previewAccent: "bg-emerald-600",
			icon: <Sun className="w-4 h-4 text-amber-500" />,
		},
		{
			id: "cyberpunk",
			title: "Xavier Cyberpunk",
			subtitle: "Clásico",
			description:
				"Historic green neon #39ff14 terminal theme.",
			badge: "Secondary / Clásico",
			previewBg: "bg-[#000000]",
			previewBorder: "border-[#39ff14]/40",
			previewText: "text-[#39ff14]",
			previewAccent: "bg-[#39ff14]",
			icon: <Sparkles className="w-4 h-4 text-[#39ff14]" />,
		},
		{
			id: "system",
			title: "System",
			subtitle: "Sincronizado con SO",
			description:
				"Matches host OS appearance automatically.",
			badge: "Auto Sync",
			previewBg: "bg-[#12131a]",
			previewBorder: "border-slate-700",
			previewText: "text-slate-200",
			previewAccent: "bg-cyan-500",
			icon: <Laptop className="w-4 h-4 text-cyan-400" />,
		},
	];

	return (
		<motion.div
			initial={{ opacity: 0, y: 8 }}
			animate={{ opacity: 1, y: 0 }}
			transition={{ duration: 0.2 }}
			className="flex flex-col gap-8 pb-12 p-6 max-w-6xl mx-auto"
		>
			{/* Header */}
			<div className="flex flex-col gap-1 border-b border-white/5 pb-6">
				<div className="flex items-center gap-3">
					<div className="p-2.5 rounded-2xl bg-[#39ff14]/10 border border-[#39ff14]/20 text-[#39ff14]">
						<Layout className="w-6 h-6" />
					</div>
					<div>
						<h2 className="text-2xl font-light text-white tracking-tight">
							Appearance / Aspecto
						</h2>
						<p className="text-sm text-white/50 mt-0.5">
							Customize theme, typography, and visual ergonomics
						</p>
					</div>
				</div>
			</div>

			{/* Section: Theme Preset */}
			<section className="space-y-4">
				<div className="flex items-center gap-2">
					<Sparkles className="w-4 h-4 text-[#39ff14]" />
					<h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/60">
						Theme Preset
					</h3>
				</div>

				<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
					{presetsConfig.map((p) => {
						const isSelected = preset === p.id;
						return (
							<button
								key={p.id}
								type="button"
								onClick={() => handleSelectPreset(p.id)}
								className={`group relative text-left flex flex-col justify-between p-5 rounded-2xl border transition-all duration-200 ${
									isSelected
										? "bg-white/[0.06] border-[#39ff14] shadow-[0_0_20px_rgba(57,255,20,0.15)] ring-1 ring-[#39ff14]"
										: "bg-[#050505]/40 border-white/10 hover:border-white/20 hover:bg-white/[0.03]"
								}`}
							>
								<div>
									{/* Top header inside card */}
									<div className="flex items-center justify-between mb-3">
										<div className="flex items-center gap-2">
											{p.icon}
											<span className="text-xs font-mono text-white/40">
												{p.subtitle}
											</span>
										</div>
										{isSelected && (
											<div className="p-1 rounded-full bg-[#39ff14] text-black">
												<Check className="w-3 h-3 stroke-[3]" />
											</div>
										)}
									</div>

									<h4 className="text-base font-medium text-white group-hover:text-[#39ff14] transition-colors">
										{p.title}
									</h4>
									<p className="text-xs text-white/50 mt-1 line-clamp-2">
										{p.description}
									</p>
								</div>

								{/* Visual mini-preview inside card */}
								<div className="mt-5 pt-4 border-t border-white/5">
									<div
										className={`p-3 rounded-xl border ${p.previewBg} ${p.previewBorder} flex items-center justify-between`}
									>
										<div className="flex items-center gap-2">
											<div
												className={`w-2.5 h-2.5 rounded-full ${p.previewAccent}`}
											/>
											<span
												className={`text-[10px] font-mono font-medium ${p.previewText}`}
											>
												{p.badge}
											</span>
										</div>
										<span className="text-[9px] font-mono opacity-40 uppercase">
											{p.id}
										</span>
									</div>
								</div>
							</button>
						);
					})}
				</div>
			</section>

			{/* Section: Typography */}
			<section className="bg-[#050505]/30 border border-white/5 rounded-[28px] p-6 space-y-4">
				<div className="flex items-center gap-2">
					<Type className="w-4 h-4 text-[#39ff14]" />
					<h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/60">
						Typography
					</h3>
				</div>

				<div className="grid grid-cols-1 md:grid-cols-2 gap-4">
					<button
						type="button"
						onClick={() => handleToggleTypography("sans")}
						className={`flex items-center justify-between p-4 rounded-xl border transition-all ${
							typography === "sans"
								? "bg-white/[0.06] border-[#39ff14] text-white"
								: "bg-black/20 border-white/5 text-white/60 hover:text-white"
						}`}
					>
						<div className="text-left">
							<p className="text-sm font-sans font-medium">Native System Font</p>
							<p className="text-xs text-white/40 mt-0.5 font-sans">
								Clean, accessible proportional sans-serif interface typeface
							</p>
						</div>
						<div
							className={`w-4 h-4 rounded-full border flex items-center justify-center ${
								typography === "sans"
									? "border-[#39ff14] bg-[#39ff14]"
									: "border-white/30"
							}`}
						>
							{typography === "sans" && (
								<div className="w-1.5 h-1.5 rounded-full bg-black" />
							)}
						</div>
					</button>

					<button
						type="button"
						onClick={() => handleToggleTypography("mono")}
						className={`flex items-center justify-between p-4 rounded-xl border transition-all ${
							typography === "mono"
								? "bg-white/[0.06] border-[#39ff14] text-white"
								: "bg-black/20 border-white/5 text-white/60 hover:text-white"
						}`}
					>
						<div className="text-left">
							<p className="text-sm font-mono font-medium">Monospace Typography</p>
							<p className="text-xs text-white/40 mt-0.5 font-mono">
								Terminal-inspired high precision monospaced code font
							</p>
						</div>
						<div
							className={`w-4 h-4 rounded-full border flex items-center justify-center ${
								typography === "mono"
									? "border-[#39ff14] bg-[#39ff14]"
									: "border-white/30"
							}`}
						>
							{typography === "mono" && (
								<div className="w-1.5 h-1.5 rounded-full bg-black" />
							)}
						</div>
					</button>
				</div>
			</section>

			{/* Section: Micro-interactions & Borders */}
			<section className="bg-[#050505]/30 border border-white/5 rounded-[28px] p-6 space-y-4">
				<div className="flex items-center gap-2">
					<Sliders className="w-4 h-4 text-[#39ff14]" />
					<h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/60">
						Micro-interactions & Borders
					</h3>
				</div>

				<div className="grid grid-cols-1 md:grid-cols-2 gap-4">
					{/* Toggle for Outlined/Bordered Icons */}
					<div className="flex items-center justify-between p-4 rounded-xl bg-black/20 border border-white/5">
						<div>
							<p className="text-sm font-medium text-white/90">
								Outlined / Bordered Icons
							</p>
							<p className="text-xs text-white/40 mt-0.5">
								Render defined stroke borders around status icons and avatars
							</p>
						</div>
						<button
							type="button"
							role="switch"
							aria-checked={borderedIcons}
							onClick={() => handleToggleBorderedIcons(!borderedIcons)}
							aria-label="Toggle Outlined or Bordered Icons"
							className={`relative w-11 h-6 rounded-full transition-colors ${
								borderedIcons ? "bg-[#39ff14]/30 border border-[#39ff14]" : "bg-white/10 border border-white/10"
							}`}
						>
							<div
								className={`absolute top-0.5 w-4 h-4 rounded-full transition-all ${
									borderedIcons
										? "left-[calc(100%-18px)] bg-[#39ff14]"
										: "left-1 bg-white/40"
								}`}
							/>
						</button>
					</div>

					{/* Toggle for Press & Hover Attenuation */}
					<div className="flex items-center justify-between p-4 rounded-xl bg-black/20 border border-white/5">
						<div>
							<p className="text-sm font-medium text-white/90">
								Press & Hover Attenuation
							</p>
							<p className="text-xs text-white/40 mt-0.5">
								Soften contrast and scale animations during cursor interactions
							</p>
						</div>
						<button
							type="button"
							role="switch"
							aria-checked={hoverAttenuation}
							onClick={() => handleToggleHoverAttenuation(!hoverAttenuation)}
							aria-label="Toggle Press and Hover Attenuation"
							className={`relative w-11 h-6 rounded-full transition-colors ${
								hoverAttenuation ? "bg-[#39ff14]/30 border border-[#39ff14]" : "bg-white/10 border border-white/10"
							}`}
						>
							<div
								className={`absolute top-0.5 w-4 h-4 rounded-full transition-all ${
									hoverAttenuation
										? "left-[calc(100%-18px)] bg-[#39ff14]"
										: "left-1 bg-white/40"
								}`}
							/>
						</button>
					</div>
				</div>
			</section>

			{/* Section: Interactive Preview */}
			<section className="bg-[#050505]/30 border border-white/5 rounded-[28px] p-6 space-y-4">
				<div className="flex items-center justify-between">
					<div className="flex items-center gap-2">
						<Eye className="w-4 h-4 text-[#39ff14]" />
						<h3 className="text-xs uppercase tracking-[0.2em] font-bold text-white/60">
							Interactive Preview
						</h3>
					</div>
					<span className="text-[10px] font-mono text-white/30 uppercase tracking-wider">
						Live UI Ergonomics
					</span>
				</div>

				<div
					className={`p-6 rounded-2xl border transition-all duration-300 ${
						typography === "mono" ? "font-mono" : "font-sans"
					} ${
						preset === "studio-bone"
							? "bg-[#f5f2eb] text-neutral-900 border-neutral-300"
							: preset === "cyberpunk"
							? "bg-[#000000] text-[#39ff14] border-[#39ff14]/50 shadow-[0_0_30px_rgba(57,255,20,0.1)]"
							: "bg-[#090a0f] text-white/90 border-white/10"
					}`}
				>
					<div className="flex flex-col gap-6">
						<div className="flex flex-wrap items-center justify-between gap-4">
							<div className="flex items-center gap-3">
								<div
									className={`p-2 rounded-xl ${
										borderedIcons ? "border border-current" : ""
									} ${
										preset === "cyberpunk"
											? "bg-[#39ff14]/10 text-[#39ff14]"
											: preset === "studio-bone"
											? "bg-emerald-100 text-emerald-800"
											: "bg-[#39ff14]/10 text-[#39ff14]"
									}`}
								>
									<CheckCircle2 className="w-5 h-5" />
								</div>
								<div>
									<h4 className="text-sm font-bold">Xavier Mesh Gateway</h4>
									<p className="text-xs opacity-60">Node #8100 · Active Session</p>
								</div>
							</div>

							<div className="flex items-center gap-2">
								<span
									className={`px-2.5 py-1 rounded-full text-xs font-medium border ${
										borderedIcons ? "border-current" : "border-transparent"
									} ${
										preset === "cyberpunk"
											? "bg-[#39ff14]/20 text-[#39ff14] border-[#39ff14]"
											: preset === "studio-bone"
											? "bg-emerald-200 text-emerald-900"
											: "bg-[#39ff14]/15 text-[#39ff14] border-[#39ff14]/30"
									}`}
								>
									P2P Ready
								</span>
								<span className="px-2.5 py-1 rounded-full text-xs font-medium bg-cyan-500/15 text-cyan-400 border border-cyan-500/30">
									TLS 1.3
								</span>
							</div>
						</div>

						<div className="grid grid-cols-1 md:grid-cols-2 gap-4 pt-2">
							<div className="space-y-1.5">
								<label htmlFor="preview-node-alias" className="text-xs opacity-70 block font-medium">
									Node Identifier / Alias
								</label>
								<input
									id="preview-node-alias"
									type="text"
									value={sampleInput}
									onChange={(e) => setSampleInput(e.target.value)}
									className={`w-full px-4 py-2.5 rounded-xl text-xs outline-none border transition-all ${
										preset === "studio-bone"
											? "bg-white text-neutral-900 border-neutral-300 focus:border-emerald-600"
											: preset === "cyberpunk"
											? "bg-black text-[#39ff14] border-[#39ff14]/60 focus:border-[#39ff14] focus:shadow-[0_0_10px_rgba(57,255,20,0.5)]"
											: "bg-black/40 text-white border-white/10 focus:border-[#39ff14]/50"
									}`}
								/>
							</div>

							<div className="flex items-end gap-3">
								<button
									type="button"
									className={`px-5 py-2.5 rounded-xl text-xs font-bold transition-all ${
										hoverAttenuation ? "hover:opacity-80 active:scale-98" : "hover:scale-102 active:scale-95"
									} ${
										preset === "studio-bone"
											? "bg-emerald-700 text-white shadow-sm"
											: preset === "cyberpunk"
											? "bg-[#39ff14] text-black shadow-[0_0_15px_rgba(57,255,20,0.4)]"
											: "bg-[#39ff14] text-black hover:shadow-[0_0_20px_rgba(57,255,20,0.4)]"
									}`}
								>
									Execute Command
								</button>
								<button
									type="button"
									className={`px-4 py-2.5 rounded-xl text-xs font-medium border transition-all ${
										preset === "studio-bone"
											? "border-neutral-400 text-neutral-700 hover:bg-neutral-200"
											: "border-white/20 text-white/80 hover:bg-white/10"
									}`}
								>
									Reset
								</button>
							</div>
						</div>
					</div>
				</div>
			</section>
		</motion.div>
	);
}
