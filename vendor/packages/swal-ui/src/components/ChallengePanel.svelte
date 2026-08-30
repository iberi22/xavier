<script lang="ts">
  import { onMount } from 'svelte';
  import ChallengeCard from './ChallengeCard.svelte';
  import ChallengeForm from './ChallengeForm.svelte';
  import Card from './Card.svelte';
  import Badge from './Badge.svelte';
  import Button from './Button.svelte';

  // Svelte 5 runes syntax
  let {
    instanceId,
    challenges = $bindable([]),
    responses = $bindable({}), // map of challengeId -> ChallengeResponse
    onrespond, // callback async (challengeId, answerText) => Promise<ChallengeResponse>
    optOut = $bindable(false),
    onToggleOptOut // callback async (enabled) => Promise<void>
  } = $props();

  let selectedChallenge = $state<any>(null);
  let filterKind = $state<string>('all');
  let filterStatus = $state<string>('all');
  let submitting = $state(false);

  // Filter lists
  const kinds = ['all', 'contradiction', 'recall', 'alternate_scenario', 'sandbox', 'coherence'];
  const statuses = ['all', 'pending', 'answered', 'scored', 'expired', 'skipped'];

  const kindLabels = {
    all: 'Todos los tipos',
    contradiction: 'Contradicción',
    recall: 'Recuerdo',
    alternate_scenario: 'Escenario Alterno',
    sandbox: 'Demo / Sandbox',
    coherence: 'Coherencia'
  };

  const statusLabels = {
    all: 'Todos los estados',
    pending: 'Pendientes',
    answered: 'Respondidos',
    scored: 'Evaluados',
    expired: 'Expirados',
    skipped: 'Omitidos'
  };

  const filteredChallenges = $derived(
    challenges.filter((c: any) => {
      const matchKind = filterKind === 'all' || c.kind === filterKind;
      const matchStatus = filterStatus === 'all' || c.status === filterStatus;
      return matchKind && matchStatus;
    })
  );

  async function handleRespond(answerText: string) {
    if (!selectedChallenge) return;
    submitting = true;
    try {
      if (onrespond) {
        const res = await onrespond(selectedChallenge.id, answerText);
        responses[selectedChallenge.id] = res;

        // Update local challenge status
        challenges = challenges.map((c: any) =>
          c.id === selectedChallenge.id
            ? { ...c, status: res.scores ? 'scored' : 'answered' }
            : c
        );

        // Refresh selected reference with updated data
        selectedChallenge = challenges.find((c: any) => c.id === selectedChallenge.id) || selectedChallenge;
      }
    } catch (err) {
      console.error("Error submitting response", err);
    } finally {
      submitting = false;
    }
  }

  async function handleToggleOptOut() {
    const nextVal = !optOut;
    try {
      if (onToggleOptOut) {
        await onToggleOptOut(nextVal);
      }
      optOut = nextVal;
    } catch (err) {
      console.error("Error changing opt-out state", err);
    }
  }

  // Auto-select first challenge on mount if available
  onMount(() => {
    if (filteredChallenges.length > 0 && !selectedChallenge) {
      selectedChallenge = filteredChallenges[0];
    }
  });
</script>

