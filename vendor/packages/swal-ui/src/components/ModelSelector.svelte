<script>
  import ModelBadge from './ModelBadge.svelte';

  let {
    // Lista de proveedores pasados por props
    // Cada proveedor tiene: { id: string, name: string, source: 'Local'|'CliAgent'|'ApiCloud', status: 'online'|'offline'|'no_credits'|'not_installed', model: string, credits?: number, maxCredits?: number, maskedKey?: string }
    providers = $bindable([
      { id: 'ollama', name: 'Ollama Local', source: 'Local', status: 'online', model: 'llama3.1:8b' },
      { id: 'opencode', name: 'OpenCode CLI', source: 'CliAgent', status: 'online', model: 'qwen2.5-coder:7b' },
      { id: 'claude-code', name: 'Claude Code', source: 'CliAgent', status: 'not_installed', model: 'claude-3-5-sonnet' },
      { id: 'openrouter', name: 'OpenRouter Cloud', source: 'ApiCloud', status: 'no_credits', model: 'gpt-4o-mini', credits: 0, maxCredits: 100, maskedKey: 'sk-or-a...52f9' }
    ]),

    // Estrategia de enrutamiento seleccionada: 'PrivacyFirst' | 'CostFirst' | 'QualityFirst' | 'LatencyFirst'
    strategy = $bindable('PrivacyFirst'),

    // Si está en modo automático
    auto = $bindable(true),

    // ID del proveedor seleccionado cuando NO es auto (modo manual pin)
    pinnedProviderId = $bindable(null),

    // Historial de eventos de switch (para la UI / depuración)
    switchHistory = $bindable([]),

    // Callback de cambio de modo/selección
    onchange = null,

    ...rest
  } = $props();

  // Estados visuales internos
  const strategies = [
    { value: 'PrivacyFirst', label: 'Privacidad Primero (Local/CLI)' },
    { value: 'CostFirst', label: 'Costo Primero' },
    { value: 'QualityFirst', label: 'Calidad Primero' },
    { value: 'LatencyFirst', label: 'Latencia Mínima' }
  ];

  // Agrupación de proveedores por fuente
  let localProviders = $derived(providers.filter(p => p.source === 'Local'));
  let cliProviders = $derived(providers.filter(p => p.source === 'CliAgent'));
  let apiProviders = $derived(providers.filter(p => p.source === 'ApiCloud'));

  // Manejo de eventos de click / toggles
  function handleToggleAuto() {
    auto = !auto;
    if (auto) {
      pinnedProviderId = null;
    } else if (providers.length > 0) {
      // Pin first online or first available provider
      const active = providers.find(p => p.status === 'online') || providers[0];
      pinnedProviderId = active ? active.id : null;
    }
    if (onchange) onchange({ auto, strategy, pinnedProviderId });
  }

  function handleSelectStrategy(e) {
    strategy = e.target.value;
    if (onchange) onchange({ auto, strategy, pinnedProviderId });
  }

  function handlePinProvider(id) {
    auto = false;
    pinnedProviderId = id;
    if (onchange) onchange({ auto, strategy, pinnedProviderId });
  }
</script>

