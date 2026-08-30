<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    initLiveFeed,
    disposeLiveFeed,
    getLiveStatus,
    getLiveEventCount,
    type LiveStatus,
  } from "../lib/liveStore.svelte";

  let status = $state<LiveStatus>("offline");
  let eventCount = $state(0);

  // Poll reactive state (Svelte 5 runes are function-based, so we sync)
  let pollInterval: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    initLiveFeed();
    pollInterval = setInterval(() => {
      status = getLiveStatus();
      eventCount = getLiveEventCount();
    }, 500);
  });

  onDestroy(() => {
    if (pollInterval) clearInterval(pollInterval);
    disposeLiveFeed();
  });

  const statusColors: Record<LiveStatus, string> = {
    connected: "bg-emerald-500",
    reconnecting: "bg-amber-500",
    offline: "bg-red-500",
  };

  const statusLabels: Record<LiveStatus, string> = {
    connected: "Connected",
    reconnecting: "Reconnecting",
    offline: "Offline",
  };

  const pulseClass: Record<LiveStatus, string> = {
    connected: "animate-pulse",
    reconnecting: "animate-pulse",
    offline: "",
  };
</script>

<div class="flex items-center gap-2 text-xs select-none">
  <span class="relative flex h-2.5 w-2.5">
    {#if status !== "offline"}
      <span
        class="absolute inline-flex h-full w-full rounded-full opacity-75 {statusColors[status]} {pulseClass[status]}"
      ></span>
    {/if}
    <span
      class="relative inline-flex h-2.5 w-2.5 rounded-full {statusColors[status]}"
    ></span>
  </span>
  <span class="text-zinc-400">{statusLabels[status]}</span>
  {#if eventCount > 0}
    <span class="text-zinc-600 tabular-nums">· {eventCount}</span>
  {/if}
</div>
