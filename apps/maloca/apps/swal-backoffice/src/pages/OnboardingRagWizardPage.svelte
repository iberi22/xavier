<script lang="ts">
  import { useRagProfileStore, RAG_PROFILES } from "../lib/ragProfileStore.svelte";

  interface Props {
    apiBaseUrl?: string;
    onCompleted?: (result: { success: boolean; message: string; nodeConfigId?: string }) => void;
  }

  let { apiBaseUrl = "", onCompleted }: Props = $props();

  const store = useRagProfileStore();

  async function handleFormSubmit(e: SubmitEvent) {
    e.preventDefault();
    const result = await store.saveProfiles(apiBaseUrl);
    if (onCompleted) {
      onCompleted(result);
    }
  }
</script>

<!-- Multi-Modal RAG Wizard for Documentos Legales, Código Fuente, Video, Fotos y Audio -->
<div class="min-h-screen bg-zinc-950 text-[#FDFBF7] p-4 sm:p-6 md:p-10 font-sans selection:bg-amber-500 selection:text-zinc-950">
  <div class="max-w-5xl mx-auto space-y-8">
    <!-- Header section -->
    <header class="border-b border-zinc-800 pb-6 space-y-3">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <div class="flex items-center gap-2 text-xs font-mono uppercase tracking-widest text-amber-400/90 mb-1">
            <span class="inline-block w-2 h-2 rounded-full bg-amber-400 animate-pulse"></span>
            Asistente de Ingesta & Memoria
          </div>
          <h1 class="text-2xl sm:text-3xl font-bold tracking-tight text-[#FDFBF7]">
            Configuración de Perfiles RAG
          </h1>
        </div>

        <!-- Zero-Cloud / On-Premise Badge -->
        <div class="inline-flex items-center gap-2.5 px-3.5 py-1.5 rounded-full bg-emerald-950/60 border border-emerald-500/30 text-emerald-300 text-xs font-medium self-start sm:self-auto">
          <svg class="w-4 h-4 text-emerald-400 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"/>
          </svg>
          <span>Zero-Cloud & Privacy First</span>
        </div>
      </div>
      <p class="text-sm text-zinc-400 max-w-3xl leading-relaxed">
        Seleccione las fuentes de información que su nodo Xavier indexará (Documentos Legales, código AST, multimedia y voz). Toda la información permanecerá encriptada y procesada localmente sin enviar datos a servidores externos.
      </p>
    </header>

    <form onsubmit={handleFormSubmit} class="space-y-8">
      <!-- Profile Cards Selector Grid -->
      <section class="space-y-4">
        <div class="flex items-center justify-between">
          <h2 class="text-lg font-semibold text-[#FDFBF7] flex items-center gap-2">
            <span>1. Seleccione los perfiles de datos a indexar</span>
            <span class="text-xs font-normal text-zinc-400">({store.selectedProfiles.length} seleccionados)</span>
          </h2>
        </div>

        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {#each RAG_PROFILES as profile (profile.id)}
            {@const isSelected = store.selectedProfiles.includes(profile.id)}
            <button
              type="button"
              onclick={() => store.toggleProfile(profile.id)}
              aria-pressed={isSelected}
              class="relative flex flex-col text-left p-5 rounded-xl border transition-all duration-200 outline-none focus-visible:ring-2 focus-visible:ring-amber-500 {isSelected ? 'bg-zinc-900/90 border-amber-500/70 shadow-lg shadow-amber-500/5 ring-1 ring-amber-500/40' : 'bg-zinc-900/40 border-zinc-800 hover:border-zinc-700 hover:bg-zinc-900/60'}"
            >
              <div class="flex items-start justify-between gap-3 mb-3">
                <div class="flex items-center gap-2.5">
                  <span class="text-2xl" role="img" aria-label={profile.title}>{profile.icon}</span>
                  <div>
                    <span class="text-[10px] uppercase font-mono tracking-wider text-amber-400/80 block">
                      {profile.category}
                    </span>
                    <h3 class="font-semibold text-sm text-[#FDFBF7] leading-snug">
                      {profile.title}
                    </h3>
                  </div>
                </div>

                <!-- Checkbox Indicator -->
                <div class="w-5 h-5 rounded border flex items-center justify-center shrink-0 transition-colors {isSelected ? 'bg-amber-500 border-amber-400 text-zinc-950' : 'border-zinc-700 bg-zinc-950/50'}">
                  {#if isSelected}
                    <svg class="w-3.5 h-3.5 stroke-[3]" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7"/>
                    </svg>
                  {/if}
                </div>
              </div>

              <p class="text-xs text-zinc-400 leading-relaxed mt-auto">
                {profile.description}
              </p>
            </button>
          {/each}
        </div>
      </section>

      <!-- Quota and Hardware Allocation Section -->
      <section class="grid grid-cols-1 lg:grid-cols-3 gap-6 pt-2">
        <!-- Storage Slider Card -->
        <div class="lg:col-span-2 p-6 rounded-xl bg-zinc-900/50 border border-zinc-800 space-y-5">
          <div class="flex items-center justify-between">
            <div>
              <h2 class="text-base font-semibold text-[#FDFBF7]">
                2. Asignación de Almacenamiento
              </h2>
              <p class="text-xs text-zinc-400 mt-0.5">
                Ajuste la capacidad máxima de disco dedicada a vectores e índices FTS5
              </p>
            </div>
            <span class="text-2xl font-bold font-mono text-amber-400 bg-amber-500/10 border border-amber-500/20 px-3 py-1 rounded-lg">
              {store.storageGb} GB
            </span>
          </div>

          <!-- Slider component -->
          <div class="space-y-2 pt-2">
            <input
              type="range"
              min="10"
              max="500"
              step="5"
              value={store.storageGb}
              oninput={(e) => store.setStorageGb(Number((e.target as HTMLInputElement).value))}
              class="w-full h-2 bg-zinc-800 rounded-lg appearance-none cursor-pointer accent-amber-500 focus:outline-none focus:ring-2 focus:ring-amber-500/50"
            />
            <div class="flex justify-between text-[11px] font-mono text-zinc-500">
              <span>10 GB (Mínimo)</span>
              <span>100 GB</span>
              <span>250 GB</span>
              <span>500 GB (Máximo)</span>
            </div>
          </div>

          <!-- Zero-Cloud Privacy Toggle -->
          <div class="pt-4 border-t border-zinc-800/80 flex items-center justify-between gap-4">
            <div class="space-y-0.5">
              <label for="zero-cloud-toggle" class="text-sm font-medium text-[#FDFBF7] cursor-pointer">
                Modo Estricto On-Premise (Zero-Cloud)
              </label>
              <p class="text-xs text-zinc-400">
                Bloquea llamadas a APIs de nube y fuerza el uso de modelos locales Whisper y Ollama/GLLM
              </p>
            </div>
            <button
              id="zero-cloud-toggle"
              type="button"
              role="switch"
              aria-checked={store.zeroCloud}
              onclick={() => store.setZeroCloud(!store.zeroCloud)}
              class="relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-amber-500/50 {store.zeroCloud ? 'bg-emerald-600' : 'bg-zinc-700'}"
            >
              <span class="sr-only">Modo On-Premise</span>
              <span class="pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out {store.zeroCloud ? 'translate-x-5' : 'translate-x-0'}"></span>
            </button>
          </div>
        </div>

        <!-- Resource Estimation Summary Box -->
        <div class="p-6 rounded-xl bg-zinc-900/50 border border-zinc-800 flex flex-col justify-between space-y-4">
          <div>
            <h3 class="text-xs uppercase font-mono tracking-wider text-amber-400/90 mb-3">
              Estimación de Recursos Requeridos
            </h3>

            <dl class="space-y-3.5 text-xs">
              <div class="flex justify-between items-center pb-2 border-b border-zinc-800/60">
                <dt class="text-zinc-400">RAM Estimada:</dt>
                <dd class="font-mono font-semibold text-[#FDFBF7]">{store.formattedRamEstimate}</dd>
              </div>
              <div class="flex justify-between items-center pb-2 border-b border-zinc-800/60">
                <dt class="text-zinc-400">Espacio en Disco:</dt>
                <dd class="font-mono font-semibold text-[#FDFBF7]">{store.storageGb} GB</dd>
              </div>
              <div class="flex justify-between items-center pb-2 border-b border-zinc-800/60">
                <dt class="text-zinc-400">Mínimo CPU Cores:</dt>
                <dd class="font-mono font-semibold text-[#FDFBF7]">{store.recommendedNodeSpecs.minCpuCores} Cores</dd>
              </div>
              <div class="flex justify-between items-center">
                <dt class="text-zinc-400">Privacidad:</dt>
                <dd class="font-semibold text-emerald-400">
                  {store.zeroCloud ? "On-Premise Estricto" : "Híbrido"}
                </dd>
              </div>
            </dl>
          </div>

          <div class="p-3 rounded-lg bg-zinc-950/60 border border-zinc-800 text-[11px] text-zinc-400 leading-relaxed">
            💡 La memoria RAM escala según la cantidad de perfiles activos y la cuota de vectores reservada.
          </div>
        </div>
      </section>

      <!-- Status Messages -->
      {#if store.saveStatus}
        <div class="p-4 rounded-xl border text-sm flex items-center gap-3 {store.saveStatus.success ? 'bg-emerald-950/40 border-emerald-500/40 text-emerald-200' : 'bg-rose-950/40 border-rose-500/40 text-rose-200'}">
          {#if store.saveStatus.success}
            <svg class="w-5 h-5 text-emerald-400 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"/>
            </svg>
          {:else}
            <svg class="w-5 h-5 text-rose-400 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"/>
            </svg>
          {/if}
          <span>{store.saveStatus.message}</span>
        </div>
      {/if}

      <!-- Submit Action Button -->
      <div class="flex items-center justify-end gap-4 pt-4 border-t border-zinc-800">
        <button
          type="button"
          onclick={() => store.resetSelection()}
          disabled={store.isSaving}
          class="px-4 py-2.5 rounded-xl border border-zinc-800 text-xs font-medium text-zinc-300 hover:bg-zinc-900 transition-colors disabled:opacity-50"
        >
          Restablecer
        </button>

        <button
          type="submit"
          disabled={store.isSaving || store.selectedProfiles.length === 0}
          class="inline-flex items-center gap-2 px-6 py-2.5 rounded-xl bg-amber-500 text-zinc-950 font-semibold text-xs hover:bg-amber-400 transition-colors shadow-md shadow-amber-500/10 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {#if store.isSaving}
            <svg class="w-4 h-4 animate-spin text-zinc-950" fill="none" viewBox="0 0 24 24">
              <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
              <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            <span>Configurando Nodo...</span>
          {:else}
            <span>Guardar y Configurar Nodo</span>
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M14 5l7 7m0 0l-7 7m7-7H3"/>
            </svg>
          {/if}
        </button>
      </div>
    </form>
  </div>
</div>
