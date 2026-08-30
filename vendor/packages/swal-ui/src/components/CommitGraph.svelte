<script lang="ts">
  import { onMount } from "svelte";
  import * as d3 from "d3";
  import {
    type GraphNodeData,
    type GraphLink,
    type CommitNode,
  } from "../lib/commitClient";

  // ── Props ───────────────────────────────────────────────────────────────
  let {
    nodes = $bindable([]),
    links = $bindable([]),
    onCommitClick,
  }: {
    nodes: GraphNodeData[];
    links: GraphLink[];
    onCommitClick?: (node: CommitNode) => void;
  } = $props();

  // ── DOM refs ────────────────────────────────────────────────────────────
  let container: HTMLDivElement;
  let svg: d3.Selection<SVGSVGElement, unknown, null, undefined>;
  let g: d3.Selection<SVGGElement, unknown, null, undefined>;
  let simulation: d3.Simulation<any, undefined>;
  let zoom: d3.ZoomBehavior<SVGSVGElement, unknown>;

  // ── Local state ─────────────────────────────────────────────────────────
  let selectedRepos = $state<Set<string>>(new Set());
  let selectedTypes = $state<Set<string>>(new Set(["commit", "symbol", "entity"]));
  let loading = $state(false);
  let error = $state<string | null>(null);

  // Tooltip
  let tooltipNode = $state<GraphNodeData | null>(null);
  let tooltipX = $state(0);
  let tooltipY = $state(0);

  // ── Computed filtered data ──────────────────────────────────────────────
  let filteredNodes = $derived(
    nodes.filter((n) => {
      if (!selectedTypes.has(n.type)) return false;
      if (selectedRepos.size > 0 && n.repo && !selectedRepos.has(n.repo)) return false;
      return true;
    }),
  );

  let filteredNodeIds = $derived(new Set(filteredNodes.map((n) => n.id)));

  let filteredLinks = $derived(
    links.filter(
      (l) =>
        filteredNodeIds.has(l.source as string) &&
        filteredNodeIds.has(l.target as string),
    ),
  );

  // ── Colors ──────────────────────────────────────────────────────────────
  const TYPE_COLORS: Record<string, string> = {
    commit: "#06b6d4",    // hive-cyan
    symbol: "#8b5cf6",    // violet-500
    entity: "#10b981",    // emerald-500
    decision: "#f59e0b",  // amber-500
  };

  function getNodeColor(n: GraphNodeData): string {
    return TYPE_COLORS[n.type] ?? "#475569";
  }

  function getNodeRadius(n: GraphNodeData): number {
    if (n.type === "commit") {
      const c = n as CommitNode;
      const impact = (c.lines_added ?? 0) + (c.lines_deleted ?? 0);
      return Math.min(8 + Math.sqrt(impact) * 0.4, 28);
    }
    if (n.type === "symbol") {
      return Math.min(5 + (n.connections ?? 1) * 1.5, 18);
    }
    return 7;
  }

  // ── D3 init ─────────────────────────────────────────────────────────────
  function initD3() {
    const width = container.clientWidth;
    const height = container.clientHeight;

    svg = d3
      .select(container)
      .append("svg")
      .attr("width", "100%")
      .attr("height", "100%")
      .attr("viewBox", [0, 0, width, height]);

    g = svg.append("g");

    zoom = d3
      .zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.1, 4])
      .on("zoom", (event) => {
        g.attr("transform", event.transform);
      });

    svg.call(zoom);
    svg.on("dblclick.zoom", null);

    simulation = d3
      .forceSimulation()
      .force(
        "link",
        d3
          .forceLink()
          .id((d: any) => d.id)
          .distance(80),
      )
      .force("charge", d3.forceManyBody().strength(-250))
      .force("center", d3.forceCenter(width / 2, height / 2))
      .force("x", d3.forceX(width / 2).strength(0.08))
      .force("y", d3.forceY(height / 2).strength(0.08))
      .force(
        "collision",
        d3.forceCollide().radius((d: any) => getNodeRadius(d) + 3),
      );
  }

  // ── Render / update ─────────────────────────────────────────────────────
  function updateGraph() {
    if (!svg || !g || !simulation) return;

    const simNodes = filteredNodes.map((n) => ({ ...n })) as any[];
    const simLinks = filteredLinks.map((l) => ({ ...l })) as any[];

    // Links
    const link = g
      .selectAll<SVGLineElement, any>(".commit-link")
      .data(simLinks, (d: any) => `${d.source?.id ?? d.source}-${d.target?.id ?? d.target}`)
      .join("line")
      .attr("class", "commit-link")
      .attr("stroke", (d: any) => {
        if (d.type === "commit_file") return "#06b6d4";
        if (d.type === "file_symbol") return "#8b5cf6";
        return "#334155";
      })
      .attr("stroke-opacity", 0.4)
      .attr("stroke-width", (d: any) => (d.weight ?? 0.5) * 2.5);

    // Nodes
    const node = g
      .selectAll<SVGCircleElement | SVGPathElement, any>(".commit-node")
      .data(simNodes, (d: any) => d.id)
      .join(
        (enter) => {
          const merged = enter.append("g").attr("class", "commit-node");
          merged.append("circle");
          merged.append("text");
          return merged;
        },
      )
      .attr("cursor", (d: any) => (d.type === "commit" ? "pointer" : "default"));

    node
      .select("circle")
      .attr("r", (d: any) => getNodeRadius(d))
      .attr("fill", (d: any) => getNodeColor(d))
      .attr("stroke", "#0f172a")
      .attr("stroke-width", 2)
      .attr("filter", (d: any) =>
        d.type === "commit" ? "url(#glow)" : null,
      );

    node
      .select("text")
      .attr("text-anchor", "middle")
      .attr("dy", "0.35em")
      .attr("fill", "#e2e8f0")
      .attr("font-size", (d: any) => (d.type === "commit" ? "8px" : "6px"))
      .attr("pointer-events", "none")
      .text((d: any) => (d.type === "commit" ? d.short_hash : "").substring(0, 6));

    // Tooltip
    node
      .on("mouseover", (event: MouseEvent, d: any) => {
        tooltipNode = d;
        tooltipX = event.pageX;
        tooltipY = event.pageY;
      })
      .on("mousemove", (event: MouseEvent) => {
        tooltipX = event.pageX;
        tooltipY = event.pageY;
      })
      .on("mouseout", () => {
        tooltipNode = null;
      })
      .on("click", (event: MouseEvent, d: any) => {
        if (d.type === "commit" && onCommitClick) {
          onCommitClick(d as CommitNode);
        }
      });

    // Drag
    node.call(
      d3
        .drag<any, any>()
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
        }),
    );

    // Simulation
    simulation.nodes(simNodes);
    const forceLink = simulation.force("link") as d3.ForceLink<any, any>;
    forceLink.links(simLinks);
    simulation.alpha(0.8).restart();

    simulation.on("tick", () => {
      link
        .attr("x1", (d: any) => d.source.x)
        .attr("y1", (d: any) => d.source.y)
        .attr("x2", (d: any) => d.target.x)
        .attr("y2", (d: any) => d.target.y);

      node.attr("transform", (d: any) => `translate(${d.x},${d.y})`);
    });
  }

  // ── Public API ──────────────────────────────────────────────────────────
  export function fitToScreen() {
    if (!svg || !g) return;
    svg.transition().duration(750).call(zoom.transform, d3.zoomIdentity);
  }

  export function injectNode(n: GraphNodeData) {
    // Called from WebSocket live stream to add a commit in real-time
    const existing = nodes.find((x) => x.id === n.id);
    if (!existing) {
      nodes = [...nodes, n];
    }
  }

  // ── Lifecycle ───────────────────────────────────────────────────────────
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

    updateGraph();
    return () => ro.disconnect();
  });

  $effect(() => {
    // Re-render when filtered data changes
    filteredNodes; // track
    filteredLinks; // track
    updateGraph();
  });

  function toggleType(t: string) {
    if (selectedTypes.has(t)) selectedTypes.delete(t);
    else selectedTypes.add(t);
    selectedTypes = new Set(selectedTypes);
  }