<div class="swal-model-selector" {...rest}>
  <!-- Encabezado con el selector de modo Auto vs Pin Manual -->
  <header class="selector-header">
    <div class="title-section">
      <h3 class="title">Configuración de Modelos</h3>
      <p class="subtitle">Selecciona el enrutamiento inteligente o fuerza un modelo específico</p>
    </div>

    <div class="mode-toggle">
      <button
        class="mode-btn"
        class:active={auto}
        onclick={handleToggleAuto}
      >
        Auto
      </button>
      <button
        class="mode-btn"
        class:active={!auto}
        onclick={() => {
          if (auto) handleToggleAuto();
        }}
      >
        Manual (Pin)
      </button>
    </div>
  </header>

  <!-- Si Auto está activo, mostramos selector de estrategia -->
  {#if auto}
    <div class="strategy-section swal-enter">
      <label for="strategy-select" class="strategy-label">Estrategia Activa</label>
      <div class="select-wrapper">
        <select
          id="strategy-select"
          class="strategy-select"
          value={strategy}
          onchange={handleSelectStrategy}
        >
          {#each strategies as strat}
            <option value={strat.value}>{strat.label}</option>
          {/each}
        </select>
        <span class="select-arrow" aria-hidden="true">▼</span>
      </div>
      <p class="strategy-desc">
        La cadena de prioridad predeterminada intentará usar recursos Locales primero, luego CLI y finalmente APIs en la nube.
      </p>
    </div>
  {/if}

  <!-- Listado de fuentes y proveedores -->
  <div class="providers-list">
    <!-- SECCIÓN 1: LOCAL -->
    <section class="provider-group">
      <h4 class="group-title">1. Recursos Locales (GPUD/ManagedLlama)</h4>
      <div class="group-items">
        {#each localProviders as p}
          <div
            class="provider-card"
            class:pinned={!auto && pinnedProviderId === p.id}
            onclick={() => handlePinProvider(p.id)}
            role="button"
            tabindex="0"
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') handlePinProvider(p.id); }}
          >
            <div class="provider-info">
              <span class="provider-name">{p.name}</span>
              <span class="provider-model">{p.model}</span>
            </div>
            <div class="provider-actions">
              <ModelBadge source={p.source} status={p.status} />
              {#if !auto && pinnedProviderId === p.id}
                <span class="pin-indicator">📌 Pinned</span>
              {/if}
            </div>
          </div>
        {/each}
        {#if localProviders.length === 0}
          <p class="empty-state">No hay proveedores locales configurados.</p>
        {/if}
      </div>
    </section>

    <!-- SECCIÓN 2: AGENTES CLI -->
    <section class="provider-group">
      <h4 class="group-title">2. Agentes CLI (Subprocesos locales)</h4>
      <div class="group-items">
        {#each cliProviders as p}
          <div
            class="provider-card"
            class:pinned={!auto && pinnedProviderId === p.id}
            onclick={() => handlePinProvider(p.id)}
            role="button"
            tabindex="0"
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') handlePinProvider(p.id); }}
          >
            <div class="provider-info">
              <span class="provider-name">{p.name}</span>
              <span class="provider-model">{p.model}</span>
            </div>
            <div class="provider-actions">
              <ModelBadge source={p.source} status={p.status} />
              {#if !auto && pinnedProviderId === p.id}
                <span class="pin-indicator">📌 Pinned</span>
              {/if}
            </div>
          </div>
        {/each}
        {#if cliProviders.length === 0}
          <p class="empty-state">No hay agentes CLI configurados.</p>
        {/if}
      </div>
    </section>

    <!-- SECCIÓN 3: API CLOUD -->
    <section class="provider-group">
      <h4 class="group-title">3. Proveedores de API (Conexión Cloud con créditos)</h4>
      <div class="group-items">
        {#each apiProviders as p}
          <div
            class="provider-card cloud-card"
            class:pinned={!auto && pinnedProviderId === p.id}
            onclick={() => handlePinProvider(p.id)}
            role="button"
            tabindex="0"
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') handlePinProvider(p.id); }}
          >
            <div class="provider-main-row">
              <div class="provider-info">
                <span class="provider-name">{p.name}</span>
                <span class="provider-model">{p.model}</span>
                {#if p.maskedKey}
                  <span class="masked-key">Key: <code>{p.maskedKey}</code></span>
                {/if}
              </div>
              <div class="provider-actions">
                <ModelBadge source={p.source} status={p.status} />
                {#if !auto && pinnedProviderId === p.id}
                  <span class="pin-indicator">📌 Pinned</span>
                {/if}
              </div>
            </div>

            <!-- Barra de créditos para APIs -->
            {#if p.credits !== undefined}
              <div class="credits-container">
                <div class="credits-header">
                  <span class="credits-label">Créditos de API</span>
                  <span class="credits-val">${p.credits.toFixed(2)} / ${(p.maxCredits || 100).toFixed(2)}</span>
                </div>
                <div class="credits-bar-bg">
                  <div
                    class="credits-bar-fill"
                    style="width: {Math.min(100, Math.max(0, (p.credits / (p.maxCredits || 100)) * 100))}%"
                  ></div>
                </div>
              </div>
            {/if}
          </div>
        {/each}
        {#if apiProviders.length === 0}
          <p class="empty-state">No hay proveedores de API configurados.</p>
        {/if}
      </div>
    </section>
  </div>

  <!-- Historial de switches / Eventos de Fallback (si existen) -->
  {#if switchHistory.length > 0}
    <div class="switch-history swal-enter">
      <h4 class="history-title">Eventos de Conectividad (Router Fallback)</h4>
      <ul class="history-list">
        {#each switchHistory as event}
          <li class="history-item">
            <span class="history-time">{event.timestamp || 'Ahora'}</span>
            <span class="history-desc">{event.message}</span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .swal-model-selector {
    background: var(--swal-surface);
    border: 1px solid var(--swal-border);
    border-radius: var(--swal-radius-lg, 12px);
    padding: var(--swal-space-5, 20px);
    font-family: var(--swal-font);
    color: var(--swal-text);
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-5, 20px);
    max-width: 640px;
  }

  .selector-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--swal-space-3, 12px);
    border-bottom: 1px solid var(--swal-border);
    padding-bottom: var(--swal-space-4, 16px);
  }

  .title {
    font-size: var(--swal-font-size-md, 1rem);
    font-weight: 600;
    margin: 0;
  }

  .subtitle {
    font-size: var(--swal-font-size-xs, 0.75rem);
    color: var(--swal-text-secondary);
    margin: var(--swal-space-1, 4px) 0 0 0;
  }

  /* Toggle Auto / Pin manual */
  .mode-toggle {
    display: inline-flex;
    background: var(--swal-elevated, #0f172a);
    border: 1px solid var(--swal-border);
    padding: 2px;
    border-radius: var(--swal-radius-sm, 4px);
  }

  .mode-btn {
    background: transparent;
    border: none;
    color: var(--swal-text-secondary);
    font-size: var(--swal-font-size-xs, 0.75rem);
    font-weight: 500;
    padding: 6px 12px;
    border-radius: var(--swal-radius-sm, 4px);
    cursor: pointer;
    transition: all var(--swal-transition-fast, 150ms);
  }

  .mode-btn.active {
    background: var(--swal-accent, #06b6d4);
    color: #fff;
    box-shadow: var(--swal-shadow-sm);
  }

  /* Estrategia de enrutamiento */
  .strategy-section {
    background: var(--swal-elevated, #0f172a);
    border: 1px solid var(--swal-border);
    border-radius: var(--swal-radius, 8px);
    padding: var(--swal-space-4, 16px);
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-2, 8px);
  }

  .strategy-label {
    font-size: var(--swal-font-size-xs, 0.75rem);
    font-weight: 600;
    color: var(--swal-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .select-wrapper {
    position: relative;
    display: inline-block;
    width: 100%;
  }

  .strategy-select {
    width: 100%;
    background: var(--swal-surface);
    color: var(--swal-text);
    border: 1px solid var(--swal-border);
    border-radius: var(--swal-radius-sm, 4px);
    padding: 8px 12px;
    font-size: var(--swal-font-size-sm, 0.875rem);
    cursor: pointer;
    appearance: none;
    outline: none;
  }

  .strategy-select:focus {
    border-color: var(--swal-accent);
  }

  .select-arrow {
    position: absolute;
    right: 12px;
    top: 50%;
    transform: translateY(-50%);
    pointer-events: none;
    font-size: 10px;
    color: var(--swal-text-secondary);
  }

  .strategy-desc {
    font-size: var(--swal-font-size-xs, 0.75rem);
    color: var(--swal-text-muted);
    margin: 0;
  }

  /* Listado de proveedores */
  .providers-list {
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-5, 20px);
  }

  .provider-group {
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-2, 8px);
  }

  .group-title {
    font-size: var(--swal-font-size-xs, 0.75rem);
    font-weight: 600;
    color: var(--swal-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0;
  }

  .group-items {
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-2, 8px);
  }

  .provider-card {
    background: var(--swal-elevated, #0f172a);
    border: 1px solid var(--swal-border);
    border-radius: var(--swal-radius, 8px);
    padding: var(--swal-space-3, 12px);
    display: flex;
    justify-content: space-between;
    align-items: center;
    cursor: pointer;
    transition: all var(--swal-transition-fast, 150ms);
    user-select: none;
  }

  .provider-card:hover {
    background: var(--swal-surface-hover);
    border-color: var(--swal-border-light);
  }

  .provider-card.pinned {
    border-color: var(--swal-accent-orange, #f97316);
    box-shadow: var(--swal-shadow-neon-orange);
  }

  .provider-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .provider-name {
    font-size: var(--swal-font-size-sm, 0.875rem);
    font-weight: 500;
  }

  .provider-model {
    font-size: var(--swal-font-size-xs, 0.75rem);
    color: var(--swal-text-secondary);
    font-family: var(--swal-font-mono);
  }

  .provider-actions {
    display: flex;
    align-items: center;
    gap: var(--swal-space-3, 12px);
  }

  .pin-indicator {
    font-size: var(--swal-font-size-xs, 0.75rem);
    font-weight: 600;
    color: var(--swal-accent-orange, #f97316);
  }

  .empty-state {
    font-size: var(--swal-font-size-xs, 0.75rem);
    color: var(--swal-text-muted);
    font-style: italic;
    margin: 0;
  }

  /* Cloud Card específicos */
  .cloud-card {
    flex-direction: column;
    align-items: stretch;
    gap: var(--swal-space-3, 12px);
  }

  .provider-main-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .masked-key {
    font-size: 10px;
    color: var(--swal-text-muted);
    margin-top: 2px;
  }

  .masked-key code {
    font-family: var(--swal-font-mono);
    color: var(--swal-text-secondary);
  }

  /* Barra de créditos */
  .credits-container {
    display: flex;
    flex-direction: column;
    gap: 4px;
    border-top: 1px solid var(--swal-border);
    padding-top: var(--swal-space-2, 8px);
  }

  .credits-header {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
  }

  .credits-label {
    color: var(--swal-text-muted);
  }

  .credits-val {
    color: var(--swal-text-secondary);
    font-family: var(--swal-font-mono);
  }

  .credits-bar-bg {
    height: 4px;
    background: rgba(255, 255, 255, 0.05);
    border-radius: 2px;
    overflow: hidden;
  }

  .credits-bar-fill {
    height: 100%;
    background: var(--swal-accent-orange, #f97316);
    border-radius: 2px;
    box-shadow: var(--swal-shadow-neon-orange);
  }

  /* Historial de switches */
  .switch-history {
    background: rgba(239, 68, 68, 0.05);
    border: 1px solid rgba(239, 68, 68, 0.1);
    border-radius: var(--swal-radius, 8px);
    padding: var(--swal-space-3, 12px);
  }

  .history-title {
    font-size: var(--swal-font-size-xs, 0.75rem);
    font-weight: 600;
    color: var(--swal-danger, #ef4444);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 var(--swal-space-2, 8px) 0;
  }

  .history-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .history-item {
    display: flex;
    gap: var(--swal-space-3, 12px);
    font-size: var(--swal-font-size-xs, 0.75rem);
  }

  .history-time {
    color: var(--swal-text-muted);
    font-family: var(--swal-font-mono);
    white-space: nowrap;
  }

  .history-desc {
    color: var(--swal-text-secondary);
  }

  /* Animaciones */
  .swal-enter {
    animation: swal-fade-in var(--swal-transition, 200ms) var(--swal-ease-out, cubic-bezier(0.16, 1, 0.3, 1));
  }

  @keyframes swal-fade-in {
    from { opacity: 0; transform: translateY(4px); }
    to { opacity: 1; transform: translateY(0); }
  }
</style>
