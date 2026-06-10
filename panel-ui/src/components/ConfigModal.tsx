import {
  Bookmark,
  Brain,
  Cpu,
  Cpu as CpuIcon,
  Database,
  Globe,
  Grid,
  Layers,
  MessageSquare,
  Network,
  Plug,
  Server,
  Share2,
  Shield,
  X,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type React from "react";
import { useMemo, useState } from "react";
import ProvidersPage from "../pages/Settings/Providers";
import SecurityConfigPanel from "../pages/Settings/Security";
import type { BookmarkArtifact, GraphData } from "../types";
import BookmarksView from "./BookmarksView";
import GraphView from "./GraphView";
import MessagingConfigModal, {
  MessagingConfigInner,
} from "./MessagingConfigModal";

interface ConfigModalProps {
  key?: React.Key;
  onClose: () => void;
  graphData: GraphData;
  onUpdateGraphData: (data: GraphData) => void;
  bookmarks: BookmarkArtifact[];
  onPinArtifact: (artifact: BookmarkArtifact) => void;
  onUpdateBookmark: (updated: BookmarkArtifact) => void;
  token?: string;
}

type MainTab =
  | "config"
  | "graph"
  | "bookmarks"
  | "providers"
  | "messaging"
  | "security";

export default function ConfigModal({
  onClose,
  graphData,
  onUpdateGraphData,
  bookmarks,
  onPinArtifact,
  onUpdateBookmark,
  token,
}: ConfigModalProps) {
  const [mainTab, setMainTab] = useState<MainTab>("config");

  // Time and Milestone Filters
  const [startDate, setStartDate] = useState<string>("");
  const [endDate, setEndDate] = useState<string>("");
  const [selectedMilestone, setSelectedMilestone] = useState<string>("all");

  const milestones = Array.from(
    new Set(graphData.nodes.map((n) => n.milestone).filter(Boolean)),
  ) as string[];

  const filteredGraphData = useMemo(() => {
    let nodes = graphData.nodes;
    if (startDate) nodes = nodes.filter((n) => !n.date || n.date >= startDate);
    if (endDate) nodes = nodes.filter((n) => !n.date || n.date <= endDate);
    if (selectedMilestone !== "all")
      nodes = nodes.filter((n) => n.milestone === selectedMilestone);

    const nodeIds = new Set(nodes.map((n) => n.id));
    const links = graphData.links.filter(
      (l) => nodeIds.has(l.source) && nodeIds.has(l.target),
    );
    return { nodes, links };
  }, [graphData, startDate, endDate, selectedMilestone]);

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95, y: 10 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.95, y: 10 }}
      transition={{ duration: 0.4, ease: [0.22, 1, 0.36, 1] }}
      className="relative z-20 w-[1000px] h-[650px] max-w-[95vw] rounded-[32px] flex flex-col overflow-hidden shadow-2xl glass"
    >
      {/* Top Navigation */}
      <div className="flex items-center justify-between px-8 py-4 border-b border-white/5 bg-black/40">
        <div className="flex gap-6 overflow-x-auto">
          <TabButton
            active={mainTab === "config"}
            onClick={() => setMainTab("config")}
            icon={<SettingsIcon />}
            label="Configuration"
          />
          <TabButton
            active={mainTab === "providers"}
            onClick={() => setMainTab("providers")}
            icon={<Globe className="w-4 h-4" />}
            label="Providers"
          />
          <TabButton
            active={mainTab === "messaging"}
            onClick={() => setMainTab("messaging")}
            icon={<MessageSquare className="w-4 h-4" />}
            label="Messaging"
          />
          <TabButton
            active={mainTab === "security"}
            onClick={() => setMainTab("security")}
            icon={<Shield className="w-4 h-4" />}
            label="Security"
          />
          <TabButton
            active={mainTab === "graph"}
            onClick={() => setMainTab("graph")}
            icon={<Share2 className="w-4 h-4" />}
            label="Knowledge Graph"
          />
          <TabButton
            active={mainTab === "bookmarks"}
            onClick={() => setMainTab("bookmarks")}
            icon={<Bookmark className="w-4 h-4" />}
            label="Saved Artifacts"
          />
        </div>
        <button
          onClick={onClose}
          className="p-2 hover:bg-white/10 rounded-full transition-colors text-white/50 hover:text-white group"
          title="Salir"
        >
          <X className="w-5 h-5 group-hover:scale-110 transition-transform" />
        </button>
      </div>

      {/* Main Content Area */}
      <div className="flex-1 overflow-hidden relative bg-black/20">
        <AnimatePresence mode="wait">
          {mainTab === "config" && (
            <ConfigView key="config" graphData={graphData} />
          )}
          {mainTab === "graph" && (
            <motion.div
              key="graph"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="w-full h-full relative"
            >
              <div className="absolute bottom-6 left-1/2 -translate-x-1/2 z-30 flex gap-4 bg-[#0a0a0a]/90 backdrop-blur-md p-4 rounded-xl border border-white/10 shadow-2xl items-end">
                {(startDate || endDate || selectedMilestone !== "all") && (
                  <button
                    onClick={() => {
                      setStartDate("");
                      setEndDate("");
                      setSelectedMilestone("all");
                    }}
                    className="h-7 w-7 rounded-lg bg-red-500/20 text-red-400 hover:bg-red-500/30 transition-colors flex items-center justify-center border border-transparent shrink-0"
                    title="Clear Filters"
                  >
                    <X className="w-3.5 h-3.5" />
                  </button>
                )}
                <div className="flex flex-col">
                  <label className="text-[10px] uppercase text-white/50 tracking-widest mb-1">
                    Start Date
                  </label>
                  <input
                    type="date"
                    value={startDate}
                    onChange={(e) => setStartDate(e.target.value)}
                    className="h-7 bg-black/50 border border-white/10 rounded-lg px-2 py-1 text-xs text-white/90 outline-none focus:border-[#39ff14] transition-colors [color-scheme:dark]"
                  />
                </div>
                <div className="flex flex-col">
                  <label className="text-[10px] uppercase text-white/50 tracking-widest mb-1">
                    End Date
                  </label>
                  <input
                    type="date"
                    value={endDate}
                    onChange={(e) => setEndDate(e.target.value)}
                    className="h-7 bg-black/50 border border-white/10 rounded-lg px-2 py-1 text-xs text-white/90 outline-none focus:border-[#39ff14] transition-colors [color-scheme:dark]"
                  />
                </div>
                <div className="flex flex-col">
                  <label className="text-[10px] uppercase text-white/50 tracking-widest mb-1">
                    Milestone
                  </label>
                  <select
                    value={selectedMilestone}
                    onChange={(e) => setSelectedMilestone(e.target.value)}
                    className="h-7 bg-black/50 border border-white/10 rounded-lg px-2 py-1 text-xs text-white/90 outline-none focus:border-[#39ff14] transition-colors w-32 appearance-none"
                  >
                    <option value="all">All Milestones</option>
                    {milestones.map((m) => (
                      <option key={m} value={m}>
                        {m}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
              <GraphView
                data={filteredGraphData}
                onUpdateData={onUpdateGraphData}
              />
            </motion.div>
          )}
          {mainTab === "bookmarks" && (
            <BookmarksView
              key="bookmarks"
              bookmarks={bookmarks}
              onPinArtifact={onPinArtifact}
              onUpdateBookmark={onUpdateBookmark}
            />
          )}
          {mainTab === "providers" && (
            <div className="p-10 overflow-y-auto h-full">
              <ProvidersPage token={token || ""} />
            </div>
          )}
          {mainTab === "messaging" && (
            <motion.div
              key="messaging"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="w-full h-full overflow-y-auto"
            >
              {/* Embedded messaging config — no close button, no backdrop */}
              <div className="p-6 h-full flex flex-col">
                <div className="mb-4">
                  <h2 className="text-2xl font-light text-white tracking-tight">
                    Messaging Integrations
                  </h2>
                  <p className="text-sm text-white/40 mt-1">
                    Configure external communication channels for Xavier.
                  </p>
                </div>
                <div className="flex-1 bg-black/20 border border-white/[0.04] rounded-2xl overflow-hidden">
                  <MessagingEmbedded />
                </div>
              </div>
            </motion.div>
          )}
          {mainTab === "security" && (
            <motion.div
              key="security"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="w-full h-full overflow-y-auto"
            >
              <div className="p-6">
                <div className="mb-4">
                  <h2 className="text-2xl font-light text-white tracking-tight">
                    Security & Tokens
                  </h2>
                  <p className="text-sm text-white/40 mt-1">
                    Manage API tokens, provider keys, audit log, and network
                    settings.
                  </p>
                </div>
              </div>
              <SecurityConfigPanel embedded />
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </motion.div>
  );
}

function SettingsIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

/** Embedded version rendered inside ConfigModal (no backdrop/close button) */
function MessagingEmbedded() {
  return (
    <div className="w-full h-full">
      <MessagingConfigInner initialTab="telegram" />
    </div>
  );
}

function TabButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-2 pb-1 relative transition-colors duration-300 text-sm font-medium tracking-wide
        ${active ? "text-[#39ff14]" : "text-white/40 hover:text-white/80"}`}
    >
      {icon}
      {label}
      {active && (
        <motion.div
          layoutId="activeTopTab"
          className="absolute -bottom-[17px] left-0 right-0 h-[2px] bg-[#39ff14] shadow-[0_0_10px_#39ff14]"
        />
      )}
    </button>
  );
}

import {
  Bar,
  BarChart,
  CartesianGrid,
  Tooltip as RechartsTooltip,
  ResponsiveContainer,
  XAxis,
  YAxis,
} from "recharts";

function ConfigView({ graphData }: { graphData: GraphData }) {
  const [activeTab, setActiveTab] = useState("topology");

  const tabs = [
    {
      id: "server",
      label: "Server & Network",
      icon: <Server className="w-4 h-4" />,
    },
    {
      id: "limits",
      label: "Workspace Limits",
      icon: <Grid className="w-4 h-4" />,
    },
    {
      id: "memory",
      label: "Memory & Layers",
      icon: <Brain className="w-4 h-4" />,
    },
    {
      id: "topology",
      label: "Topology Stats",
      icon: <Network className="w-4 h-4" />,
    },
    {
      id: "models",
      label: "AI Models & Routing",
      icon: <CpuIcon className="w-4 h-4" />,
    },
    {
      id: "embedding",
      label: "Embedding & Cache",
      icon: <Database className="w-4 h-4" />,
    },
    {
      id: "advanced",
      label: "Advanced & Security",
      icon: <Shield className="w-4 h-4" />,
    },
    {
      id: "integrations",
      label: "Integrations (PgHeart/TG)",
      icon: <Plug className="w-4 h-4" />,
    },
  ];

  const topologyData = useMemo(() => {
    if (!graphData) return [];
    const childrenMap: Record<string, string[]> = {};
    const incomingCount: Record<string, number> = {};

    graphData.nodes.forEach((n) => (incomingCount[n.id] = 0));

    graphData.links.forEach((l) => {
      const source =
        typeof l.source === "object" ? (l.source as any).id : l.source;
      const target =
        typeof l.target === "object" ? (l.target as any).id : l.target;
      if (!childrenMap[source]) childrenMap[source] = [];
      childrenMap[source].push(target);
      incomingCount[target] = (incomingCount[target] || 0) + 1;
    });

    const depthCount: Record<number, number> = {};
    const roots = graphData.nodes.filter((n) => incomingCount[n.id] === 0);

    const queue: { id: string; depth: number }[] = roots.map((r) => ({
      id: r.id,
      depth: 1,
    }));
    const visited = new Set<string>();

    while (queue.length > 0) {
      const { id, depth } = queue.shift()!;
      if (visited.has(id)) continue;
      visited.add(id);

      depthCount[depth] = (depthCount[depth] || 0) + 1;

      const children = childrenMap[id] || [];
      for (const child of children) {
        queue.push({ id: child, depth: depth + 1 });
      }
    }

    graphData.nodes.forEach((n) => {
      if (!visited.has(n.id)) {
        depthCount[0] = (depthCount[0] || 0) + 1; // Unconnected or cyclical
      }
    });

    return Object.entries(depthCount).map(([depth, count]) => ({
      depth: depth === "0" ? "Cyclic/Isolated" : `Level ${depth}`,
      count,
    }));
  }, [graphData]);

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="flex h-full w-full"
    >
      {/* Sidebar */}
      <div className="w-64 border-r border-white/5 p-6 flex flex-col bg-black/10 overflow-y-auto">
        <nav className="flex flex-col gap-2">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-3 px-4 py-3 transition-all duration-300 text-sm font-medium rounded-lg
                ${
                  activeTab === tab.id
                    ? "active-tab text-[#39ff14]"
                    : "text-white/40 hover:text-white/80 hover:bg-white/5"
                }`}
            >
              {tab.icon}
              {tab.label}
            </button>
          ))}
        </nav>
      </div>

      {/* Content Area */}
      <div className="flex-1 p-10 overflow-y-auto">
        <AnimatePresence mode="wait">
          {activeTab === "topology" && (
            <motion.div
              key="topology"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="flex flex-col gap-8 max-w-2xl h-full"
            >
              <div>
                <h2 className="text-3xl font-light text-white tracking-tight">
                  Topology Stats
                </h2>
                <p className="text-sm text-white/40 mt-1">
                  Distribution of nodes across different depth levels.
                </p>
              </div>

              <div className="flex-1 min-h-[300px] w-full mt-4 bg-[#050505]/50 border border-[#0d2a13]/80 rounded-xl p-6">
                {topologyData.length > 0 ? (
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart
                      data={topologyData}
                      margin={{ top: 20, right: 30, left: 0, bottom: 20 }}
                    >
                      <CartesianGrid
                        strokeDasharray="3 3"
                        stroke="#1a1a1a"
                        vertical={false}
                      />
                      <XAxis
                        dataKey="depth"
                        stroke="#ffffff40"
                        fontSize={12}
                        tickLine={false}
                        axisLine={false}
                        dy={10}
                      />
                      <YAxis
                        stroke="#ffffff40"
                        fontSize={12}
                        tickLine={false}
                        axisLine={false}
                        dx={-10}
                      />
                      <RechartsTooltip
                        cursor={{ fill: "#39ff14", opacity: 0.05 }}
                        contentStyle={{
                          backgroundColor: "#0a0a0a",
                          border: "1px solid #333",
                          borderRadius: "8px",
                          color: "#fff",
                        }}
                      />
                      <Bar
                        dataKey="count"
                        fill="#39ff14"
                        radius={[4, 4, 0, 0]}
                        barSize={40}
                        activeBar={{ stroke: "#fff", strokeWidth: 1 }}
                      />
                    </BarChart>
                  </ResponsiveContainer>
                ) : (
                  <div className="w-full h-full flex items-center justify-center text-white/30 text-sm">
                    No graph data available.
                  </div>
                )}
              </div>
            </motion.div>
          )}

          {activeTab === "memory" && (
            <motion.div
              key="memory"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="flex flex-col gap-8 max-w-2xl"
            >
              <div>
                <h2 className="text-3xl font-light text-white tracking-tight">
                  Memory Management
                </h2>
                <p className="text-sm text-white/40 mt-1">
                  Configure vector stores and working memory parameters.
                </p>
              </div>

              <div className="space-y-6">
                <SelectInput
                  label="Backend Engine"
                  options={["vec", "sqlite", "file"]}
                  defaultValue="vec"
                />
                <TextInput
                  label="Embedding Dimensions"
                  type="number"
                  defaultValue="384"
                />
                <ToggleRow
                  label="Enable Working Memory LRU"
                  description="Automatically prune least recently used context layers"
                  defaultChecked={true}
                />
                <SliderInput
                  label="BM25 K1 Parameter"
                  min={0}
                  max={1}
                  step={0.1}
                  defaultValue={0.5}
                />
                <SliderInput
                  label="BM25 B Parameter"
                  min={0}
                  max={1}
                  step={0.1}
                  defaultValue={0.7}
                />
                <SliderInput
                  label="Episodic Summary Window (Days)"
                  min={1}
                  max={30}
                  step={1}
                  defaultValue={7}
                  format={(v) => `${v}d`}
                />
                <SliderInput
                  label="Minimum Event Importance"
                  min={0}
                  max={1}
                  step={0.05}
                  defaultValue={0.4}
                />
              </div>
            </motion.div>
          )}

          {activeTab === "advanced" && (
            <motion.div
              key="advanced"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="flex flex-col gap-8 max-w-2xl"
            >
              <div>
                <h2 className="text-3xl font-light text-white tracking-tight">
                  Advanced System Settings
                </h2>
                <p className="text-sm text-white/40 mt-1">
                  Core system overrides and security policies.
                </p>
              </div>
              <div className="space-y-6">
                <ToggleRow
                  label="Enable Entity Extraction"
                  description="Run NLP pipelines for automated entity linking"
                  defaultChecked={true}
                />
                <ToggleRow
                  label="Enable Audit Chain"
                  description="Log all LLM traces immutably"
                  defaultChecked={false}
                />
                <TextInput
                  label="Token Secret"
                  type="password"
                  defaultValue="****************"
                />
                <TextInput
                  label="Allowed CORS Domains"
                  type="text"
                  defaultValue="https://app.neural.local, http://localhost:3000"
                />
                <SliderInput
                  label="QJL Threshold"
                  min={0}
                  max={100}
                  step={1}
                  defaultValue={85}
                  format={(v) => `${v}%`}
                />
              </div>
            </motion.div>
          )}

          {activeTab === "models" && (
            <motion.div
              key="models"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="flex flex-col gap-8 max-w-2xl"
            >
              <div>
                <h2 className="text-3xl font-light text-white tracking-tight">
                  AI Models & Routing
                </h2>
                <p className="text-sm text-white/40 mt-1">
                  Configure generation pipelines and intelligent routing.
                </p>
              </div>
              <div className="space-y-6">
                <SelectInput
                  label="Primary Provider"
                  options={["local", "cloud", "openai", "anthropic", "gemini"]}
                  defaultValue="gemini"
                />
                <TextInput
                  label="Local LLM URL"
                  type="text"
                  defaultValue="http://127.0.0.1:11434"
                />
                <TextInput
                  label="Local LLM Model"
                  type="text"
                  defaultValue="llama3-8b"
                />
                <TextInput
                  label="Router Retrieved Model"
                  type="text"
                  defaultValue="gemini-1.5-flash"
                />
                <TextInput
                  label="Router Complex Model"
                  type="text"
                  defaultValue="gemini-1.5-pro"
                />
              </div>
            </motion.div>
          )}

          {!["memory", "advanced", "models", "topology"].includes(
            activeTab,
          ) && (
            <motion.div
              key="placeholder"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              className="flex flex-col items-center justify-center h-64 text-white/30"
            >
              <Layers className="w-12 h-12 mb-4 opacity-20" />
              <p>Select a category to view specific configuration layers</p>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </motion.div>
  );
}

