<script lang="ts">
  import { onMount } from "svelte";
  import * as d3 from "d3";
  import { fetchGraph, type GraphNode, type GraphEdge } from "../lib/graphClient";

  // State
  let container: HTMLDivElement;
  let svg: d3.Selection<SVGSVGElement, unknown, null, undefined>;
  let g: d3.Selection<SVGGElement, unknown, null, undefined>;
  let simulation: d3.Simulation<d3.SimulationNodeDatum & GraphNode, undefined>;
  let zoom: d3.ZoomBehavior<SVGSVGElement, unknown>;

  let layer: "memory" | "code" | "ecosystem" = $state("memory");
  let loading = $state(false);
  let error = $state<string | null>(null);

  // Filters
  let minWeight = $state(0);
  let selectedTypes = $state<Set<string>>(new Set(["entity", "code_symbol", "commit", "decision"]));

  // Data
  let nodesMap = new Map<string, GraphNode>();
  let edgesMap = new Map<string, GraphEdge>();
  
  let currentNodes = $state<(GraphNode & d3.SimulationNodeDatum)[]>([]);
  let currentEdges = $state<(GraphEdge & d3.SimulationLinkDatum<GraphNode & d3.SimulationNodeDatum>)[]>([]);

  // Tooltip
  let tooltipNode = $state<GraphNode | null>(null);
  let tooltipX = $state(0);
  let tooltipY = $state(0);

  const colors = d3.scaleOrdinal(d3.schemeSet2);

  // Colors based on app_id (Hive Dark style)
  function getColor(appId?: string) {
    if (!appId) return "#475569"; // slate-600
    if (appId === "core") return "#0ea5e9"; // cyan-500
    if (appId === "swal-backoffice") return "#10b981"; // emerald-500
    if (appId === "swal-node") return "#8b5cf6"; // violet-500
    return colors(appId);
  }

  function getShape(type: string, size = 15) {
    const symbol = d3.symbol().size(size * size * 2);
    switch (type) {
      case "code_symbol":
        return symbol.type(d3.symbolSquare)();
      case "commit":
        return symbol.type(d3.symbolDiamond)();
      case "decision":
        return symbol.type(d3.symbolStar)();
      default: // entity
        return symbol.type(d3.symbolCircle)();
    }
  }

  async function loadInitial() {
    loading = true;
    error = null;
    nodesMap.clear();
    edgesMap.clear();
    try {
      const data = await fetchGraph(layer);
      for (const n of data.nodes) nodesMap.set(n.id, { ...n });
      for (const e of data.edges) edgesMap.set(e.id, { ...e });
      updateGraph();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function expandNode(entityId: string) {
    try {
      const data = await fetchGraph(layer, entityId);
      let changed = false;
      for (const n of data.nodes) {
        if (!nodesMap.has(n.id)) {
          nodesMap.set(n.id, { ...n });
          changed = true;
        }
      }
      for (const e of data.edges) {
        if (!edgesMap.has(e.id)) {
          edgesMap.set(e.id, { ...e });
          changed = true;
        }
      }
      if (changed) updateGraph();
    } catch (e) {
      console.error("Failed to expand node", e);
    }
  }

  function applyFilters() {
    currentNodes = Array.from(nodesMap.values()).filter(
      n => selectedTypes.has(n.entity_type)
    );
    const nodeIds = new Set(currentNodes.map(n => n.id));
    currentEdges = Array.from(edgesMap.values()).filter(
      e => nodeIds.has(e.source) && nodeIds.has(e.target) && (e.weight === undefined || e.weight >= minWeight)
    ) as any;
  }

  function updateGraph() {
    applyFilters();
    if (!svg || !g) return;

    // Links
    const link = g.selectAll(".link")
      .data(currentEdges, (d: any) => d.id)
      .join("line")
      .attr("class", "link")
      .attr("stroke", "#334155")
      .attr("stroke-width", (d: any) => (d.weight ?? 0.5) * 3);

    // Nodes
    const node = g.selectAll(".node")
      .data(currentNodes, (d: any) => d.id)
      .join("path")
      .attr("class", "node cursor-pointer transition-colors")
      .attr("d", (d: any) => getShape(d.entity_type))
      .attr("fill", (d: any) => getColor(d.app_id))
      .attr("stroke", "#0f172a")
      .attr("stroke-width", 2)
      .on("mouseover", (event, d: any) => {
        tooltipNode = d;
        tooltipX = event.pageX;
        tooltipY = event.pageY;
      })
      .on("mousemove", (event) => {
        tooltipX = event.pageX;
        tooltipY = event.pageY;
      })
      .on("mouseout", () => {
        tooltipNode = null;
      })
      .on("click", (event, d: any) => {
        expandNode(d.id);
      })
      .call(d3.drag<any, any>()
        .on("start", (event, d) => {
          if (!event.active) simulation.alphaTarget(0.3).restart();
          d.fx = d.x;
          d.fy = d.y;
        })
        .on("drag", (event, d) => {
          d.fx = event.x;
          d.fy = event.y;
        })
        .on("end", (event, d) => {
          if (!event.active) simulation.alphaTarget(0);
          d.fx = null;
          d.fy = null;
        })
      );

    simulation.nodes(currentNodes);
    const forceLink = simulation.force("link") as d3.ForceLink<any, any>;
    forceLink.links(currentEdges);
    simulation.alpha(1).restart();

    simulation.on("tick", () => {
      link
        .attr("x1", (d: any) => d.source.x)
        .attr("y1", (d: any) => d.source.y)
        .attr("x2", (d: any) => d.target.x)
        .attr("y2", (d: any) => d.target.y);

      node
        .attr("transform", (d: any) => `translate(${d.x},${d.y})`);
    });
  }

  function initD3() {
    const width = container.clientWidth;
    const height = container.clientHeight;

    svg = d3.select(container)
      .append("svg")
      .attr("width", "100%")
      .attr("height", "100%")
      .attr("viewBox", [0, 0, width, height]);

    g = svg.append("g");

    zoom = d3.zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.1, 4])
      .on("zoom", (event) => {
        g.attr("transform", event.transform);
      });

    svg.call(zoom);
    svg.on("dblclick.zoom", null); // Disable double click zoom

    simulation = d3.forceSimulation()
      .force("link", d3.forceLink().id((d: any) => d.id).distance(100))
      .force("charge", d3.forceManyBody().strength(-300))
      .force("center", d3.forceCenter(width / 2, height / 2))
      .force("x", d3.forceX(width / 2).strength(0.1))
      .force("y", d3.forceY(height / 2).strength(0.1));
  }

  export function fitToScreen() {
    if (!svg || !g) return;
    svg.transition().duration(750).call(zoom.transform, d3.zoomIdentity);
  }

  $effect(() => {
    // Whenever layer changes, we reload
    loadInitial();
  });

  $effect(() => {
    // Re-apply filters when they change
    updateGraph();
  });

  onMount(() => {
    initD3();
    const ro = new ResizeObserver(() => {
      if (!container || !svg || !simulation) return;
      const w = container.clientWidth;
      const h = container.clientHeight;
      svg.attr("viewBox", [0, 0, w, h]);
      (simulation.force("center") as any).x(w / 2).y(h / 2);
      simulation.alpha(0.3).restart();
    });
    ro.observe(container);
    return () => ro.disconnect();
  });

  function toggleType(t: string) {
    if (selectedTypes.has(t)) selectedTypes.delete(t);
    else selectedTypes.add(t);
    // Svelte 5 trigger reactivity for Set
    selectedTypes = new Set(selectedTypes);
  }