<div class="challenge-panel-layout swal-grid-bg swal-dvh">
  <!-- Top header bar -->
  <header class="panel-header">
    <div class="header-left">
      <h1 class="panel-title swal-text swal-neon-cyan">Panel de Retos de Entrenamiento</h1>
      <span class="instance-badge">Instancia: <code>{instanceId || 'unknown'}</code></span>
    </div>
    <div class="header-right">
      <Button
        variant={optOut ? 'danger' : 'secondary'}
        size="sm"
        onclick={handleToggleOptOut}
      >
        {optOut ? 'Opt-Out Activo (Habilitar)' : 'Desactivar Retos (Opt-Out)'}
      </Button>
    </div>
  </header>

  {#if optOut}
    <div class="opt-out-banner">
      <Card variant="glass" padding="lg">
        <h2 class="swal-text swal-neon-orange">El entrenamiento humano (Opt-Out) está desactivado</h2>
        <p class="swal-text-secondary">
          Has decidido no participar en los retos interactivos de validación de modelos.
          Los generadores automáticos no crearán nuevos retos y el almacenamiento de resultados está restringido.
        </p>
        <div style="margin-top: var(--swal-space-4)">
          <Button variant="orange" onclick={handleToggleOptOut}>Habilitar Entrenamiento Humano</Button>
        </div>
      </Card>
    </div>
  {:else}
    <div class="panel-content">
      <!-- Sidebar / List of challenges -->
      <aside class="challenge-sidebar">
        <!-- Filters card -->
        <Card variant="surface" padding="sm" class="filters-card">
          <h2 class="sidebar-title swal-text-secondary">Filtros</h2>
          <div class="filter-controls">
            <div class="filter-field">
              <label for="filterKind" class="swal-text-muted">Tipo:</label>
              <select id="filterKind" class="swal-select" bind:value={filterKind}>
                {#each kinds as k}
                  <option value={k}>{kindLabels[k]}</option>
                {/each}
              </select>
            </div>
            <div class="filter-field">
              <label for="filterStatus" class="swal-text-muted">Estado:</label>
              <select id="filterStatus" class="swal-select" bind:value={filterStatus}>
                {#each statuses as s}
                  <option value={s}>{statusLabels[s]}</option>
                {/each}
              </select>
            </div>
          </div>
        </Card>

        <!-- Challenge list -->
        <div class="challenges-list swal-scrollbar">
          {#if filteredChallenges.length === 0}
            <div class="no-challenges swal-text-muted">
              No se encontraron retos con los filtros seleccionados.
            </div>
          {:else}
            {#each filteredChallenges as c (c.id)}
              <ChallengeCard
                challenge={c}
                active={selectedChallenge?.id === c.id}
                onselect={(sel) => selectedChallenge = sel}
              />
            {/each}
          {/if}
        </div>
      </aside>

      <!-- Main viewport for active challenge -->
      <main class="challenge-viewport">
        {#if !selectedChallenge}
          <div class="placeholder-view">
            <Card variant="glass" padding="lg">
              <span class="placeholder-icon">✦</span>
              <p class="swal-text-secondary">Selecciona un reto de la lista lateral para responder, auditar sus fuentes o consultar resultados de evaluación.</p>
            </Card>
          </div>
        {:else}
          <div class="selected-challenge-layout">
            <!-- Header card -->
            <Card variant="surface" padding="md">
              <div class="challenge-meta">
                <Badge variant="orange" size="md">Reto Activo</Badge>
                <span class="id-tag">ID: <code>{selectedChallenge.id}</code></span>
              </div>
              <h2 class="challenge-prompt swal-text">{selectedChallenge.prompt}</h2>
            </Card>

            <!-- Sources / Context details -->
            {#if selectedChallenge.sourceRefs && selectedChallenge.sourceRefs.length > 0}
              <Card variant="default" padding="sm">
                <h3 class="section-heading swal-text-secondary">Referencias y Fuentes de Contexto</h3>
                <div class="source-refs-list">
                  {#each selectedChallenge.sourceRefs as ref}
                    <div class="source-ref-card">
                      <div class="ref-header">
                        <Badge variant="neutral" size="sm">{ref.refType}</Badge>
                        <span class="ref-id">ID: <code>{ref.id}</code></span>
                      </div>
                      {#if ref.snippet}
                        <blockquote class="ref-snippet">"{ref.snippet}"</blockquote>
                      {/if}
                    </div>
                  {/each}
                </div>
              </Card>
            {/if}

            <!-- Response Form / Evaluation -->
            <Card variant="surface" padding="md">
              <ChallengeForm
                challenge={selectedChallenge}
                onrespond={handleRespond}
                {submitting}
                response={responses[selectedChallenge.id]}
              />
            </Card>
          </div>
        {/if}
      </main>
    </div>
  {/if}
</div>

<style>
  .challenge-panel-layout {
    display: flex;
    flex-direction: column;
    height: 100vh;
    height: 100dvh;
    color: var(--swal-text);
    font-family: var(--swal-font);
    box-sizing: border-box;
    padding: var(--swal-space-4);
    gap: var(--swal-space-4);
  }
  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--swal-border);
    padding-bottom: var(--swal-space-3);
    flex-shrink: 0;
  }
  .panel-title {
    margin: 0;
    font-size: var(--swal-font-size-lg);
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .instance-badge {
    font-size: var(--swal-font-size-xs);
    color: var(--swal-text-muted);
  }
  .instance-badge code {
    color: var(--swal-text-secondary);
  }
  .panel-content {
    display: flex;
    flex: 1;
    min-height: 0;
    gap: var(--swal-space-4);
  }
  .challenge-sidebar {
    width: 320px;
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-3);
    flex-shrink: 0;
    min-height: 0;
  }
  .sidebar-title {
    font-size: var(--swal-font-size-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 var(--swal-space-2);
  }
  .filter-controls {
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-2);
  }
  .filter-field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--swal-space-2);
  }
  .filter-field label {
    font-size: var(--swal-font-size-xs);
    font-weight: 500;
  }
  .swal-select {
    background: var(--swal-surface);
    border: 1px solid var(--swal-border);
    border-radius: var(--swal-radius-sm);
    color: var(--swal-text);
    padding: var(--swal-space-1) var(--swal-space-2);
    font-size: var(--swal-font-size-xs);
    outline: none;
    cursor: pointer;
  }
  .swal-select:focus {
    border-color: var(--swal-accent);
  }
  .challenges-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-2);
    padding-right: var(--swal-space-1);
  }
  .no-challenges {
    text-align: center;
    padding: var(--swal-space-6);
    font-size: var(--swal-font-size-xs);
  }
  .challenge-viewport {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .placeholder-view {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
  }
  .placeholder-icon {
    font-size: 3rem;
    color: var(--swal-accent);
    display: block;
    margin-bottom: var(--swal-space-4);
    text-align: center;
  }
  .selected-challenge-layout {
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-4);
  }
  .challenge-meta {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--swal-space-2);
  }
  .id-tag {
    font-size: var(--swal-font-size-xs);
    color: var(--swal-text-muted);
  }
  .id-tag code {
    color: var(--swal-text-secondary);
  }
  .challenge-prompt {
    margin: 0;
    font-size: var(--swal-font-size-lg);
    line-height: 1.4;
  }
  .section-heading {
    font-size: var(--swal-font-size-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 var(--swal-space-2);
  }
  .source-refs-list {
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-2);
  }
  .source-ref-card {
    background: rgba(0, 0, 0, 0.2);
    border: 1px solid var(--swal-border);
    border-radius: var(--swal-radius-sm);
    padding: var(--swal-space-2);
  }
  .ref-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--swal-space-1);
  }
  .ref-id {
    font-size: var(--swal-font-size-xs);
    color: var(--swal-text-muted);
  }
  .ref-snippet {
    margin: 0;
    font-size: var(--swal-font-size-xs);
    color: var(--swal-text-secondary);
    font-style: italic;
    background: rgba(255, 255, 255, 0.02);
    padding: var(--swal-space-1) var(--swal-space-2);
    border-radius: var(--swal-radius-sm);
  }
  .opt-out-banner {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    text-align: center;
    max-width: 600px;
    margin: 0 auto;
  }
  code {
    font-family: var(--swal-font-mono);
    font-size: 0.9em;
    background: rgba(255, 255, 255, 0.05);
    padding: 1px 4px;
    border-radius: var(--swal-radius-sm);
  }
</style>
