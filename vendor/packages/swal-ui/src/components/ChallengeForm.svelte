<script lang="ts">
  import Button from './Button.svelte';
  import Badge from './Badge.svelte';

  // Svelte 5 runes syntax
  let {
    challenge,
    onrespond, // callback receiving (answer_text)
    submitting = false,
    response = null // optional ChallengeResponse structure if already answered/scored
  } = $props();

  let answerText = $state(response?.answerText || '');

  function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (!answerText.trim()) return;
    onrespond?.(answerText);
  }

  // Determine scores and verdicts if available
  const scores = $derived(response?.scores);
  const verdict = $derived(response?.verdict);

  const verdictConfig = {
    confirmed: { label: 'Confirmado', variant: 'success' },
    updated: { label: 'Actualizado', variant: 'info' },
    conflict: { label: 'Conflicto', variant: 'danger' },
    uncertain: { label: 'Incierto', variant: 'warning' }
  };

  const verdictInfo = $derived(verdict ? (verdictConfig[verdict] || { label: verdict, variant: 'neutral' }) : null);
</script>

<div class="challenge-form-container">
  <form onsubmit={handleSubmit} class="respond-form">
    <div class="form-group">
      <label for="answerText" class="swal-text-secondary">Tu respuesta:</label>
      <textarea
        id="answerText"
        class="swal-textarea swal-scrollbar"
        placeholder="Escribe tu justificación, respuesta o resolución al reto planteado..."
        bind:value={answerText}
        disabled={submitting || !!response}
        required
      ></textarea>
    </div>

    {#if !response}
      <div class="actions">
        <Button
          type="submit"
          variant="orange"
          disabled={!answerText.trim() || submitting}
          loading={submitting}
        >
          Enviar respuesta
        </Button>
      </div>
    {:else}
      <div class="results-panel">
        <h3 class="swal-text">Resultado de la Evaluación</h3>

        {#if verdictInfo}
          <div class="verdict-row">
            <span class="swal-text-secondary">Veredicto:</span>
            <Badge variant={verdictInfo.variant} size="md">
              {verdictInfo.label}
            </Badge>
          </div>
        {/if}

        {#if scores}
          <div class="scores-grid">
            <div class="score-card">
              <span class="score-title">Coherencia</span>
              <span class="score-val" class:low={scores.coherence < 0.5}>{Math.round(scores.coherence * 100)}%</span>
              <div class="score-bar-bg">
                <div class="score-bar" style="width: {scores.coherence * 100}%; background: var(--swal-accent)"></div>
              </div>
            </div>
            <div class="score-card">
              <span class="score-title">Contradicción</span>
              <span class="score-val" class:low={scores.contradiction < 0.5}>{Math.round(scores.contradiction * 100)}%</span>
              <div class="score-bar-bg">
                <div class="score-bar" style="width: {scores.contradiction * 100}%; background: var(--swal-danger)"></div>
              </div>
            </div>
            <div class="score-card">
              <span class="score-title">Recuerdo</span>
              <span class="score-val" class:low={scores.recall < 0.5}>{Math.round(scores.recall * 100)}%</span>
              <div class="score-bar-bg">
                <div class="score-bar" style="width: {scores.recall * 100}%; background: var(--swal-success)"></div>
              </div>
            </div>
            <div class="score-card highlighted">
              <span class="score-title">Overall Score</span>
              <span class="score-val highlighted">{Math.round(scores.overall * 100)}%</span>
              <div class="score-bar-bg">
                <div class="score-bar" style="width: {scores.overall * 100}%; background: var(--swal-accent-orange)"></div>
              </div>
            </div>
          </div>
        {/if}

        {#if response.feedback}
          <div class="feedback-box">
            <h4 class="feedback-title swal-text-secondary">Feedback generado por el Analizador:</h4>
            <p class="feedback-text">{response.feedback}</p>
          </div>
        {/if}

        {#if response.analyzerVersion}
          <span class="analyzer-version">
            Analyzer: v{response.analyzerVersion}
          </span>
        {/if}
      </div>
    {/if}
  </form>
</div>

<style>
  .challenge-form-container {
    width: 100%;
    font-family: var(--swal-font);
  }
  .form-group {
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-2);
    margin-bottom: var(--swal-space-4);
  }
  .form-group label {
    font-size: var(--swal-font-size-sm);
    font-weight: 500;
  }
  .swal-textarea {
    width: 100%;
    min-height: 120px;
    background: var(--swal-surface);
    border: 1px solid var(--swal-border);
    border-radius: var(--swal-radius);
    padding: var(--swal-space-3);
    color: var(--swal-text);
    font-size: var(--swal-font-size-sm);
    font-family: var(--swal-font);
    resize: vertical;
    transition: border-color var(--swal-transition-fast), box-shadow var(--swal-transition-fast);
    box-sizing: border-box;
  }
  .swal-textarea:focus {
    outline: none;
    border-color: var(--swal-accent);
    box-shadow: 0 0 0 2px var(--swal-accent-muted);
  }
  .swal-textarea:disabled {
    opacity: 0.7;
    cursor: not-allowed;
    background: rgba(0, 0, 0, 0.2);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .results-panel {
    background: rgba(0, 0, 0, 0.2);
    border: 1px solid var(--swal-border-light);
    border-radius: var(--swal-radius);
    padding: var(--swal-space-4);
    margin-top: var(--swal-space-4);
  }
  .results-panel h3 {
    margin: 0 0 var(--swal-space-3);
    font-size: var(--swal-font-size-sm);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .verdict-row {
    display: flex;
    align-items: center;
    gap: var(--swal-space-3);
    margin-bottom: var(--swal-space-4);
    font-size: var(--swal-font-size-sm);
  }
  .scores-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
    gap: var(--swal-space-3);
    margin-bottom: var(--swal-space-4);
  }
  .score-card {
    background: var(--swal-surface);
    border: 1px solid var(--swal-border);
    border-radius: var(--swal-radius-sm);
    padding: var(--swal-space-3);
    display: flex;
    flex-direction: column;
    gap: var(--swal-space-1);
  }
  .score-card.highlighted {
    border-color: var(--swal-accent-orange);
    background: var(--swal-accent-orange-muted);
  }
  .score-title {
    font-size: 11px;
    color: var(--swal-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.02em;
  }
  .score-val {
    font-size: var(--swal-font-size-lg);
    font-weight: 700;
    font-family: var(--swal-font-mono);
  }
  .score-val.low {
    color: var(--swal-danger);
  }
  .score-val.highlighted {
    color: var(--swal-accent-orange);
  }
  .score-bar-bg {
    width: 100%;
    height: 4px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: var(--swal-radius-sm);
    overflow: hidden;
    margin-top: var(--swal-space-1);
  }
  .score-bar {
    height: 100%;
    border-radius: var(--swal-radius-sm);
  }
  .feedback-box {
    background: rgba(0, 0, 0, 0.3);
    border-left: 3px solid var(--swal-accent-orange);
    border-radius: 0 var(--swal-radius-sm) var(--swal-radius-sm) 0;
    padding: var(--swal-space-3);
    margin-bottom: var(--swal-space-3);
  }
  .feedback-title {
    margin: 0 0 var(--swal-space-1);
    font-size: var(--swal-font-size-xs);
    font-weight: 600;
  }
  .feedback-text {
    margin: 0;
    font-size: var(--swal-font-size-sm);
    color: var(--swal-text);
    line-height: 1.5;
  }
  .analyzer-version {
    font-size: 10px;
    color: var(--swal-text-muted);
    font-family: var(--swal-font-mono);
    display: block;
    text-align: right;
  }
</style>