</script>

<div class="flex h-full w-full flex-col lg:flex-row rounded-lg border border-zinc-800 bg-zinc-950/60 overflow-hidden relative text-zinc-100">
  
  <!-- Sidebar Filters -->
  <aside class="w-full lg:w-64 border-b lg:border-b-0 lg:border-r border-zinc-800 bg-zinc-900/50 p-4 flex flex-col gap-4 overflow-y-auto">
    <div>
      <h3 class="text-sm font-semibold mb-2">Layers</h3>
      <div class="flex flex-col gap-1">
        {#each ["memory", "code", "ecosystem"] as l}
          <button
            class="px-3 py-1.5 text-left text-sm rounded transition-colors {layer === l ? 'bg-cyan-900/40 text-cyan-200' : 'hover:bg-zinc-800 text-zinc-400'}"
            onclick={() => layer = l as any}
          >
            {l}
          </button>
        {/each}
      </div>
    </div>

    <div>
      <h3 class="text-sm font-semibold mb-2">Node Types</h3>
      <div class="flex flex-col gap-1 text-sm text-zinc-400">
        {#each ["entity", "code_symbol", "commit", "decision"] as t}
          <label class="flex items-center gap-2 cursor-pointer hover:text-zinc-200">
            <input type="checkbox" checked={selectedTypes.has(t)} onchange={() => toggleType(t)} class="rounded border-zinc-700 bg-zinc-800 text-cyan-500 focus:ring-cyan-500" />
            <span class="capitalize">{t.replace("_", " ")}</span>
          </label>
        {/each}
      </div>
    </div>

    <div>
      <h3 class="text-sm font-semibold mb-2">Min Edge Weight</h3>
      <div class="flex items-center gap-2">
        <input type="range" min="0" max="1" step="0.1" bind:value={minWeight} class="w-full accent-cyan-500" />
        <span class="text-xs text-zinc-400 w-6 text-right">{minWeight.toFixed(1)}</span>
      </div>
    </div>
    
    <div class="mt-auto">
      <button class="w-full py-2 bg-zinc-800 hover:bg-zinc-700 text-sm rounded transition-colors" onclick={fitToScreen}>
        Fit to Screen
      </button>
    </div>
  </aside>

  <!-- Graph Canvas -->
  <div class="flex-1 relative min-h-[500px]">
    {#if loading}
      <div class="absolute inset-0 flex items-center justify-center bg-zinc-950/20 z-10">
        <span class="animate-pulse text-cyan-500 text-sm font-semibold tracking-widest">LOADING GRAPH...</span>
      </div>
    {/if}
    {#if error}
      <div class="absolute inset-0 flex items-center justify-center bg-zinc-950/80 z-10">
        <p class="text-red-400 text-sm">Error: {error}</p>
      </div>
    {/if}
    
    <div bind:this={container} class="w-full h-full cursor-move outline-none"></div>

    <!-- Tooltip -->
    {#if tooltipNode}
      <div
        class="absolute pointer-events-none z-20 rounded shadow-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-xs"
        style="left: {tooltipX + 15}px; top: {tooltipY + 15}px;"
      >
        <p class="font-bold text-white mb-1">{tooltipNode.label}</p>
        <p class="text-zinc-400">Type: {tooltipNode.entity_type}</p>
        {#if tooltipNode.app_id}
          <p class="text-zinc-400">App: {tooltipNode.app_id}</p>
        {/if}
        {#if tooltipNode.trust_score !== undefined}
          <p class="text-zinc-400">Trust: {tooltipNode.trust_score}</p>
        {/if}
        {#if tooltipNode.memory_count !== undefined}
          <p class="text-zinc-400">Memory Count: {tooltipNode.memory_count}</p>
        {/if}
      </div>
    {/if}
  </div>
</div>
