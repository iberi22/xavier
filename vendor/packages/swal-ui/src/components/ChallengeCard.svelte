<script lang="ts">
  import Badge from './Badge.svelte';
  import Card from './Card.svelte';

  // Svelte 5 runes syntax
  let {
    challenge,
    active = false,
    onselect
  } = $props();

  // Mapping types to readable text and variants
  const kindConfig = {
    contradiction: { label: 'Contradicción', variant: 'danger' },
    recall: { label: 'Recuerdo', variant: 'info' },
    alternate_scenario: { label: 'Escenario Alterno', variant: 'orange' },
    sandbox: { label: 'Demo / Sandbox', variant: 'neutral' },
    coherence: { label: 'Coherencia', variant: 'warning' }
  };

  const statusConfig = {
    pending: { label: 'Pendiente', variant: 'warning' },
    answered: { label: 'Respondido', variant: 'info' },
    scored: { label: 'Evaluado', variant: 'success' },
    expired: { label: 'Expirado', variant: 'neutral' },
    skipped: { label: 'Omitido', variant: 'neutral' }
  };

  const kindInfo = $derived(kindConfig[challenge.kind] || { label: challenge.kind, variant: 'neutral' });
  const statusInfo = $derived(statusConfig[challenge.status] || { label: challenge.status, variant: 'neutral' });
  const difficultyPercent = $derived(Math.round((challenge.difficulty || 0) * 100));
</script>

<div class="challenge-card-wrapper" class:active>
  <Card
    variant={active ? 'surface' : 'default'}
    hoverable={true}
    onclick={() => onselect?.(challenge)}
    padding="sm"
  >
    <div class="card-header">
      <Badge variant={kindInfo.variant} size="sm">
        {kindInfo.label}
      </Badge>
      <Badge variant={statusInfo.variant} size="sm">
        {statusInfo.label}
      </Badge>
    </div>

    <p class="prompt-preview">{challenge.prompt}</p>

    <div class="card-footer">
      <span class="difficulty-indicator">
        Dificultad: <span class="difficulty-value" style="color: var(--swal-accent-orange)">{difficultyPercent}%</span>
      </span>
      {#if challenge.expiresAt}
        <span class="expiry-date">
          Expira: {new Date(challenge.expiresAt).toLocaleDateString()}
        </span>
      {/if}
    </div>
  </Card>
</div>

<style>
  .challenge-card-wrapper {
    width: 100%;
    border-radius: var(--swal-radius);
    transition: box-shadow var(--swal-transition-fast);
  }
  .challenge-card-wrapper.active {
    box-shadow: var(--swal-shadow-neon-cyan);
    border: 1px solid var(--swal-accent);
  }
  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--swal-space-2);
    gap: var(--swal-space-2);
  }
  .prompt-preview {
    margin: 0 0 var(--swal-space-3);
    font-size: var(--swal-font-size-sm);
    color: var(--swal-text);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    line-height: 1.4;
  }
  .card-footer {
    display: flex;
    justify-content: space-between;
    font-size: var(--swal-font-size-xs);
    color: var(--swal-text-muted);
    font-family: var(--swal-font-mono);
  }
  .difficulty-value {
    font-weight: 600;
  }
</style>
