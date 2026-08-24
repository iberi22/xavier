<script>
  let { status = 'healthy', credits = null, label = '' } = $props();
</script>

<div class="model-badge badge-{status}">
  {#if label}
    <span class="badge-label">{label}</span>
  {/if}
  <span class="status-indicator status-{status}">{status}</span>
  {#if credits !== null && credits !== undefined}
    <span class="badge-credits">{credits} cr</span>
  {/if}
</div>

<style>
  .model-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0.5rem;
    border-radius: 4px;
    font-size: 0.75rem;
    font-weight: 500;
  }

  .status-indicator {
    text-transform: capitalize;
  }

  .status-healthy {
    color: #10b981;
  }

  .status-degraded {
    color: #f59e0b;
  }

  .status-offline {
    color: #ef4444;
  }

  .badge-credits {
    background: rgba(255, 255, 255, 0.1);
    padding: 0.1rem 0.3rem;
    border-radius: 2px;
  }
</style>
