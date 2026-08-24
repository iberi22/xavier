<script>
  import { createEventDispatcher } from 'svelte';
  import ModelBadge from './ModelBadge.svelte';

  const dispatch = createEventDispatcher();

  let {
    xavierUrl = 'http://localhost:8080',
    selectedStrategy = $bindable('Local'),
    selectedModel = $bindable('')
  } = $props();

  let strategies = ['Local', 'CLI', 'Clavis Cloud'];
  let models = $state([]);
  let loading = $state(false);
  let error = $state(null);
  let isOffline = $state(false);

  const fallbackModels = [
    { id: 'local-default', name: 'Local Default (llama3)', provider: 'Local', health: 'healthy', credits: 0, strategy: 'Local' },
    { id: 'cli-default', name: 'CLI Tool (ollama)', provider: 'CLI', health: 'healthy', credits: 0, strategy: 'CLI' },
    { id: 'clavis-cloud-v1', name: 'Clavis Cloud GPT-4o', provider: 'Clavis Cloud', health: 'healthy', credits: 100, strategy: 'Clavis Cloud' }
  ];

  $effect(() => {
    fetchModels(xavierUrl);
  });

  async function fetchModels(baseUrl) {
    loading = true;
    error = null;
    try {
      const response = await fetch(`${baseUrl}/v1/maloca/models/list`);
      if (!response.ok) {
        throw new Error(`HTTP error ${response.status}`);
      }
      const data = await response.json();
      models = data.models || data || [];
      isOffline = false;
    } catch (e) {
      console.warn('Failed to fetch models from API, falling back to offline mode:', e);
      isOffline = true;
      models = fallbackModels;
    } finally {
      loading = false;
    }
  }

  let filteredModels = $derived(
    models.filter(m => !selectedStrategy || m.strategy === selectedStrategy || m.provider === selectedStrategy)
  );

  let activeModel = $derived(
    models.find(m => m.id === selectedModel) || filteredModels[0] || null
  );

  function handleStrategyChange(strategy) {
    selectedStrategy = strategy;
    const available = models.filter(m => m.strategy === strategy || m.provider === strategy);
    if (available.length > 0) {
      selectedModel = available[0].id;
    }
    emitSelect();
  }

  function handleModelChange(event) {
    selectedModel = event.target.value;
    emitSelect();
  }

  function emitSelect() {
    const payload = {
      strategy: selectedStrategy,
      modelId: selectedModel,
      model: activeModel,
      isOffline
    };
    dispatch('select', payload);
  }
</script>

<div class="model-selector-container">
  <div class="strategy-picker">
    <span class="picker-label">Strategy:</span>
    {#each strategies as strategy}
      <button
        type="button"
        class="strategy-btn {selectedStrategy === strategy ? 'active' : ''}"
        onclick={() => handleStrategyChange(strategy)}
      >
        {strategy}
      </button>
    {/each}
  </div>

  <div class="model-dropdown-row">
    <label for="model-select" class="picker-label">Model:</label>
    {#if loading}
      <span class="loading-spinner">Loading models...</span>
    {:else}
      <select
        id="model-select"
        class="model-select-input"
        value={selectedModel || (activeModel ? activeModel.id : '')}
        onchange={handleModelChange}
      >
        {#each filteredModels as model}
          <option value={model.id}>{model.name || model.id}</option>
        {/each}
      </select>
    {/if}

    {#if activeModel}
      <ModelBadge
        status={activeModel.health || (isOffline ? 'offline' : 'healthy')}
        credits={activeModel.credits}
        label={isOffline ? 'Offline Mode' : 'Live API'}
      />
    {/if}
  </div>
</div>

<style>
  .model-selector-container {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    padding: 1rem;
    background: #1e1e2e;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    color: #cdd6f4;
  }

  .strategy-picker {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .picker-label {
    font-size: 0.875rem;
    font-weight: 600;
  }

  .strategy-btn {
    padding: 0.35rem 0.75rem;
    border-radius: 4px;
    border: 1px solid rgba(255, 255, 255, 0.2);
    background: transparent;
    color: #cdd6f4;
    cursor: pointer;
    font-size: 0.8rem;
    transition: all 0.2s ease;
  }

  .strategy-btn.active {
    background: #89b4fa;
    color: #11111b;
    border-color: #89b4fa;
    font-weight: 600;
  }

  .model-dropdown-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .model-select-input {
    padding: 0.4rem 0.75rem;
    border-radius: 4px;
    background: #181825;
    color: #cdd6f4;
    border: 1px solid rgba(255, 255, 255, 0.2);
    font-size: 0.85rem;
  }

  .loading-spinner {
    font-size: 0.85rem;
    color: #a6adc8;
  }
</style>
