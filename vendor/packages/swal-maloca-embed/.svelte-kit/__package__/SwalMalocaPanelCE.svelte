<script lang="ts">
  import MalocaPanelActual from './MalocaPanel.svelte';

  const MalocaPanel = MalocaPanelActual as any;

  let { appId, compact = false, xavierUrl } = $props<{
    appId: string;
    compact?: boolean;
    xavierUrl?: string;
  }>();

  let container = $state<HTMLElement | null>(null);

  function dispatch(name: string, detail: any) {
    if (container) {
      const root = container.getRootNode();
      const host = root && 'host' in root ? (root as any).host : null;
      const target = host || container;
      target.dispatchEvent(
        new CustomEvent(name, {
          bubbles: true,
          composed: true,
          detail
        })
      );
    }
  }

  function handleReady(e: any) {
    dispatch('maloca-ready', e?.detail || e);
  }

  function handleError(e: any) {
    dispatch('maloca-error', e?.detail || e);
  }
</script>

<svelte:options
  customElement={{
    tag: 'swal-maloca-panel',
    props: {
      appId: { attribute: 'app-id' },
      compact: { attribute: 'compact', type: 'Boolean' },
      xavierUrl: { attribute: 'xavier-url' }
    },
    shadow: 'open'
  }}
/>

<div bind:this={container} style="display: contents;">
  <MalocaPanel
    {appId}
    {compact}
    {xavierUrl}
    onready={handleReady}
    onerror={handleError}
    onmalocaready={handleReady}
    onmalocaerror={handleError}
  />
</div>
