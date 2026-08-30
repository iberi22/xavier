<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { onLiveEvent, type LiveEvent } from "../lib/liveStore.svelte";

  interface Toast {
    id: string;
    event: LiveEvent;
    visible: boolean;
  }

  let toasts = $state<Toast[]>([]);
  const MAX_TOASTS = 5;
  const TOAST_DURATION_MS = 6_000;

  // Event types that warrant a toast notification
  const NOTABLE_TYPES = new Set([
    "decision",
    "commit",
    "status_change",
    "error",
    "memory_created",
    "deploy",
  ]);

  let unsub: (() => void) | null = null;

  onMount(() => {
    unsub = onLiveEvent((event) => {
      // Only show toasts for notable events
      if (!NOTABLE_TYPES.has(event.type)) return;

      const toast: Toast = {
        id: event.id,
        event,
        visible: true,
      };

      toasts = [toast, ...toasts].slice(0, MAX_TOASTS);

      // Auto-dismiss after duration
      setTimeout(() => dismissToast(toast.id), TOAST_DURATION_MS);
    });
  });

  onDestroy(() => {
    unsub?.();
  });

  function dismissToast(id: string) {
    toasts = toasts.map((t) =>
      t.id === id ? { ...t, visible: false } : t
    );
    // Remove from DOM after fade-out
    setTimeout(() => {
      toasts = toasts.filter((t) => t.id !== id);
    }, 300);
  }

  function getToastColor(type: string): string {
    switch (type) {
      case "decision":
        return "border-amber-500/40 bg-amber-500/10";
      case "commit":
        return "border-emerald-500/40 bg-emerald-500/10";
      case "status_change":
        return "border-cyan-500/40 bg-cyan-500/10";
      case "error":
        return "border-red-500/40 bg-red-500/10";
      case "memory_created":
        return "border-indigo-500/40 bg-indigo-500/10";
      case "deploy":
        return "border-violet-500/40 bg-violet-500/10";
      default:
        return "border-zinc-500/40 bg-zinc-500/10";
    }
  }

  function getToastTextColor(type: string): string {
    switch (type) {
      case "decision":
        return "text-amber-300";
      case "commit":
        return "text-emerald-300";
      case "status_change":
        return "text-cyan-300";
      case "error":
        return "text-red-300";
      case "memory_created":
        return "text-indigo-300";
      case "deploy":
        return "text-violet-300";
      default:
        return "text-zinc-300";
    }
  }

  function timeAgo(ts: number): string {
    const diff = Math.floor((Date.now() - ts) / 1000);
    if (diff < 5) return "just now";
    if (diff < 60) return `${diff}s ago`;
    return `${Math.floor(diff / 60)}m ago`;
  }
</script>

{#if toasts.length > 0}
  <div class="fixed bottom-4 right-4 z-50 flex flex-col gap-2 max-w-sm">
    {#each toasts as toast (toast.id)}
      <div
        class="rounded-lg border px-4 py-3 text-sm shadow-xl backdrop-blur-sm transition-all duration-300 {getToastColor(
          toast.event.type
        )} {toast.visible
          ? 'translate-x-0 opacity-100'
          : 'translate-x-4 opacity-0'}"
      >
        <div class="flex items-start justify-between gap-2">
          <div class="flex-1 min-w-0">
            <span
              class="text-[10px] font-medium uppercase tracking-wider {getToastTextColor(
                toast.event.type
              )}"
            >
              {toast.event.type.replace("_", " ")}
            </span>
            {#if toast.event.summary}
              <p class="mt-0.5 text-xs text-zinc-300 truncate">
                {toast.event.summary}
              </p>
            {/if}
            {#if toast.event.agent}
              <span class="text-[10px] text-zinc-500">
                {toast.event.agent}
              </span>
            {/if}
          </div>
          <div class="flex items-center gap-2 shrink-0">
            <span class="text-[10px] text-zinc-600 tabular-nums">
              {timeAgo(toast.event.timestamp)}
            </span>
            <button
              type="button"
              class="text-zinc-600 hover:text-zinc-300 transition"
              onclick={() => dismissToast(toast.id)}
              aria-label="Dismiss"
            >
              ✕
            </button>
          </div>
        </div>
      </div>
    {/each}
  </div>
{/if}