// ------ Reusable UI Components ------

function ToggleRow({
  label,
  description,
  defaultChecked,
}: {
  label: string;
  description: string;
  defaultChecked: boolean;
}) {
  const [checked, setChecked] = useState(defaultChecked);
  return (
    <div className="flex items-center justify-between p-4 rounded-xl bg-[#050505]/50 border border-[#0d2a13]/80 hover:border-[#1a4a23] transition-colors">
      <div>
        <h4 className="text-white/90 text-sm font-medium mb-1">{label}</h4>
        <p className="text-xs text-white/40">{description}</p>
      </div>
      <button
        onClick={() => setChecked(!checked)}
        className={`relative w-12 h-7 rounded-full transition-all duration-300 ${checked ? "bg-[#39ff14] shadow-[0_0_15px_rgba(57,255,20,0.4)]" : "bg-white/10"}`}
      >
        <div
          className={`absolute top-1 left-1 w-5 h-5 rounded-full bg-white transition-transform duration-300 ${checked ? "translate-x-5" : "translate-x-0 opacity-60"}`}
        />
      </button>
    </div>
  );
}

function SliderInput({
  label,
  min,
  max,
  step = 1,
  defaultValue,
  format,
}: {
  label: string;
  min: number;
  max: number;
  step?: number;
  defaultValue: number;
  format?: (val: number) => string;
}) {
  const [value, setValue] = useState(defaultValue);
  const percentage = ((value - min) / (max - min)) * 100;

  return (
    <div className="p-4 rounded-xl bg-[#050505]/50 border border-[#0d2a13]/80">
      <div className="flex items-center justify-between mb-4">
        <label className="text-[10px] uppercase text-white/50 tracking-widest">
          {label}
        </label>
        <span className="text-[#39ff14] text-xs font-mono neon-text">
          {format ? format(value) : value.toFixed(step < 1 ? 2 : 0)}
        </span>
      </div>
      <div className="relative pt-1">
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => setValue(Number(e.target.value))}
          className="w-full h-1 bg-[#1a1a1a] rounded-lg appearance-none cursor-pointer absolute top-1/2 -translate-y-1/2 z-10 opacity-0 w-full"
        />
        {/* Custom Track */}
        <div className="w-full h-1 bg-[#1a1a1a] rounded-lg relative overflow-visible">
          <div
            className="h-full bg-[#39ff14] rounded-lg shadow-[0_0_10px_#39ff14]"
            style={{ width: `${percentage}%` }}
          />
          {/* Thumb */}
          <div
            className="w-4 h-4 bg-white rounded-full absolute top-1/2 -translate-y-1/2 shadow-[0_0_15px_rgba(57,255,20,0.8)] border-2 border-[#39ff14] pointer-events-none"
            style={{ left: `calc(${percentage}% - 8px)` }}
          />
        </div>
      </div>
    </div>
  );
}

