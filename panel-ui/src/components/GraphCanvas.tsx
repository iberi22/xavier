import {
  Brain,
  FileCode,
  FolderCode,
  AlertCircle,
  X,
  Minimize2,
  Maximize2,
  Cpu,
  Bookmark,
  ChevronRight,
} from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import ForceGraph2D from "react-force-graph-2d";
import type { GraphData, GraphLink, GraphNode } from "../types";

interface GraphCanvasProps {
  data: GraphData;
  loading?: boolean;
  error?: string | null;
  emptyMessage?: string;
  isTruncated?: boolean;
  onNodeDoubleClick?: (node: GraphNode) => void;
  onNodeExpand?: (node: GraphNode) => void;
  onNodeSelect?: (node: GraphNode) => Promise<any> | void;
  isCodeMode?: boolean;
}

export default function GraphCanvas({
  data,
  loading = false,
  error = null,
  emptyMessage = "No graph data available",
  isTruncated = false,
  onNodeDoubleClick,
  onNodeExpand,
  onNodeSelect,
  isCodeMode = false,
}: GraphCanvasProps) {
  const fgRef = useRef<any>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [dimensions, setDimensions] = useState({
    width: 600,
    height: 400,
  });

  const [hoveredNode, setHoveredNode] = useState<GraphNode | null>(null);
  const [selectedNode, setSelectedNode] = useState<any | null>(null);
  const [selectedNodeExtra, setSelectedNodeExtra] = useState<any>(null);
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 });
  const lastClickTimeRef = useRef<number>(0);

  // Handle auto resizing
  useEffect(() => {
    const observer = new ResizeObserver((entries) => {
      if (entries[0]) {
        setDimensions({
          width: entries[0].contentRect.width,
          height: entries[0].contentRect.height,
        });
      }
    });

    if (containerRef.current) {
      observer.observe(containerRef.current);
      setDimensions({
        width: containerRef.current.clientWidth || 600,
        height: containerRef.current.clientHeight || 400,
      });
    }

    return () => observer.disconnect();
  }, []);

  // Track global mouse position for tooltip placement
  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      setMousePos({ x: e.clientX, y: e.clientY });
    };
    window.addEventListener("mousemove", handleMouseMove);
    return () => window.removeEventListener("mousemove", handleMouseMove);
  }, []);

  // Deep copy nodes and links so force-graph's internal mutations don't trigger React warnings
  const graphData = useMemo(() => {
    return {
      nodes: data.nodes.map((n) => ({ ...n })),
      links: data.links.map((l) => ({ ...l })),
    };
  }, [data]);

  const linkCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    data.links.forEach((l) => {
      const source = typeof l.source === "object" ? (l.source as any).id : l.source;
      const target = typeof l.target === "object" ? (l.target as any).id : l.target;
      counts[source] = (counts[source] || 0) + 1;
      counts[target] = (counts[target] || 0) + 1;
    });
    return counts;
  }, [data]);

  const maxLinks = useMemo(
    () => Math.max(1, ...(Object.values(linkCounts) as number[])),
    [linkCounts],
  );

  const handleNodeClick = async (node: any) => {
    const nodeObj = node as GraphNode;

    // Detect double click manually since ForceGraph2D doesn't have native onNodeDoubleClick
    const now = Date.now();
    const diff = now - lastClickTimeRef.current;
    if (diff < 300) {
      if (onNodeDoubleClick) {
        onNodeDoubleClick(nodeObj);
        return;
      }
    }
    lastClickTimeRef.current = now;

    setSelectedNode(nodeObj);
    setSelectedNodeExtra(null);
    if (onNodeSelect) {
      try {
        const extra = await onNodeSelect(nodeObj);
        if (extra) {
          setSelectedNodeExtra(extra);
        }
      } catch (err) {
        console.warn("Failed to fetch node detail:", err);
      }
    }
  };

  const paintNode = useCallback(
    (node: any, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const isHovered = hoveredNode?.id === node.id;
      const isSelected = selectedNode?.id === node.id;
      const linksCount = linkCounts[node.id] || 0;
      const intensity = linksCount / maxLinks;

      // Base radius depending on node type/kind
      let baseR = 5;
      const typeLower = String(node.type || "").toLowerCase();
      if (typeLower === "concept" || typeLower === "class" || typeLower === "struct") {
        baseR = 7;
      } else if (typeLower === "person" || typeLower === "function" || typeLower === "fn") {
        baseR = 6;
      }

      const radius = isSelected ? baseR * 1.5 : isHovered ? baseR * 1.2 : baseR;

      ctx.beginPath();
      ctx.arc(node.x, node.y, radius, 0, Math.PI * 2);

      // Distinct high-contrast colors for read-only layers
      let nodeColor = "#888";
      if (isCodeMode) {
        if (typeLower.includes("fn") || typeLower.includes("function")) {
          nodeColor = "#00bcd4"; // Cyan
        } else if (typeLower.includes("struct") || typeLower.includes("class")) {
          nodeColor = "#9c27b0"; // Purple
        } else if (typeLower.includes("import") || typeLower.includes("module")) {
          nodeColor = "#ff9800"; // Orange
        } else {
          nodeColor = "#4caf50"; // Green
        }
      } else {
        // Memory KG layer colors
        if (typeLower === "concept") {
          nodeColor = "#39ff14"; // Neon green
        } else if (typeLower === "person") {
          nodeColor = "#2196f3"; // Blue
        } else if (typeLower === "organization") {
          nodeColor = "#e91e63"; // Pink
        } else {
          nodeColor = "#00e5ff"; // Teal
        }
      }

      ctx.fillStyle = isHovered ? "#ffffff" : nodeColor;

      // Outer glow for hovered/selected nodes
      if (isHovered || isSelected || intensity > 0.1) {
        ctx.shadowBlur = (isSelected ? 20 : 10 + intensity * 15) / globalScale;
        ctx.shadowColor = isHovered || isSelected ? "#ffffff" : nodeColor;
      } else {
        ctx.shadowBlur = 0;
      }

      ctx.fill();
      ctx.shadowBlur = 0; // reset shadow

      // Draw label if zoomed in or hovered/selected
      if (globalScale > 1.8 || isHovered || isSelected) {
        ctx.fillStyle = isSelected ? "#39ff14" : "rgba(255, 255, 255, 0.85)";
        ctx.font = `${isHovered || isSelected ? 8 : 6}px monospace`;
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        ctx.fillText(node.label || node.id, node.x, node.y + radius + 3);
      }
    },
    [hoveredNode, selectedNode, linkCounts, maxLinks, isCodeMode],
  );

  const paintLink = useCallback(
    (link: any, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const s = link.source;
      const t = link.target;
      if (!s || !t || s.x === undefined || t.x === undefined) return;

      ctx.beginPath();
      const isHoveredLink =
        hoveredNode && (hoveredNode.id === s.id || hoveredNode.id === t.id);
      const isSelectedLink =
        selectedNode && (selectedNode.id === s.id || selectedNode.id === t.id);

      ctx.strokeStyle = isSelectedLink
        ? "rgba(57, 255, 20, 0.8)"
        : isHoveredLink
          ? "rgba(255, 255, 255, 0.5)"
          : "rgba(255, 255, 255, 0.12)";

      ctx.lineWidth = isSelectedLink || isHoveredLink ? 1.5 / globalScale : 0.8 / globalScale;
      ctx.moveTo(s.x, s.y);
      ctx.lineTo(t.x, t.y);
      ctx.stroke();
    },
    [hoveredNode, selectedNode],
  );

  return (
    <div className="w-full h-full relative" ref={containerRef}>
      {/* Loading Overlay */}
      {loading && (
        <div className="absolute inset-0 z-30 flex items-center justify-center bg-black/50 backdrop-blur-sm">
          <div className="flex flex-col items-center gap-3">
            <div className="w-8 h-8 border-4 border-t-[#39ff14] border-r-transparent border-b-transparent border-l-transparent rounded-full animate-spin" />
            <span className="text-xs text-white/60 font-mono tracking-widest uppercase">
              Loading Graph...
            </span>
          </div>
        </div>
      )}

      {/* Error Overlay */}
      {error && (
        <div className="absolute inset-0 z-30 flex items-center justify-center bg-black/60 p-6">
          <div className="flex flex-col items-center max-w-sm text-center bg-[#0a0a0a]/90 border border-red-500/30 rounded-2xl p-6 shadow-2xl">
            <AlertCircle className="w-8 h-8 text-red-500 mb-3 animate-pulse" />
            <h4 className="text-sm font-semibold text-white mb-2">
              Failed to load graph
            </h4>
            <p className="text-xs text-white/50 leading-relaxed mb-4">{error}</p>
          </div>
        </div>
      )}

      {/* Truncated Badge */}
      {isTruncated && !loading && !error && (
        <div className="absolute top-4 right-4 z-20">
          <span
            className="flex items-center gap-1.5 px-2.5 py-1 text-[10px] font-mono font-semibold uppercase tracking-wider bg-orange-500/10 border border-orange-500/30 text-orange-400 rounded-full shadow-[0_0_15px_rgba(239,108,0,0.1)]"
            title="Some nodes or relations have been truncated to fit safety limits."
          >
            <span className="w-1.5 h-1.5 rounded-full bg-orange-400 animate-ping" />
            TRUNCATED
          </span>
        </div>
      )}

      {/* Empty State */}
      {!loading && !error && data.nodes.length === 0 && (
        <div className="absolute inset-0 z-20 flex items-center justify-center bg-[#050505]/60 backdrop-blur-sm pointer-events-none">
          <div className="pointer-events-auto text-center max-w-xs px-6 py-8 rounded-2xl border border-white/10 bg-[#050505]/95 shadow-2xl">
            <div className="w-12 h-12 rounded-full bg-white/5 flex items-center justify-center mx-auto mb-4 border border-white/10">
              <Brain className="w-5 h-5 text-white/40" />
            </div>
            <p className="text-xs text-white/40 leading-relaxed mb-1">
              {emptyMessage}
            </p>
          </div>
        </div>
      )}

      {/* Main Force Graph Canvas */}
      {!error && (
        <div className="absolute inset-0 bg-[#020202]">
          <ForceGraph2D
            ref={fgRef}
            width={dimensions.width}
            height={dimensions.height}
            graphData={graphData}
            nodeCanvasObject={paintNode}
            linkCanvasObject={paintLink}
            nodeRelSize={6}
            d3VelocityDecay={0.4}
            d3AlphaDecay={0.03}
            onNodeHover={(node) => setHoveredNode((node as GraphNode) || null)}
            onNodeClick={handleNodeClick}
            onBackgroundClick={() => {
              setSelectedNode(null);
              setSelectedNodeExtra(null);
            }}
          />
        </div>
      )}

      {/* Floating Tooltip / Hover Card */}
      <AnimatePresence>
        {hoveredNode && !selectedNode && (
          <motion.div
            initial={{ opacity: 0, scale: 0.95, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: 10 }}
            transition={{ duration: 0.12 }}
            className="fixed z-[100] bg-[#050505]/95 border border-white/15 rounded-xl p-4 shadow-xl backdrop-blur-xl pointer-events-none max-w-[260px]"
            style={{
              left: mousePos.x + 15,
              top: mousePos.y + 15,
            }}
          >
            <div className="flex items-center gap-2 mb-1.5">
              <span className="text-[10px] uppercase tracking-widest text-[#39ff14] font-mono">
                {hoveredNode.type || "symbol"}
              </span>
            </div>
            <h4 className="text-white text-xs font-semibold leading-tight truncate">
              {hoveredNode.label || hoveredNode.id}
            </h4>
            {hoveredNode.description && (
              <p className="text-[10px] text-white/50 mt-1 line-clamp-2">
                {hoveredNode.description}
              </p>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* Selected Node Details Slide-over Panel */}
      <AnimatePresence>
        {selectedNode && (
          <motion.div
            initial={{ opacity: 0, x: 40 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: 40 }}
            transition={{ type: "spring", damping: 25, stiffness: 220 }}
            className="absolute top-0 right-0 bottom-0 w-[350px] max-w-[85vw] bg-[#050505]/95 shadow-[0_0_35px_rgba(0,0,0,0.9)] border-l border-white/10 flex flex-col z-30 backdrop-blur-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between p-5 border-b border-white/5">
              <div className="min-w-0">
                <span className="text-[9px] uppercase tracking-widest text-[#39ff14] font-mono block mb-1">
                  {selectedNode.type}
                </span>
                <h3 className="text-base text-white font-medium truncate">
                  {selectedNode.label}
                </h3>
              </div>
              <button
                type="button"
                onClick={() => {
                  setSelectedNode(null);
                  setSelectedNodeExtra(null);
                }}
                className="p-1.5 text-white/40 hover:text-white transition-colors bg-white/5 hover:bg-white/10 rounded-full"
                aria-label="Close details"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>

            <div className="flex-1 overflow-y-auto p-5 space-y-6 scrollbar-hide text-xs">
              {/* Description */}
              {selectedNode.description && (
                <section className="bg-white/[0.02] border border-white/[0.04] p-3 rounded-xl">
                  <span className="text-[9px] uppercase text-white/40 block mb-1">
                    Description
                  </span>
                  <p className="text-white/80 leading-relaxed font-mono whitespace-pre-wrap break-all">
                    {selectedNode.description}
                  </p>
                </section>
              )}

              {/* Node Metadata & API detail */}
              <section className="space-y-3">
                <span className="text-[9px] uppercase text-white/40 block tracking-wider">
                  Properties
                </span>
                <div className="grid grid-cols-2 gap-2">
                  <div className="bg-black/30 p-2.5 rounded-lg border border-white/5">
                    <span className="text-[9px] text-white/40 block">ID</span>
                    <span className="text-[10px] text-white font-mono truncate block">
                      {selectedNode.id}
                    </span>
                  </div>
                  {selectedNode.meta?.kind && (
                    <div className="bg-black/30 p-2.5 rounded-lg border border-white/5">
                      <span className="text-[9px] text-white/40 block">Kind</span>
                      <span className="text-[10px] text-white font-mono truncate block">
                        {selectedNode.meta.kind}
                      </span>
                    </div>
                  )}
                  {selectedNode.meta?.trust !== undefined && (
                    <div className="bg-black/30 p-2.5 rounded-lg border border-white/5">
                      <span className="text-[9px] text-white/40 block">Trust Level</span>
                      <span className="text-[10px] text-white font-mono block">
                        {selectedNode.meta.trust}
                      </span>
                    </div>
                  )}
                  {selectedNode.meta?.memory_count !== undefined && (
                    <div className="bg-black/30 p-2.5 rounded-lg border border-white/5">
                      <span className="text-[9px] text-white/40 block">Memory Count</span>
                      <span className="text-[10px] text-[#39ff14] font-mono block">
                        {selectedNode.meta.memory_count}
                      </span>
                    </div>
                  )}
                  {selectedNode.meta?.language && (
                    <div className="bg-black/30 p-2.5 rounded-lg border border-white/5">
                      <span className="text-[9px] text-white/40 block">Language</span>
                      <span className="text-[10px] text-white font-mono block">
                        {selectedNode.meta.language}
                      </span>
                    </div>
                  )}
                  {selectedNode.meta?.complexity !== undefined && (
                    <div className="bg-black/30 p-2.5 rounded-lg border border-white/5">
                      <span className="text-[9px] text-white/40 block">Complexity</span>
                      <span className="text-[10px] text-white font-mono block">
                        {selectedNode.meta.complexity}
                      </span>
                    </div>
                  )}
                </div>
              </section>

              {/* Extra Dynamic Properties from detail API (GET /memory/graph/entities/{id}) */}
              {selectedNodeExtra && (
                <section className="space-y-2 border-t border-white/5 pt-4">
                  <span className="text-[9px] uppercase text-[#39ff14] block tracking-wider">
                    Extended Evidence
                  </span>
                  <div className="bg-black/40 border border-[#39ff14]/15 p-3 rounded-xl space-y-2 font-mono text-[10px] text-white/70">
                    {Object.entries(selectedNodeExtra).map(([key, val]: [string, any]) => {
                      if (typeof val === "object") return null;
                      return (
                        <div key={key} className="flex justify-between gap-4">
                          <span className="text-white/40">{key}:</span>
                          <span className="text-right truncate max-w-[180px]">{String(val)}</span>
                        </div>
                      );
                    })}
                  </div>
                </section>
              )}

              {/* Expand / Navigation Actions */}
              {isCodeMode && (onNodeDoubleClick || onNodeExpand) && (
                <div className="pt-4 border-t border-white/5">
                  <button
                    type="button"
                    onClick={() => {
                      if (onNodeExpand) onNodeExpand(selectedNode);
                      else if (onNodeDoubleClick) onNodeDoubleClick(selectedNode);
                    }}
                    className="w-full flex items-center justify-center gap-1.5 py-2 px-3 bg-[#39ff14]/15 border border-[#39ff14]/40 text-[#39ff14] font-medium tracking-wide rounded-lg hover:bg-[#39ff14]/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]/50 transition-all text-xs"
                  >
                    <span>Expand Ego Graph</span>
                    <ChevronRight className="w-3.5 h-3.5" />
                  </button>
                </div>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
