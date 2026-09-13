import {
  ArrowLeft,
  Bookmark,
  Bot,
  Brain,
  ChevronRight,
  Cpu,
  Database,
  Globe,
  Grid,
  Layers,
  MessageSquare,
  Network,
  Palette,
  Play,
  Plug,
  Puzzle,
  RefreshCw,
  Server,
  Share2,
  Shield,
  Sliders,
  TrendingUp,
  X,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import type React from "react";
import { useCallback, useEffect, useId, useMemo, useState } from "react";
import { ApiClient } from "../api/client";
import {
  canvasToForceData,
  codeViewToCanvas,
  memoryViewToCanvas,
} from "../api/graphAdapters";
import AppearancePage from "../pages/Settings/Appearance";
import ProvidersPage from "../pages/Settings/Providers";
import SecurityConfigPanel from "../pages/Settings/Security";
import type { BookmarkArtifact, GraphData, GraphNode } from "../types";
import { mergeFilteredGraphUpdate } from "../utils/roadmapGraph";
import AgentsView from "./AgentsView";
import BookmarksView from "./BookmarksView";
import { CloudRelayConfig } from "./CloudRelayConfig";
import DataCommonsConfigUI from "./DataCommonsConfigUI";
import GraphCanvas from "./GraphCanvas";
import GraphView from "./GraphView";
import MemoryBrowser from "./MemoryBrowser";
import MeshConfig from "./MeshConfig";
import { MessagingConfigInner } from "./MessagingConfigModal";
import { PluginsManager } from "./PluginsManager";
import UsageMetricsPanel from "./UsageMetricsPanel";

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

export type SidebarTab =
  | "general"
  | "appearance"
  | "models"
  | "security"
  | "mesh"
  | "memory"
  | "agents"
  | "roadmap"
  | "bookmarks"
  | "providers"
  | "usage"
  | "messaging"
  | "plugins";

type SubLayer = "roadmap" | "memory" | "code";

export default function ConfigModal({
  onClose,
  graphData,
  onUpdateGraphData,
  bookmarks,
  onPinArtifact,
  onUpdateBookmark,
  token,
}: ConfigModalProps) {
  const [activeTab, setActiveTab] = useState<SidebarTab>("general");
  const [subLayer, setSubLayer] = useState<SubLayer>("roadmap");

  const api = useMemo(() => new ApiClient(token || ""), [token]);

  // Time and Milestone Filters
  const [startDate, setStartDate] = useState<string>("");
  const [endDate, setEndDate] = useState<string>("");
  const [selectedMilestone, setSelectedMilestone] = useState<string>("all");

  const milestones = useMemo(() => {
    return Array.from(
      graphData.nodes.reduce((acc, n) => {
        if (n.milestone) acc.add(n.milestone);
        return acc;
      }, new Set<string>())
    );
  }, [graphData.nodes]);

  const filteredGraphData = useMemo(() => {
    let nodes = graphData.nodes;
    if (startDate) nodes = nodes.filter((n) => !n.date || n.date >= startDate);
    if (endDate) nodes = nodes.filter((n) => !n.date || n.date <= endDate);
    if (selectedMilestone !== "all")
      nodes = nodes.filter((n) => n.milestone === selectedMilestone);

    const nodeIds = new Set(nodes.map((n) => n.id));
    const links = graphData.links.filter(
      (l) => nodeIds.has(String(l.source)) && nodeIds.has(String(l.target)),
    );
    return { nodes, links };
  }, [graphData, startDate, endDate, selectedMilestone]);

  const handleFilteredGraphUpdate = useCallback(
    (updated: GraphData) => {
      const visibleIds = new Set(filteredGraphData.nodes.map((n) => n.id));
      onUpdateGraphData(
        mergeFilteredGraphUpdate(graphData, visibleIds, updated),
      );
    },
    [filteredGraphData.nodes, graphData, onUpdateGraphData],
  );

  // ─── Memory KG State ───
  const [memoryData, setMemoryData] = useState<GraphData>({
    nodes: [],
    links: [],
  });
  const [memoryLoading, setMemoryLoading] = useState(false);
  const [memoryError, setMemoryError] = useState<string | null>(null);
  const [isMemoryTruncated, setIsMemoryTruncated] = useState(false);

  const fetchMemoryGraph = useCallback(async () => {
    setMemoryLoading(true);
    setMemoryError(null);
    try {
      const isTauri =
        typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
      const baseUrl = isTauri ? "http://127.0.0.1:8006" : "";
      const activeWorkspace =
        typeof localStorage !== "undefined"
          ? localStorage.getItem("xavier_active_workspace") || "default"
          : "default";
      const res = await fetch(`${baseUrl}/memory/graph/view`, {
        headers: {
          "Content-Type": "application/json",
          "X-Xavier-Token": token || "",
          "X-Workspace-Id": activeWorkspace,
        },
      });
      if (!res.ok) throw new Error(await res.text());
      const body = await res.json();
      const canvas = memoryViewToCanvas(body);
      setMemoryData(canvasToForceData(canvas));
      if (body?.truncated || body?.is_truncated || canvas.truncated) {
        setIsMemoryTruncated(true);
      } else {
        setIsMemoryTruncated(false);
      }
    } catch (e: any) {
      setMemoryError(e.message || "Failed to load memory graph");
    } finally {
      setMemoryLoading(false);
    }
  }, [token]);

  const fetchMemoryNodeDetail = useCallback(
    async (node: GraphNode) => {
      try {
        const isTauri =
          typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
        const baseUrl = isTauri ? "http://127.0.0.1:8006" : "";
        const activeWorkspace =
          typeof localStorage !== "undefined"
            ? localStorage.getItem("xavier_active_workspace") || "default"
            : "default";
        const res = await fetch(
          `${baseUrl}/memory/graph/entities/${encodeURIComponent(node.id)}`,
          {
            headers: {
              "Content-Type": "application/json",
              "X-Xavier-Token": token || "",
              "X-Workspace-Id": activeWorkspace,
            },
          },
        );
        if (res.ok) {
          return await res.json();
        }
      } catch (e) {
        console.warn("Could not load entity details", e);
      }
      return null;
    },
    [token],
  );

  // ─── Code State ───
  const [codeStats, setCodeStats] = useState<{
    total_symbols: number;
    total_files: number;
  } | null>(null);
  const [codeData, setCodeData] = useState<GraphData>({ nodes: [], links: [] });
  const [codeLoading, setCodeLoading] = useState(false);
  const [codeError, setCodeError] = useState<string | null>(null);
  const [codeEgoQuery, setCodeEgoQuery] = useState<string | null>(null);

  const fetchCodeStatsAndGraph = useCallback(
    async (query: string | null = null) => {
      setCodeLoading(true);
      setCodeError(null);
      try {
        const isTauri =
          typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
        const baseUrl = isTauri ? "http://127.0.0.1:8006" : "";

        const activeWorkspace =
          typeof localStorage !== "undefined"
            ? localStorage.getItem("xavier_active_workspace") || "default"
            : "default";
        const statsRes = await fetch(`${baseUrl}/code/stats`, {
          headers: {
            "Content-Type": "application/json",
            "X-Xavier-Token": token || "",
            "X-Workspace-Id": activeWorkspace,
          },
        });
        if (!statsRes.ok) throw new Error("Failed to load code statistics");
        const stats = await statsRes.json();
        setCodeStats(stats);

        if (stats.total_symbols > 0 || stats.total_files > 0) {
          const mode = query ? "ego" : "overview";
          let url = `${baseUrl}/code/graph/view?mode=${mode}`;
          if (query) {
            url += `&query=${encodeURIComponent(query)}`;
          }
          const graphRes = await fetch(url, {
            headers: {
              "Content-Type": "application/json",
              "X-Xavier-Token": token || "",
              "X-Workspace-Id": activeWorkspace,
            },
          });
          if (!graphRes.ok) throw new Error("Failed to load code graph view");
          const graphBody = await graphRes.json();
          setCodeData(canvasToForceData(codeViewToCanvas(graphBody)));
        } else {
          setCodeData({ nodes: [], links: [] });
        }
      } catch (e: any) {
        setCodeError(e.message || "Failed to load code view");
      } finally {
        setCodeLoading(false);
      }
    },
    [token],
  );

  const handleScanCodebase = async () => {
    setCodeLoading(true);
    setCodeError(null);
    try {
      const isTauri =
        typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
      const baseUrl = isTauri ? "http://127.0.0.1:8006" : "";
      const activeWorkspace =
        typeof localStorage !== "undefined"
          ? localStorage.getItem("xavier_active_workspace") || "default"
          : "default";
      const res = await fetch(`${baseUrl}/code/scan`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Xavier-Token": token || "",
          "X-Workspace-Id": activeWorkspace,
        },
        body: JSON.stringify({ path: "src" }),
      });
      if (!res.ok) throw new Error(await res.text());
      await fetchCodeStatsAndGraph();
    } catch (e: any) {
      setCodeError(e.message || "Codebase scan failed");
    } finally {
      setCodeLoading(false);
    }
  };

  const handleNodeExpand = (node: GraphNode) => {
    setCodeEgoQuery(node.id);
    void fetchCodeStatsAndGraph(node.id);
  };

  const handleResetCodeOverview = () => {
    setCodeEgoQuery(null);
    void fetchCodeStatsAndGraph(null);
  };

  useEffect(() => {
    if (activeTab === "roadmap") {
      if (subLayer === "memory") {
        void fetchMemoryGraph();
      } else if (subLayer === "code") {
        void fetchCodeStatsAndGraph(codeEgoQuery);
      }
    }

    const handleWorkspaceChange = () => {
      if (activeTab === "roadmap") {
        if (subLayer === "memory") {
          void fetchMemoryGraph();
        } else if (subLayer === "code") {
          void fetchCodeStatsAndGraph(codeEgoQuery);
        }
      }
    };

    window.addEventListener("xavier:workspace-changed", handleWorkspaceChange);
    return () => {
      window.removeEventListener("xavier:workspace-changed", handleWorkspaceChange);
    };
  }, [
    activeTab,
    subLayer,
    fetchMemoryGraph,
    fetchCodeStatsAndGraph,
    codeEgoQuery,
  ]);

  const sidebarItems: { id: SidebarTab; label: string; icon: React.ReactNode }[] = [
    { id: "general", label: "General & Configuration", icon: <Sliders className="w-4 h-4" /> },
    { id: "appearance", label: "Appearance", icon: <Palette className="w-4 h-4" /> },
    { id: "models", label: "Models & Routing", icon: <Cpu className="w-4 h-4" /> },
    { id: "security", label: "Security & Tokens", icon: <Shield className="w-4 h-4" /> },
    { id: "mesh", label: "Mesh Network", icon: <Network className="w-4 h-4" /> },
    { id: "memory", label: "Memory Browser", icon: <Brain className="w-4 h-4" /> },
    { id: "agents", label: "Agents", icon: <Bot className="w-4 h-4" /> },
    { id: "roadmap", label: "Roadmap", icon: <Share2 className="w-4 h-4" /> },
    { id: "bookmarks", label: "Bookmarks / Saved Artifacts", icon: <Bookmark className="w-4 h-4" /> },
    { id: "providers", label: "Providers", icon: <Globe className="w-4 h-4" /> },
    { id: "usage", label: "Usage Metrics", icon: <TrendingUp className="w-4 h-4" /> },
    { id: "messaging", label: "Messaging", icon: <MessageSquare className="w-4 h-4" /> },
    { id: "plugins", label: "Plugins", icon: <Puzzle className="w-4 h-4" /> },
  ];

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95, y: 10 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.95, y: 10 }}
      transition={{ duration: 0.4, ease: [0.22, 1, 0.36, 1] }}
      className="relative z-20 w-[1080px] h-[680px] max-w-[95vw] rounded-[32px] flex flex-col overflow-hidden shadow-2xl glass"
    >
      {/* Header Bar */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-white/5 bg-black/40">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-xl bg-[#39ff14]/10 border border-[#39ff14]/30 flex items-center justify-center">
            <Sliders className="w-4 h-4 text-[#39ff14]" />
          </div>
          <div>
            <h2 className="text-sm font-bold text-white uppercase tracking-wider font-mono">
              Xavier Control Node Settings
            </h2>
            <p className="text-[10px] text-white/40">
              System preferences, models, security, and knowledge graph tools
            </p>
          </div>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="p-2 hover:bg-white/10 rounded-full transition-colors text-white/50 hover:text-white group cursor-pointer"
          title="Salir"
          aria-label="Cerrar ventana de configuración"
        >
          <X className="w-5 h-5 group-hover:scale-110 transition-transform" />
        </button>
      </div>

      {/* Unified Antigravity Sidebar & Content Layout */}
      <div className="flex-1 flex overflow-hidden bg-black/20">
        {/* Antigravity Left Sidebar */}
        <div className="w-64 border-r border-white/5 p-4 flex flex-col bg-black/30 overflow-y-auto shrink-0">
          <nav className="flex flex-col gap-1.5">
            {sidebarItems.map((item) => {
              const isActive = activeTab === item.id;
              return (
                <button
                  type="button"
                  key={item.id}
                  onClick={() => setActiveTab(item.id)}
                  className={`flex items-center gap-3 px-3.5 py-2.5 rounded-xl transition-all duration-200 text-xs font-medium cursor-pointer ${
                    isActive
                      ? "bg-[#39ff14]/15 border border-[#39ff14]/40 text-[#39ff14] shadow-[0_0_12px_rgba(57,255,20,0.15)] font-bold"
                      : "text-white/60 hover:text-white hover:bg-white/5 border border-transparent"
                  }`}
                >
                  <span className={isActive ? "text-[#39ff14]" : "text-white/40"}>
                    {item.icon}
                  </span>
                  <span className="truncate">{item.label}</span>
                </button>
              );
            })}
          </nav>
        </div>

        {/* Content View Area */}
        <div className="flex-1 overflow-hidden relative">
          <AnimatePresence mode="wait">
            {activeTab === "general" && (
              <ConfigView key="general" graphData={graphData} token={token || ""} />
            )}
            {activeTab === "appearance" && (
              <motion.div
                key="appearance"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="w-full h-full overflow-y-auto p-6"
              >
                <AppearancePage />
              </motion.div>
            )}
            {activeTab === "models" && (
              <motion.div
                key="models"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="w-full h-full overflow-y-auto p-8"
              >
                <div className="mb-6">
                  <h2 className="text-2xl font-light text-white tracking-tight">
                    AI Models & Intelligent Routing
                  </h2>
                  <p className="text-sm text-white/40 mt-1">
                    Configure local LLMs, cloud models, and heuristic routing profiles.
                  </p>
                </div>
                <div className="space-y-6 max-w-2xl">
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
                    label="Router Fast Model"
                    type="text"
                    defaultValue="gemini-1.5-flash"
                  />
                  <TextInput
                    label="Router Complex Reasoning Model"
                    type="text"
                    defaultValue="gemini-1.5-pro"
                  />
                </div>
              </motion.div>
            )}
            {activeTab === "roadmap" && (
              <motion.div
                key="roadmap"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="w-full h-full relative flex flex-col"
              >
                <div className="flex items-center justify-between px-8 py-3 bg-[#0a0a0a]/80 border-b border-white/5 shrink-0 z-40">
                  <div
                    role="tablist"
                    aria-label="Knowledge layers"
                    className="flex bg-white/5 p-1 rounded-xl gap-1 border border-white/5"
                  >
                    <button
                      role="tab"
                      id="tab-sub-roadmap"
                      aria-controls="panel-sub-roadmap"
                      aria-selected={subLayer === "roadmap"}
                      tabIndex={subLayer === "roadmap" ? 0 : -1}
                      onClick={() => setSubLayer("roadmap")}
                      className={`px-4 py-1.5 rounded-lg text-xs font-semibold tracking-wide transition-all ${
                        subLayer === "roadmap"
                          ? "bg-[#39ff14] text-black shadow-[0_0_15px_rgba(57,255,20,0.3)] font-bold"
                          : "text-white/60 hover:text-white hover:bg-white/5"
                      }`}
                    >
                      Roadmap
                    </button>
                    <button
                      role="tab"
                      id="tab-sub-memory"
                      aria-controls="panel-sub-memory"
                      aria-selected={subLayer === "memory"}
                      tabIndex={subLayer === "memory" ? 0 : -1}
                      onClick={() => setSubLayer("memory")}
                      className={`px-4 py-1.5 rounded-lg text-xs font-semibold tracking-wide transition-all ${
                        subLayer === "memory"
                          ? "bg-[#39ff14] text-black shadow-[0_0_15px_rgba(57,255,20,0.3)] font-bold"
                          : "text-white/60 hover:text-white hover:bg-white/5"
                      }`}
                    >
                      Memory KG
                    </button>
                    <button
                      role="tab"
                      id="tab-sub-code"
                      aria-controls="panel-sub-code"
                      aria-selected={subLayer === "code"}
                      tabIndex={subLayer === "code" ? 0 : -1}
                      onClick={() => setSubLayer("code")}
                      className={`px-4 py-1.5 rounded-lg text-xs font-semibold tracking-wide transition-all ${
                        subLayer === "code"
                          ? "bg-[#39ff14] text-black shadow-[0_0_15px_rgba(57,255,20,0.3)] font-bold"
                          : "text-white/60 hover:text-white hover:bg-white/5"
                      }`}
                    >
                      Code
                    </button>
                  </div>

                  {subLayer === "code" && codeEgoQuery && (
                    <button
                      type="button"
                      onClick={handleResetCodeOverview}
                      className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-white/5 border border-white/10 hover:bg-white/10 text-white/80 hover:text-white text-xs font-medium tracking-wide transition-colors cursor-pointer"
                    >
                      <ArrowLeft className="w-3.5 h-3.5" />
                      Reset to Overview
                    </button>
                  )}
                </div>

                <div className="flex-1 min-h-0 relative">
                  {subLayer === "roadmap" && (
                    <div
                      role="tabpanel"
                      id="panel-sub-roadmap"
                      aria-labelledby="tab-sub-roadmap"
                      className="w-full h-full relative"
                    >
                      <div className="absolute bottom-6 left-1/2 -translate-x-1/2 z-30 flex gap-4 bg-[#0a0a0a]/90 backdrop-blur-md p-4 rounded-xl border border-white/10 shadow-2xl items-end">
                        {(startDate || endDate || selectedMilestone !== "all") && (
                          <button
                            type="button"
                            onClick={() => {
                              setStartDate("");
                              setEndDate("");
                              setSelectedMilestone("all");
                            }}
                            className="h-7 w-7 rounded-lg bg-red-500/20 text-red-400 hover:bg-red-500/30 transition-colors flex items-center justify-center border border-transparent shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]/50"
                            title="Clear Filters"
                            aria-label="Clear roadmap filters"
                          >
                            <X className="w-3.5 h-3.5" aria-hidden="true" />
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
                        onUpdateData={handleFilteredGraphUpdate}
                        isFullGraphEmpty={graphData.nodes.length === 0}
                      />
                    </div>
                  )}

                  {subLayer === "memory" && (
                    <div
                      role="tabpanel"
                      id="panel-sub-memory"
                      aria-labelledby="tab-sub-memory"
                      className="w-full h-full relative"
                    >
                      <GraphCanvas
                        data={memoryData}
                        loading={memoryLoading}
                        error={memoryError}
                        emptyMessage="No entities yet — add memories to begin."
                        isTruncated={isMemoryTruncated}
                        onNodeSelect={fetchMemoryNodeDetail}
                      />
                    </div>
                  )}

                  {subLayer === "code" && (
                    <div
                      role="tabpanel"
                      id="panel-sub-code"
                      aria-labelledby="tab-sub-code"
                      className="w-full h-full relative"
                    >
                      {codeStats && codeStats.total_symbols === 0 && !codeLoading ? (
                        <div className="absolute inset-0 z-30 flex items-center justify-center bg-[#050505]/80 p-6">
                          <div className="flex flex-col items-center max-w-sm text-center bg-[#0a0a0a] border border-white/10 rounded-[24px] p-8 shadow-2xl">
                            <Cpu className="w-12 h-12 text-[#39ff14] mb-4 animate-pulse" />
                            <h3 className="text-lg font-light text-white mb-2 tracking-tight">
                              Scan Codebase
                            </h3>
                            <p className="text-xs text-white/40 leading-relaxed mb-6">
                              Index symbols, classes, structs, and calls within your "src/" directory to navigate relationships.
                            </p>
                            <button
                              type="button"
                              onClick={handleScanCodebase}
                              className="flex items-center justify-center gap-2 px-6 py-2.5 bg-[#39ff14] text-black font-bold tracking-wider uppercase text-xs rounded-xl hover:shadow-[0_0_20px_rgba(57,255,20,0.5)] focus:outline-none transition-all duration-300 cursor-pointer"
                            >
                              <Play className="w-4 h-4 fill-current" />
                              Scan Now
                            </button>
                          </div>
                        </div>
                      ) : (
                        <GraphCanvas
                          data={codeData}
                          loading={codeLoading}
                          error={codeError}
                          emptyMessage="No code graph overview available. Click scan if necessary."
                          onNodeDoubleClick={handleNodeExpand}
                          onNodeExpand={handleNodeExpand}
                          isCodeMode={true}
                        />
                      )}
                    </div>
                  )}
                </div>
              </motion.div>
            )}
            {activeTab === "bookmarks" && (
              <BookmarksView
                key="bookmarks"
                bookmarks={bookmarks}
                onPinArtifact={onPinArtifact}
                onUpdateBookmark={onUpdateBookmark}
              />
            )}
            {activeTab === "providers" && (
              <div className="p-10 overflow-y-auto h-full">
                <ProvidersPage token={token || ""} />
              </div>
            )}
            {activeTab === "usage" && (
              <motion.div
                key="usage"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="w-full h-full overflow-hidden"
              >
                <UsageMetricsPanel token={token || ""} />
              </motion.div>
            )}
            {activeTab === "messaging" && (
              <motion.div
                key="messaging"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="w-full h-full overflow-y-auto p-6 flex flex-col"
              >
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
              </motion.div>
            )}
            {activeTab === "security" && (
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
                      Manage API tokens, provider keys, audit log, and network settings.
                    </p>
                  </div>
                </div>
                <SecurityConfigPanel embedded token={token || ""} />
              </motion.div>
            )}
            {activeTab === "mesh" && (
              <motion.div
                key="mesh"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="w-full h-full overflow-hidden"
              >
                <MeshConfig token={token || ""} />
              </motion.div>
            )}
            {activeTab === "memory" && (
              <motion.div
                key="memory"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="w-full h-full overflow-hidden"
              >
                <MemoryBrowser token={token || ""} />
              </motion.div>
            )}
            {activeTab === "agents" && (
              <motion.div
                key="agents"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="w-full h-full overflow-y-auto p-8"
              >
                <AgentsView token={token || ""} />
              </motion.div>
            )}
            {activeTab === "plugins" && (
              <motion.div
                key="plugins"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                className="w-full h-full overflow-hidden"
              >
                <PluginsManager token={token || ""} />
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>
    </motion.div>
  );
}

function MessagingEmbedded() {
  return (
    <div className="w-full h-full">
      <MessagingConfigInner initialTab="telegram" />
    </div>
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

function ConfigView({
  graphData,
  token,
}: {
  graphData: GraphData;
  token: string;
}) {
  const [activeTab, setActiveTab] = useState("topology");

  const tabs = [
    { id: "server", label: "Server & Network", icon: <Server className="w-4 h-4" /> },
    { id: "limits", label: "Workspace Limits", icon: <Grid className="w-4 h-4" /> },
    { id: "memory", label: "Memory & Layers", icon: <Brain className="w-4 h-4" /> },
    { id: "topology", label: "Topology Stats", icon: <Network className="w-4 h-4" /> },
    { id: "models", label: "AI Models & Routing", icon: <Cpu className="w-4 h-4" /> },
    { id: "embedding", label: "Embedding & Cache", icon: <Database className="w-4 h-4" /> },
    { id: "advanced", label: "Advanced & Security", icon: <Shield className="w-4 h-4" /> },
    { id: "integrations", label: "Integrations (PgHeart/TG)", icon: <Plug className="w-4 h-4" /> },
  ];

  const topologyData = useMemo(() => {
    if (!graphData) return [];
    const childrenMap: Record<string, string[]> = {};
    const incomingCount: Record<string, number> = {};

    graphData.nodes.forEach((n) => (incomingCount[n.id] = 0));

    graphData.links.forEach((l) => {
      const source = typeof l.source === "object" ? (l.source as any).id : l.source;
      const target = typeof l.target === "object" ? (l.target as any).id : l.target;
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
        depthCount[0] = (depthCount[0] || 0) + 1;
      }
    });

    return Object.entries(depthCount).map(([depth, count]) => ({
      depth: depth === "0" ? "Cyclic/Isolated" : `Level ${depth}`,
      count,
    }));
  }, [graphData]);

  return (
    <div className="flex h-full w-full">
      {/* Sub-Sidebar for General Config */}
      <div className="w-56 border-r border-white/5 p-4 flex flex-col bg-black/10 overflow-y-auto shrink-0">
        <nav className="flex flex-col gap-1.5">
          {tabs.map((tab) => (
            <button
              type="button"
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2.5 px-3 py-2 transition-all duration-200 text-xs font-medium rounded-lg cursor-pointer ${
                activeTab === tab.id
                  ? "bg-white/10 text-[#39ff14] font-bold"
                  : "text-white/40 hover:text-white/80 hover:bg-white/5"
              }`}
            >
              {tab.icon}
              <span className="truncate">{tab.label}</span>
            </button>
          ))}
        </nav>
      </div>

      {/* Content Area */}
      <div className="flex-1 p-8 overflow-y-auto">
        <AnimatePresence mode="wait">
          {activeTab === "topology" && (
            <motion.div
              key="topology"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="flex flex-col gap-6 max-w-2xl h-full"
            >
              <div>
                <h2 className="text-2xl font-light text-white tracking-tight">
                  Topology Stats
                </h2>
                <p className="text-xs text-white/40 mt-1">
                  Distribution of nodes across different depth levels.
                </p>
              </div>

              <div className="flex-1 min-h-[300px] w-full mt-2 bg-[#050505]/50 border border-[#0d2a13]/80 rounded-xl p-6">
                {topologyData.length > 0 ? (
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart
                      data={topologyData}
                      margin={{ top: 20, right: 30, left: 0, bottom: 20 }}
                    >
                      <CartesianGrid strokeDasharray="3 3" stroke="#1a1a1a" vertical={false} />
                      <XAxis dataKey="depth" stroke="#ffffff40" fontSize={12} tickLine={false} axisLine={false} dy={10} />
                      <YAxis stroke="#ffffff40" fontSize={12} tickLine={false} axisLine={false} dx={-10} />
                      <RechartsTooltip
                        cursor={{ fill: "#39ff14", opacity: 0.05 }}
                        contentStyle={{
                          backgroundColor: "#0a0a0a",
                          border: "1px solid #333",
                          borderRadius: "8px",
                          color: "#fff",
                        }}
                      />
                      <Bar dataKey="count" fill="#39ff14" radius={[4, 4, 0, 0]} barSize={40} activeBar={{ stroke: "#fff", strokeWidth: 1 }} />
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
              className="flex flex-col gap-6 max-w-2xl"
            >
              <div>
                <h2 className="text-2xl font-light text-white tracking-tight">
                  Memory Management
                </h2>
                <p className="text-xs text-white/40 mt-1">
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
              </div>
            </motion.div>
          )}

          {activeTab === "advanced" && (
            <motion.div
              key="advanced"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="flex flex-col gap-6 max-w-2xl"
            >
              <div>
                <h2 className="text-2xl font-light text-white tracking-tight">
                  Advanced System Settings
                </h2>
                <p className="text-xs text-white/40 mt-1">
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
              </div>
            </motion.div>
          )}

          {activeTab === "models" && (
            <motion.div
              key="models"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="flex flex-col gap-6 max-w-2xl"
            >
              <div>
                <h2 className="text-2xl font-light text-white tracking-tight">
                  AI Models & Routing
                </h2>
                <p className="text-xs text-white/40 mt-1">
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
              </div>
            </motion.div>
          )}

          {activeTab === "server" && (
            <motion.div
              key="server"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              className="flex flex-col gap-6 max-w-2xl h-full"
            >
              <div>
                <h2 className="text-2xl font-light text-white tracking-tight">
                  Server & Network
                </h2>
                <p className="text-xs text-white/40 mt-1">
                  Configure Xavier's connectivity, P2P Cloud Relays, and Data Commons.
                </p>
              </div>
              <div className="space-y-6">
                <CloudRelayConfig token={token || ""} />
                <DataCommonsConfigUI token={token || ""} />
              </div>
            </motion.div>
          )}

          {!["memory", "advanced", "models", "topology", "server"].includes(
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
    </div>
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
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={`Toggle ${label}`}
        onClick={() => setChecked(!checked)}
        className={`relative w-12 h-7 rounded-full transition-all duration-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]/50 cursor-pointer ${checked ? "bg-[#39ff14] shadow-[0_0_15px_rgba(57,255,20,0.4)]" : "bg-white/10"}`}
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
  const inputId = useId();

  return (
    <div className="p-4 rounded-xl bg-[#050505]/50 border border-[#0d2a13]/80">
      <div className="flex items-center justify-between mb-4">
        <label htmlFor={inputId} className="text-[10px] uppercase text-white/50 tracking-widest">
          {label}
        </label>
        <span className="text-[#39ff14] text-xs font-mono neon-text">
          {format ? format(value) : value.toFixed(step < 1 ? 2 : 0)}
        </span>
      </div>
      <div className="relative pt-1">
        <input
          id={inputId}
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          onChange={(e) => setValue(Number(e.target.value))}
          className="w-full h-1 bg-[#1a1a1a] rounded-lg appearance-none cursor-pointer absolute top-1/2 -translate-y-1/2 z-10 opacity-0"
        />
        <div className="w-full h-1 bg-[#1a1a1a] rounded-lg relative overflow-visible">
          <div
            className="h-full bg-[#39ff14] rounded-lg shadow-[0_0_10px_#39ff14]"
            style={{ width: `${percentage}%` }}
          />
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
  const inputId = useId();
  return (
    <div>
      <label htmlFor={inputId} className="text-[10px] uppercase text-white/50 tracking-widest mb-2 block">
        {label}
      </label>
      <input
        id={inputId}
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
  const inputId = useId();
  return (
    <div>
      <label htmlFor={inputId} className="text-[10px] uppercase text-white/50 tracking-widest mb-2 block">
        {label}
      </label>
      <div className="relative">
        <select
          id={inputId}
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