function TextInput({
  label,
  type = "text",
  defaultValue,
}: {
  label: string;
  type?: string;
  defaultValue: string;
}) {
  return (
    <div>
      <label className="text-[10px] uppercase text-white/50 tracking-widest mb-2 block">
        {label}
      </label>
      <input
        type={type}
        defaultValue={defaultValue}
        className="w-full bg-[#050505]/80 border border-[#1a1a1a] focus:border-[#39ff14] focus:shadow-[0_0_15px_rgba(57,255,20,0.2)] text-white/90 text-sm px-4 py-3 rounded-xl outline-none transition-all"
      />
    </div>
  );
}

function SelectInput({
  label,
  options,
  defaultValue,
}: {
  label: string;
  options: string[];
  defaultValue: string;
}) {
  return (
    <div>
      <label className="text-[10px] uppercase text-white/50 tracking-widest mb-2 block">
        {label}
      </label>
      <div className="relative">
        <select
          defaultValue={defaultValue}
          className="w-full bg-[#050505]/80 border border-[#1a1a1a] focus:border-[#39ff14] focus:shadow-[0_0_15px_rgba(57,255,20,0.2)] text-white/90 text-sm px-4 py-3 rounded-xl outline-none transition-all appearance-none cursor-pointer"
        >
          {options.map((opt) => (
            <option key={opt} value={opt} className="bg-[#050505]">
              {opt}
            </option>
          ))}
        </select>
        <div className="absolute right-4 top-1/2 -translate-y-1/2 pointer-events-none text-white/40">
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
          >
            <path d="M6 9l6 6 6-6" />
          </svg>
        </div>
      </div>
    </div>
  );
}
