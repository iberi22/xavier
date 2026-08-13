import {
  Calendar,
  CheckSquare,
  FileText,
  Flag,
  GitCommit,
  RefreshCw,
  X,
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

interface GraphViewProps {
  data: GraphData;
  onUpdateData: (data: GraphData) => void;
  /** True when the unfiltered roadmap has no nodes (show create-root empty state). */
  isFullGraphEmpty?: boolean;
}

/**
 * ⚡ Bolt Performance Optimization
 *
 * 💡 What: Wrapped GraphView in React.memo()
 * 🎯 Why: ForceGraph2D is computationally expensive to render. When rendering GraphView in ConfigModal,
 *          parent state updates caused unnecessary re-renders of the entire graph subtree.
 * 📊 Impact: Prevents expensive N+1 canvas recalculations when modal tabs or other config states change.
 */
export default React.memo(function GraphView({
  data,
  onUpdateData,
  isFullGraphEmpty = false,
}: GraphViewProps) {
  const fgRef = useRef<any>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [dimensions, setDimensions] = useState({
    width: window.innerWidth,
    height: window.innerHeight,
  });

  const [hoveredNode, setHoveredNode] = useState<GraphNode | null>(null);
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 });
  const [contextMenu, setContextMenu] = useState<{
    node: GraphNode;
    x: number;
    y: number;
  } | null>(null);
  const [editNode, setEditNode] = useState<GraphNode | null>(null);
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [expandedNodes, _setExpandedNodes] = useState<Set<string>>(new Set());
  const [showStats, setShowStats] = useState(false);

  // Deep copy data to avoid mutation of react state by force-graph
  const graphData = useMemo(() => {
    return {
      nodes: data.nodes.map((n) => ({ ...n })),
      links: data.links.map((l) => ({ ...l })),
    };
  }, [data]);

  const linkCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    data.links.forEach((l) => {
      const source =
        typeof l.source === "object" ? (l.source as any).id : l.source;
      const target =
        typeof l.target === "object" ? (l.target as any).id : l.target;
      counts[source] = (counts[source] || 0) + 1;
      counts[target] = (counts[target] || 0) + 1;
    });
    return counts;
  }, [data]);

  const maxLinks = useMemo(
    () => Math.max(1, ...(Object.values(linkCounts) as number[])),
    [linkCounts],
  );

  const handleCloseContext = () => setContextMenu(null);

  const stats = useMemo(() => {
    let orgs = 0,
      projects = 0,
      subprojects = 0,
      sessions = 0;
    let newestNode: GraphNode | null = null;
    let newestDate = new Date(0);

    const childrenMap: Record<string, string[]> = {};
    data.links.forEach((l) => {
      const source =
        typeof l.source === "object" ? (l.source as any).id : l.source;
      const target =
        typeof l.target === "object" ? (l.target as any).id : l.target;
      if (!childrenMap[source]) childrenMap[source] = [];
      childrenMap[source].push(target);
    });

    const getDepth = (nodeId: string, currentDepth: number): number => {
      const children = childrenMap[nodeId] || [];
      if (children.length === 0) return currentDepth;
      return Math.max(...children.map((c) => getDepth(c, currentDepth + 1)));
    };

    let maxDepth = 0;

    for (const n of data.nodes) {
      if (n.type === "organization") {
        orgs++;
        maxDepth = Math.max(maxDepth, getDepth(n.id, 1));
      }
      if (n.type === "project") projects++;
      if (n.type === "subproject") subprojects++;
      if (n.type === "session") sessions++;

      if (n.date) {
        const dt = new Date(n.date);
        if (!Number.isNaN(dt.getTime()) && dt > newestDate) {
          newestDate = dt;
          newestNode = n;
        }
      }
    }

    return {
      total: data.nodes.length,
      orgs,
      projects,
      subprojects,
      sessions,
      newestNode,
      maxDepth,
    };
  }, [data]);

  useEffect(() => {
    const ob = new ResizeObserver((entries) => {
      if (entries[0]) {
        setDimensions({
          width: entries[0].contentRect.width,
          height: entries[0].contentRect.height,
        });
      }
    });
    if (containerRef.current) {
      ob.observe(containerRef.current);
      setDimensions({
        width: containerRef.current.offsetWidth,
        height: containerRef.current.offsetHeight,
      });
    }
    return () => ob.disconnect();
  }, []);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      setMousePos({ x: e.clientX, y: e.clientY });
    };
    window.addEventListener("mousemove", handleMouseMove);
    return () => window.removeEventListener("mousemove", handleMouseMove);
  }, []);

  const linkEnd = (end: GraphLink["source"] | GraphLink["target"]) =>
    typeof end === "object" && end !== null && "id" in end
      ? String((end as { id: string }).id)
      : String(end);

  const handleDeleteNode = (id: string) => {
    const newNodes = data.nodes.filter((n) => n.id !== id);
    const newLinks = data.links.filter(
      (l) => linkEnd(l.source) !== id && linkEnd(l.target) !== id,
    );
    onUpdateData({ nodes: newNodes, links: newLinks });
    handleCloseContext();
  };

  const handleAddRoot = () => {
    const newNode: GraphNode = {
      id: `node_${Date.now()}`,
      label: "New organization",
      description: "Root organization for this workspace roadmap",
      type: "organization",
      date: new Date().toISOString().slice(0, 10),
    };
    onUpdateData({
      nodes: [...data.nodes, newNode],
      links: [...data.links],
    });
  };

  const handleAddSub = (parentId: string) => {
    const parent = data.nodes.find((n) => n.id === parentId);
    if (!parent) return;

    let type: GraphNode["type"] = "subproject";
    if (parent.type === "organization") type = "project";
    if (parent.type === "subproject") type = "session";
    if (parent.type === "session") type = "session";

    const newNode: GraphNode = {
      id: `node_${Date.now()}`,
      label: `New ${type}`,
      description: "Newly generated link",
      type,
      parentId,
      date: new Date().toISOString().slice(0, 10),
    };
    const newLink = {
      source: parentId,
      target: newNode.id,
      relation: "contains",
    };

    onUpdateData({
      nodes: [...data.nodes, newNode],
      links: [...data.links, newLink],
    });
    handleCloseContext();
  };

  const nodePointerAreaPaint = useCallback(
    (node: any, color: string, ctx: CanvasRenderingContext2D) => {
      ctx.fillStyle = color;
      ctx.beginPath();
      // Use a larger hit area for easier interaction, especially for small subnodes
      const baseR =
        node.type === "organization" ? 8 : node.type === "project" ? 6 : 4;
      ctx.arc(node.x, node.y, baseR + 10, 0, Math.PI * 2);
      ctx.fill();
    },
    [],
  );

  const paintNode = useCallback(
    (node: any, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const isHovered = hoveredNode?.id === node.id;
      const isExp = expandedNodes.has(node.id);
      const linksCount = linkCounts[node.id] || 0;
      const intensity = linksCount / maxLinks;

      const baseR =
        node.type === "organization" ? 8 : node.type === "project" ? 6 : 4;
      const radius = isExp ? baseR * 3 : baseR;

      // Draw node
      ctx.beginPath();
      ctx.arc(node.x, node.y, radius, 0, Math.PI * 2);
      ctx.fillStyle = isHovered
        ? "#fff"
        : node.type === "organization"
          ? "#39ff14"
          : node.type === "project"
            ? "#006400"
            : "#888";

      const hue = 200 + intensity * 120;
      const glowColor = `hsla(${hue}, 100%, 60%, ${0.3 + intensity * 0.5})`;

      if (isHovered || isExp || intensity > 0.1) {
        ctx.shadowBlur = (isExp ? 25 : 10 + intensity * 25) / globalScale;
        ctx.shadowColor = isHovered || isExp ? "#39ff14" : glowColor;
      } else {
        ctx.shadowBlur = 0;
      }

      ctx.fill();
      ctx.shadowBlur = 0; // reset for next paints

      // Draw label if expanded or zoomed in enough
      if (isExp || globalScale > 2) {
        ctx.fillStyle = isExp ? "#0a0a0a" : "rgba(255, 255, 255, 0.8)";
        ctx.font = `${isExp ? 10 : 4}px monospace`;
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        const label = isExp ? node.label.substring(0, 10) : node.label;
        ctx.fillText(label, node.x, node.y + (isExp ? 0 : radius + 4));
      }
    },
    [hoveredNode, expandedNodes, linkCounts, maxLinks],
  );

  const paintLink = useCallback(
    (link: any, ctx: CanvasRenderingContext2D, globalScale: number) => {
      const s = link.source;
      const t = link.target;
      if (!s || !t || s.x === undefined || t.x === undefined) return;

      ctx.beginPath();
      const isHoveredLink =
        hoveredNode && (hoveredNode.id === s.id || hoveredNode.id === t.id);
      ctx.strokeStyle = isHoveredLink
        ? "rgba(57, 255, 20, 0.8)"
        : "rgba(57, 255, 20, 0.15)";
      ctx.lineWidth = isHoveredLink ? 2 / globalScale : 1 / globalScale;
      ctx.moveTo(s.x, s.y);
      ctx.lineTo(t.x, t.y);
      ctx.stroke();
    },
    [hoveredNode],
  );

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="w-full h-full relative"
      ref={containerRef}
    >
      <div className="absolute top-6 left-6 z-10 pointer-events-auto">
        <div className="bg-[#050505]/80 backdrop-blur border border-white/10 rounded-xl overflow-hidden transition-all duration-300 shadow-xl">
          <button
            onClick={() => setShowStats(!showStats)}
            className="px-4 py-3 text-xs text-white/80 hover:text-white flex items-center justify-between w-full min-w-[220px]"
          >
            <span className="font-mono tracking-widest uppercase">
              System Diagnostics
            </span>
            <span className="text-[10px] bg-white/10 px-2 py-1 rounded text-white/80 hover:bg-white/20 transition-colors">
              {showStats ? "Ocultar" : "Ver más"}
            </span>
          </button>
          <AnimatePresence>
            {showStats && (
              <motion.div
                initial={{ height: 0, opacity: 0 }}
                animate={{ height: "auto", opacity: 1 }}
                exit={{ height: 0, opacity: 0 }}
                className="overflow-hidden"
              >
                <div className="p-4 pt-2 border-t border-white/10 text-xs text-white/70 space-y-2">
                  <div className="flex justify-between text-white mb-2 font-medium">
                    <span>Total Nodes:</span> <span>{stats.total}</span>
                  </div>
                  <div className="flex justify-between text-[10px] text-white/50">
                    <span>↳ Orgs:</span> <span>{stats.orgs}</span>
                  </div>
                  <div className="flex justify-between text-[10px] text-white/50">
                    <span>↳ Projects:</span> <span>{stats.projects}</span>
                  </div>
                  <div className="flex justify-between text-[10px] text-white/50">
                    <span>↳ Subprojects:</span> <span>{stats.subprojects}</span>
                  </div>
                  <div className="flex justify-between text-[10px] text-white/50">
                    <span>↳ Sessions:</span> <span>{stats.sessions}</span>
                  </div>
                  <div className="flex justify-between pt-3 border-t border-white/5 mt-3 text-white/90">
                    <span>Links Count:</span> <span>{data.links.length}</span>
                  </div>
                  <div className="flex justify-between pt-1 border-white/5 mt-1 text-white/90">
                    <span>Max Depth (Levels):</span>{" "}
                    <span>{stats.maxDepth}</span>
                  </div>
                  {stats.newestNode && (
                    <div className="pt-3 border-t border-white/5 mt-3">
                      <span className="block text-[10px] uppercase text-white/40 mb-1 tracking-widest">
                        Recent Activity
                      </span>
                      <span className="text-[#39ff14]/90 block w-[180px] font-medium leading-tight">
                        {stats.newestNode.label}
                      </span>
                      <span className="text-[10px] text-white/50 font-mono mt-1 block">
                        {stats.newestNode.date}
                      </span>
                    </div>
                  )}
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>

      <div className="absolute inset-0 bg-[#020202]">
        <ForceGraph2D
          ref={fgRef}
          width={dimensions.width}
          height={dimensions.height}
          graphData={graphData}
          nodeCanvasObject={paintNode}
          nodePointerAreaPaint={nodePointerAreaPaint}
          linkCanvasObject={paintLink}
          nodeRelSize={8}
          d3VelocityDecay={0.3}
          d3AlphaDecay={0.02}
          onNodeHover={(node) => setHoveredNode((node as GraphNode) || null)}
          onNodeClick={(node) => {
            handleCloseContext();
            setSelectedNode(node as GraphNode);
          }}
          onNodeDragEnd={(node) => {
            // Optional: Pin the node after dragging by setting fx/fy
            node.fx = node.x;
            node.fy = node.y;
          }}
          onNodeRightClick={(node, _e) => {
            // react-force-graph doesn't pass native event coords directly to onNodeRightClick easily,
            // so we use window mousePos tracker
            setContextMenu({
              node: node as GraphNode,
              x: mousePos.x,
              y: mousePos.y,
            });
          }}
          onBackgroundClick={() => {
            handleCloseContext();
            setSelectedNode(null);
          }}
          onBackgroundRightClick={handleCloseContext}
        />
      </div>

      {isFullGraphEmpty && data.nodes.length === 0 && (
        <div className="absolute inset-0 z-20 flex items-center justify-center pointer-events-none">
          <div className="pointer-events-auto text-center max-w-sm px-6 py-8 rounded-2xl border border-white/10 bg-[#050505]/90 backdrop-blur-md shadow-2xl">
            <p className="text-sm text-white/80 font-medium mb-1">
              No roadmap nodes yet
            </p>
            <p className="text-xs text-white/40 mb-5 leading-relaxed">
              Build an org → project → session topology for this workspace. It
              saves to Xavier automatically.
            </p>
            <button
              type="button"
              onClick={handleAddRoot}
              className="px-4 py-2 rounded-lg bg-[#39ff14]/15 border border-[#39ff14]/40 text-[#39ff14] text-xs font-medium tracking-wide hover:bg-[#39ff14]/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]/50 transition-all"
            >
              Add root organization
            </button>
          </div>
        </div>
      )}

      {!isFullGraphEmpty && data.nodes.length === 0 && (
        <div className="absolute inset-0 z-20 flex items-center justify-center pointer-events-none">
          <p className="pointer-events-auto text-xs text-white/40 px-4 py-2 rounded-lg border border-white/10 bg-black/70">
            No nodes match the current filters
          </p>
        </div>
      )}

      <AnimatePresence>
        {hoveredNode && !selectedNode && contextMenu === null && (
          <motion.div
            initial={{ opacity: 0, scale: 0.9, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.9, y: 10 }}
            transition={{ duration: 0.15 }}
            className="fixed z-[100] bg-[#050505]/95 border border-[#39ff14]/30 rounded-xl p-4 shadow-[0_0_20px_rgba(57,255,20,0.1)] backdrop-blur-xl pointer-events-none"
            style={{
              left: mousePos.x + 20,
              top: mousePos.y + 20,
              maxWidth: 240,
            }}
          >
            <div className="flex items-center gap-2 mb-2">
              <div className="w-2 h-2 rounded-full bg-[#39ff14] animate-pulse" />
              <span className="text-[10px] uppercase tracking-widest text-[#39ff14]/80 font-mono">
                {hoveredNode.type}
              </span>
              {hoveredNode.date && (
                <span className="text-[10px] text-white/40 ml-auto whitespace-nowrap">
                  {hoveredNode.date}
                </span>
              )}
            </div>
            <h3 className="text-white font-medium mb-1 leading-tight">
              {hoveredNode.label}
            </h3>
            <p className="text-xs text-white/50 mb-3">
              {hoveredNode.description}
            </p>

            <div className="flex flex-col gap-1.5 mt-2 border-t border-white/5 pt-3">
              {(hoveredNode.relatedFiles?.length ?? 0) > 0 && (
                <div className="flex items-center gap-2 text-[10px] text-white/60">
                  <FileText className="w-3 h-3 text-blue-400" />
                  <span>{hoveredNode.relatedFiles?.length} linked files</span>
                </div>
              )}
              {(hoveredNode.commits?.length ?? 0) > 0 && (
                <div className="flex items-center gap-2 text-[10px] text-white/60">
                  <GitCommit className="w-3 h-3 text-orange-400" />
                  <span>{hoveredNode.commits?.length} recent commits</span>
                </div>
              )}
            </div>
            {hoveredNode.parentId && (
              <p className="text-[10px] text-white/30 mt-3 border-t border-white/5 pt-2 truncate">
                Parent:{" "}
                {data.nodes.find(
                  (n: GraphNode) => n.id === hoveredNode.parentId,
                )?.label || hoveredNode.parentId}
              </p>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {contextMenu && (
        <div
          className="fixed z-50 bg-[#0a0a0a] border border-white/10 rounded-xl shadow-2xl overflow-hidden py-1 min-w-[160px]"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <button
            className="w-full text-left px-4 py-2 text-sm text-white/80 hover:bg-white/10 transition-colors"
            onClick={(e) => {
              e.stopPropagation();
              setEditNode(contextMenu.node);
              handleCloseContext();
            }}
          >
            Edit Metadata
          </button>
          <button
            className="w-full text-left px-4 py-2 text-sm text-white/80 hover:bg-white/10 transition-colors"
            onClick={(e) => {
              e.stopPropagation();
              handleAddSub(contextMenu.node.id);
            }}
          >
            Add Sub-project
          </button>
          <div className="h-px w-full bg-white/5 my-1" />
          <button
            className="w-full text-left px-4 py-2 text-sm text-red-400 hover:bg-red-400/10 transition-colors"
            onClick={(e) => {
              e.stopPropagation();
              handleDeleteNode(contextMenu.node.id);
            }}
          >
            Delete Entity
          </button>
        </div>
      )}

      <AnimatePresence>
        {editNode && (
          <motion.div
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.9 }}
            className="fixed z-[60] inset-0 flex items-center justify-center bg-black/40 backdrop-blur-sm"
          >
            <div className="bg-[#050505] border border-[#39ff14]/30 rounded-2xl p-6 w-[400px] shadow-[0_0_30px_rgba(57,255,20,0.1)]">
              <h3 className="text-xl text-white font-light tracking-tight mb-4">
                Edit Node Metadata
              </h3>
              <div className="space-y-4">
                <div>
                  <label className="text-[10px] uppercase text-white/50 tracking-widest mb-2 block">
                    Label
                  </label>
                  <input
                    type="text"
                    defaultValue={editNode.label}
                    id="edit-node-label"
                    className="w-full bg-[#0a0a0a] border border-[#1a1a1a] focus:border-[#39ff14] text-white/90 text-sm px-4 py-3 rounded-xl outline-none transition-all"
                  />
                </div>
                <div>
                  <label className="text-[10px] uppercase text-white/50 tracking-widest mb-2 block">
                    Description
                  </label>
                  <input
                    type="text"
                    defaultValue={editNode.description}
                    id="edit-node-desc"
                    className="w-full bg-[#0a0a0a] border border-[#1a1a1a] focus:border-[#39ff14] text-white/90 text-sm px-4 py-3 rounded-xl outline-none transition-all"
                  />
                </div>
              </div>
              <div className="flex gap-3 mt-6">
                <button
                  onClick={() => setEditNode(null)}
                  className="flex-1 px-4 py-2 rounded-lg bg-white/5 text-white/70 hover:bg-white/10 hover:text-white transition-colors text-sm"
                >
                  Cancel
                </button>
                <button
                  onClick={() => {
                    const label = (
                      document.getElementById(
                        "edit-node-label",
                      ) as HTMLInputElement
                    ).value;
                    const description = (
                      document.getElementById(
                        "edit-node-desc",
                      ) as HTMLInputElement
                    ).value;
                    const newNodes = data.nodes.map((n: GraphNode) =>
                      n.id === editNode.id ? { ...n, label, description } : n,
                    );
                    onUpdateData({ nodes: newNodes, links: data.links });
                    setEditNode(null);
                  }}
                  className="flex-1 px-4 py-2 rounded-lg bg-[#39ff14]/20 text-[#39ff14] border border-[#39ff14]/50 hover:bg-[#39ff14]/40 hover:shadow-[0_0_15px_rgba(57,255,20,0.4)] transition-all text-sm font-medium"
                >
                  Save Changes
                </button>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <AnimatePresence>
        {selectedNode && (
          <motion.div
            initial={{ opacity: 0, x: 50 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: 50 }}
            transition={{ type: "spring", damping: 25, stiffness: 200 }}
            className="absolute top-0 right-0 bottom-0 w-[400px] max-w-[90vw] bg-[#050505]/95 shadow-[0_0_40px_rgba(0,0,0,0.8)] border-l border-white/10 flex flex-col z-30 backdrop-blur-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between p-6 border-b border-white/5">
              <div>
                <div className="flex items-center gap-2 mb-1">
                  <span className="text-[10px] uppercase tracking-widest text-[#39ff14] font-mono">
                    {selectedNode.type}
                  </span>
                </div>
                <h2 className="text-xl text-white font-medium">
                  {selectedNode.label}
                </h2>
              </div>
              <button
                onClick={() => setSelectedNode(null)}
                className="p-2 text-white/50 hover:text-white transition-colors bg-white/5 hover:bg-white/10 rounded-full"
                aria-label="Close details"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="flex-1 overflow-y-auto p-6 space-y-8 scrollbar-hide">
              <section>
                <p className="text-sm text-white/70 leading-relaxed mb-4">
                  {selectedNode.description}
                </p>
                {selectedNode.reason && (
                  <div className="bg-[#1a1a1a] rounded-xl p-4 border border-white/5">
                    <h4 className="text-[10px] uppercase text-white/40 tracking-widest mb-1 pointer-events-none">
                      Creation Motive
                    </h4>
                    <p className="text-sm text-white/80">
                      {selectedNode.reason}
                    </p>
                  </div>
                )}
              </section>

              <section className="grid grid-cols-2 gap-4">
                <div className="bg-[#0a0a0a] rounded-xl p-3 border border-white/5 flex items-center gap-3">
                  <div className="w-8 h-8 rounded-lg bg-blue-500/10 flex items-center justify-center">
                    <Calendar className="w-4 h-4 text-blue-400" />
                  </div>
                  <div>
                    <p className="text-[10px] uppercase text-white/40">
                      Creation Date
                    </p>
                    <p className="text-xs text-white tracking-wide">
                      {selectedNode.date || "Unknown"}
                    </p>
                  </div>
                </div>
                <div className="bg-[#0a0a0a] rounded-xl p-3 border border-white/5 flex items-center gap-3">
                  <div className="w-8 h-8 rounded-lg bg-purple-500/10 flex items-center justify-center">
                    <Flag className="w-4 h-4 text-purple-400" />
                  </div>
                  <div>
                    <p className="text-[10px] uppercase text-white/40">
                      Milestone
                    </p>
                    <p className="text-xs text-white tracking-wide">
                      {selectedNode.milestone || "None"}
                    </p>
                  </div>
                </div>
              </section>

              {selectedNode.relatedFiles &&
                selectedNode.relatedFiles.length > 0 && (
                  <section>
                    <h4 className="text-[10px] uppercase text-white/40 tracking-widest mb-3 flex items-center gap-2">
                      <FileText className="w-3 h-3" /> Related Files
                    </h4>
                    <ul className="space-y-2">
                      {selectedNode.relatedFiles.map(
                        (file: string, i: number) => (
                          <li
                            key={i}
                            className="text-xs text-white/80 flex items-center gap-2 before:content-[''] before:w-1 before:h-1 before:bg-[#39ff14] before:rounded-full bg-white/5 p-2 rounded-lg"
                          >
                            {file}
                          </li>
                        ),
                      )}
                    </ul>
                  </section>
                )}

              {selectedNode.decisions && selectedNode.decisions.length > 0 && (
                <section>
                  <h4 className="text-[10px] uppercase text-white/40 tracking-widest mb-3 flex items-center gap-2">
                    <CheckSquare className="w-3 h-3" /> Key Decisions
                  </h4>
                  <ul className="space-y-2">
                    {selectedNode.decisions.map((dec: string, i: number) => (
                      <li
                        key={i}
                        className="text-xs text-white/80 p-2 border-l-2 border-[#39ff14]/50 bg-gradient-to-r from-[#39ff14]/10 to-transparent"
                      >
                        {dec}
                      </li>
                    ))}
                  </ul>
                </section>
              )}

              {selectedNode.commits && selectedNode.commits.length > 0 && (
                <section>
                  <h4 className="text-[10px] uppercase text-white/40 tracking-widest mb-3 flex items-center gap-2">
                    <GitCommit className="w-3 h-3" /> Associated Commits
                  </h4>
                  <div className="space-y-2 relative before:absolute before:inset-y-2 before:left-2.5 before:w-px before:bg-white/10">
                    {selectedNode.commits.map((commit: string, i: number) => (
                      <div
                        key={i}
                        className="relative flex items-center gap-3 pl-8 text-xs text-white/70"
                      >
                        <div className="absolute left-1.5 w-2 h-2 rounded-full border border-white/30 bg-[#050505]" />
                        <span className="font-mono text-orange-400">
                          {commit.split(" - ")[0]}
                        </span>
                        <span className="truncate">
                          {commit.split(" - ")[1]}
                        </span>
                      </div>
                    ))}
                  </div>
                </section>
              )}

              {selectedNode.iterations &&
                selectedNode.iterations.length > 0 && (
                  <section>
                    <h4 className="text-[10px] uppercase text-white/40 tracking-widest mb-3 flex items-center gap-2">
                      <RefreshCw className="w-3 h-3" /> Iterations & Queries
                    </h4>
                    <div className="flex flex-wrap gap-2">
                      {selectedNode.iterations.map(
                        (iter: string, i: number) => (
                          <span
                            key={i}
                            className="text-[11px] text-[#39ff14]/80 bg-[#39ff14]/10 border border-[#39ff14]/20 px-2 py-1 rounded-md"
                          >
                            {iter}
                          </span>
                        ),
                      )}
                    </div>
                  </section>
                )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  );
});
