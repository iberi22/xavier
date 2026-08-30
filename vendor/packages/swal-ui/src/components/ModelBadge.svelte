<script>
  let {
    source = 'Local', // 'Local' | 'CliAgent' | 'ApiCloud'
    status = 'online', // 'online' | 'offline' | 'no_credits' | 'not_installed'
    label = '',
    pulse = false,
    ...rest
  } = $props();

  const variantMap = {
    online: 'success',
    offline: 'neutral',
    no_credits: 'warning',
    not_installed: 'danger'
  };

  const statusLabelMap = {
    online: 'Online',
    offline: 'Offline',
    no_credits: 'Sin créditos',
    not_installed: 'No instalado'
  };

  const sourceLabelMap = {
    Local: 'Local',
    CliAgent: 'Agente CLI',
    ApiCloud: 'API Nube'
  };

  let badgeVariant = $derived(variantMap[status] || 'neutral');
  let displayLabel = $derived(label || sourceLabelMap[source] || statusLabelMap[status] || source);
</script>

<span
  class="swal-model-badge {badgeVariant}"
  class:pulse
  {...rest}
>
  <span class="dot {status}" aria-hidden="true"></span>
  <span class="label">{displayLabel}</span>
</span>

<style>
  .swal-model-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-family: var(--swal-font);
    font-weight: 500;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 9999px;
    border: 1px solid transparent;
    user-select: none;
  }

  .success {
    background: rgba(16, 185, 129, 0.1);
    color: #34d399;
    border-color: rgba(16, 185, 129, 0.2);
  }
  .warning {
    background: rgba(245, 158, 11, 0.1);
    color: #fbbf24;
    border-color: rgba(245, 158, 11, 0.2);
  }
  .danger {
    background: rgba(239, 68, 68, 0.1);
    color: #f87171;
    border-color: rgba(239, 68, 68, 0.2);
  }
  .neutral {
    background: var(--swal-surface);
    color: var(--swal-text-secondary);
    border-color: var(--swal-border);
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }

  .dot.online {
    background: #10b981;
    box-shadow: var(--swal-shadow-neon-emerald, 0 0 8px rgba(16, 185, 129, 0.5));
  }
  .dot.offline {
    background: #64748b;
  }
  .dot.no_credits {
    background: var(--swal-accent-orange, #f97316);
    box-shadow: var(--swal-shadow-neon-orange, 0 0 8px rgba(249, 115, 22, 0.5));
  }
  .dot.not_installed {
    background: #ef4444;
    box-shadow: 0 0 8px rgba(239, 68, 68, 0.5);
  }

  .pulse {
    animation: swal-pulse 2s cubic-bezier(0.4, 0, 0.6, 1) infinite;
  }

  @keyframes swal-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  @media (prefers-reduced-motion: reduce) {
    .pulse { animation: none; }
  }
</style>
