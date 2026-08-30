<script lang="ts">
  import { onMount } from 'svelte';
  import '@swal/ui/tokens';
  import { normalizeBacklog, normalizePack, type BacklogItem, type PackInfo } from './wasm.js';

  let { appId, compact = false } = $props<{
    appId: string;
    compact?: boolean;
  }>();

  let loading = $state(true);
  let backlogItems = $state<BacklogItem[]>([]);
  let error = $state<string | null>(null);
  let packInfo = $state<PackInfo | null>(null);

  // Xavier base URL: same host/port as the app that embeds this panel.
  function xavierBase(): string {
    const explicit = (import.meta as any).env?.VITE_XAVIER_URL;
    if (explicit) return explicit;
    return `${window.location.protocol}//${window.location.hostname}:8006`;
  }

  onMount(async () => {
    try {
      const base = xavierBase();
      // Backlog real del ecosistema desde Xavier /maloca/backlog
      const backlogRes = await fetch(`${base}/maloca/backlog`);
      const backlog = await backlogRes.json();

      // Agregar pack info (features totales + decisiones)
      const packRes = await fetch(`${base}/maloca/pack`);
      const pack = packRes.ok ? await packRes.json() : null;

      // Business logic has been delegated to WASM / TS Fallback singleton in wasm.ts
      backlogItems = await normalizeBacklog(backlog, appId);
      packInfo = await normalizePack(pack);

      loading = false;
    } catch (e) {
      error = String(e);
      loading = false;
    }
  });
</script>

<div class="swal-embed-container {compact ? 'compact' : ''}">
  <header>
    <h3>Maloca Core Tracking</h3>
    <span class="badge">{appId}</span>
  </header>

  {#if packInfo}
    <div class="pack-strip">
      <span class="pack-item">🧩 {packInfo.features} features</span>
      <span class="pack-item">⚖️ {packInfo.decisions} decisiones</span>
    </div>
  {/if}

  <div class="content">
    {#if loading}
      <div class="loading">Loading ecosystem data...</div>
    {:else if error}
      <div class="error">Failed to load: {error}</div>
    {:else if backlogItems.length === 0}
      <div class="empty">Sin items para {appId} en el backlog del ecosistema.</div>
    {:else}
      <ul class="backlog-list">
        {#each backlogItems as item}
          <li>
            <span class="item-id">#{item.id}</span>
            <span class="item-title">{item.title}</span>
            <span class="item-status">{item.status}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  /* Encapsulated component styles that utilize @swal/ui tokens */
  .swal-embed-container {
    background-color: var(--swal-bg, #0f172a);
    color: var(--swal-fg, #e2e8f0);
    border: 1px solid var(--swal-border, #1e293b);
    border-radius: 8px;
    padding: 16px;
    font-family: var(--swal-font, system-ui, sans-serif);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .swal-embed-container.compact {
    padding: 8px;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--swal-border, #1e293b);
    padding-bottom: 8px;
  }

  h3 {
    margin: 0;
    font-size: 1.1rem;
    color: var(--swal-accent, #06b6d4);
  }

  .badge {
    background: var(--swal-bg-muted, #1e293b);
    padding: 2px 8px;
    border-radius: 12px;
    font-size: 0.8rem;
    color: var(--swal-fg-muted, #94a3b8);
  }

  .pack-strip {
    display: flex;
    gap: 12px;
    font-size: 0.8rem;
    color: var(--swal-fg-muted, #94a3b8);
  }

  .pack-item {
    background: var(--swal-bg-muted, #1e293b);
    padding: 3px 10px;
    border-radius: 12px;
  }

  .backlog-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .backlog-list li {
    display: flex;
    gap: 8px;
    background: var(--swal-bg-muted, #1e293b);
    padding: 8px;
    border-radius: 4px;
    font-size: 0.9rem;
  }

  .item-id {
    color: var(--swal-accent, #06b6d4);
    font-weight: bold;
  }

  .item-title {
    flex: 1;
  }

  .item-status {
    font-size: 0.8rem;
    opacity: 0.8;
  }

  .empty {
    font-size: 0.85rem;
    color: var(--swal-fg-muted, #94a3b8);
    padding: 8px 0;
  }
</style>