</script>

<!-- SVG Defs for glow filter -->
<svg width="0" height="0" class="absolute">
  <defs>
    <filter id="glow">
      <feGaussianBlur stdDeviation="2.5" result="blur" />
      <feMerge>
        <feMergeNode in="blur" />
        <feMergeNode in="SourceGraphic" />
      </feMerge>
    </filter>
  </defs>
</svg>

<div
  class="flex h-full w-full flex-col lg:flex-row rounded-lg border border-zinc-800 bg-zinc-950/60 overflow-hidden relative text-zinc-100"
>
  <!-- Sidebar Filters -->
  <aside
    class="w-full lg:w-56 border-b lg:border-b-0 lg:border-r border-zinc-800 bg-zinc-900/50 p-4 flex flex-col gap-4 overflow-y-auto"
  >
    <div>
      <h3 class="text-sm font-semibold mb-2">Node Types</h3>
      <div class="flex flex-col gap-1 text-sm text-zinc-400">
        {#each ["commit", "symbol", "entity"] as t}
          <label class="flex items-center gap-2 cursor-pointer hover:text-zinc-200">
            <input
              type="checkbox"
              checked={selectedTypes.has(t)}
              onchange={() => toggleType(t)}
              class="rounded border-zinc-700 bg-zinc-800 text-cyan-500 focus:ring-cyan-500"
            />
            <span class="capitalize">{t}</span>
          </label>
        {/each}
      </div>
    </div>

    <div class="mt-auto">
      <button
        class="w-full py-2 bg-zinc-800 hover:bg-zinc-700 text-sm rounded transition-colors"
        onclick={fitToScreen}
      >
        Fit to Screen
      </button>
    </div>
  </aside>

  <!-- Graph Canvas -->
  <div class="flex-1 relative min-h-[500px]">
    {#if loading}
      <div
        class="absolute inset-0 flex items-center justify-center bg-zinc-950/20 z-10"
      >
        <span
          class="animate-pulse text-cyan-500 text-sm font-semibold tracking-widest"
          >LOADING COMMIT GRAPH...</span
        >
      </div>
    {/if}
    {#if error}
      <div
        class="absolute inset-0 flex items-center justify-center bg-zinc-950/80 z-10"
      >
        <div class="text-center space-y-2">
          <p class="text-amber-400 text-sm font-semibold">Graph unavailable</p>
          <p class="text-zinc-500 text-xs">{error}</p>
          <p class="text-zinc-600 text-[10px]"
            >Waiting for Xavier backend — commit graph will render once the
            endpoint is live.</p
          >
        </div>
      </div>
    {/if}

    <div bind:this={container} class="w-full h-full cursor-move outline-none"></div>

    <!-- Tooltip -->
    {#if tooltipNode}
      <div
        class="absolute pointer-events-none z-20 rounded shadow-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-xs max-w-64"
        style="left: {tooltipX + 15}px; top: {tooltipY + 15}px;"
      >
        {#if tooltipNode.type === "commit"}
          <p class="font-bold text-cyan-400 mb-1 font-mono"
            >{(tooltipNode as CommitNode).short_hash}</p
          >
          <p class="text-zinc-300 text-[11px]">{tooltipNode.message}</p>
          {#if tooltipNode.repo}
            <p class="text-zinc-500 mt-1">repo: {tooltipNode.repo}</p>
          {/if}
          {#if (tooltipNode as CommitNode).lines_added !== undefined}
            <p class="text-emerald-400"
              >+{(tooltipNode as CommitNode).lines_added} / -{(tooltipNode as CommitNode).lines_deleted}</p
            >
          {/if}
        {:else}
          <p class="font-bold text-zinc-200 mb-1">{tooltipNode.label}</p>
          <p class="text-zinc-400">Type: {tooltipNode.type}</p>
          {#if tooltipNode.repo}
            <p class="text-zinc-500">repo: {tooltipNode.repo}</p>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
</div>
