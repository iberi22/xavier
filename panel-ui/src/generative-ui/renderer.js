/**
 * Xavier2 Generative UI Renderer
 * Convierte JSON declarativo a HTML/CSS interactivo
 * Zero dependencies, vanilla JS, ~400 lines
 */

class XavierUIRenderer {
  constructor(containerId, options = {}) {
    this.container = document.getElementById(containerId);
    this.options = {
      theme: options.theme || 'auto',
      onAction: options.onAction || this.defaultActionHandler.bind(this),
      onSubmit: options.onSubmit || this.defaultSubmitHandler.bind(this),
      ...options
    };
    this.components = new Map();
    this.init();
  }

  init() {
    this.registerComponents();
    this.injectStyles();
  }

  // ── Component Registry ──────────────────────────────────────
  registerComponents() {
    this.components.set('text-response', this.renderTextResponse.bind(this));
    this.components.set('data-table', this.renderDataTable.bind(this));
    this.components.set('info-card', this.renderInfoCard.bind(this));
    this.components.set('form-input', this.renderFormInput.bind(this));
    this.components.set('progress-bar', this.renderProgressBar.bind(this));
    this.components.set('code-block', this.renderCodeBlock.bind(this));
    this.components.set('timeline', this.renderTimeline.bind(this));
    this.components.set('confirm-dialog', this.renderConfirmDialog.bind(this));
    this.components.set('status-badge', this.renderStatusBadge.bind(this));
    this.components.set('chart-bar', this.renderChartBar.bind(this));
    this.components.set('list-group', this.renderListGroup.bind(this));
  }

  // ── Main Render ─────────────────────────────────────────────
  render(jsonData) {
    try {
      const data = typeof jsonData === 'string' ? JSON.parse(jsonData) : jsonData;
      
      if (!data.component) {
        console.error('[XavierUI] No component type specified');
        return this.renderFallback(data);
      }

      const renderer = this.components.get(data.component);
      if (!renderer) {
        console.error(`[XavierUI] Unknown component: ${data.component}`);
        return this.renderFallback(data);
      }

      const element = renderer(data);
      this.container.innerHTML = '';
      this.container.appendChild(element);
      return element;
    } catch (err) {
      console.error('[XavierUI] Render error:', err);
      return this.renderError(err);
    }
  }

  // ── Component: text-response ────────────────────────────────
  renderTextResponse(data) {
    const el = document.createElement('div');
    el.className = `xui-text-response xui-text-${data.style || 'default'}`;
    el.innerHTML = this.escapeHtml(data.content);
    return el;
  }

  // ── Component: data-table ────────────────────────────────────
  renderDataTable(data) {
    const wrapper = document.createElement('div');
    wrapper.className = 'xui-data-table';

    if (data.title) {
      const title = document.createElement('h3');
      title.className = 'xui-table-title';
      title.textContent = data.title;
      wrapper.appendChild(title);
    }

    const table = document.createElement('table');
    table.className = 'xui-table';

    // Header
    const thead = document.createElement('thead');
    const headerRow = document.createElement('tr');
    data.columns.forEach(col => {
      const th = document.createElement('th');
      th.textContent = col.label;
      if (col.width) th.style.width = col.width;
      headerRow.appendChild(th);
    });
    thead.appendChild(headerRow);
    table.appendChild(thead);

    // Body
    const tbody = document.createElement('tbody');
    data.rows.forEach((row, idx) => {
      const tr = document.createElement('tr');
      data.columns.forEach(col => {
        const td = document.createElement('td');
        const value = row[col.key];
        
        if (col.key === 'actions' && Array.isArray(value)) {
          value.forEach(action => {
            const btn = document.createElement('button');
            btn.className = 'xui-btn xui-btn-sm xui-btn-outline';
            btn.textContent = action;
            btn.onclick = () => this.options.onAction({
              type: 'table-action',
              action,
              row,
              rowIndex: idx
            });
            td.appendChild(btn);
          });
        } else if (col.key === 'status') {
          td.appendChild(this.createStatusBadge(value));
        } else if (col.key === 'progress' && typeof value === 'number') {
          td.appendChild(this.createMiniProgress(value));
        } else {
          td.textContent = value ?? '';
        }
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    });
    table.appendChild(tbody);
    wrapper.appendChild(table);

    return wrapper;
  }

  // ── Component: info-card ────────────────────────────────────
  renderInfoCard(data) {
    const el = document.createElement('div');
    el.className = 'xui-info-card';
    el.innerHTML = `
      <div class="xui-card-icon xui-icon-${data.color || 'blue'}">${data.icon || '●'}</div>
      <div class="xui-card-content">
        <div class="xui-card-title">${this.escapeHtml(data.title)}</div>
        <div class="xui-card-value">${this.escapeHtml(data.value)}</div>
        ${data.trend ? `<div class="xui-card-trend">${this.escapeHtml(data.trend)}</div>` : ''}
      </div>
    `;
    return el;
  }

  // ── Component: form-input ───────────────────────────────────
  renderFormInput(data) {
    const form = document.createElement('form');
    form.className = 'xui-form';
    form.onsubmit = (e) => {
      e.preventDefault();
      const formData = new FormData(form);
      const result = {};
      data.fields.forEach(field => {
        result[field.name] = formData.get(field.name);
      });
      this.options.onSubmit({ type: 'form-submit', formId: data.title, data: result });
    };

    if (data.title) {
      const title = document.createElement('h3');
      title.className = 'xui-form-title';
      title.textContent = data.title;
      form.appendChild(title);
    }

    data.fields.forEach(field => {
      const group = document.createElement('div');
      group.className = 'xui-form-group';

      const label = document.createElement('label');
      label.textContent = field.label + (field.required ? ' *' : '');
      group.appendChild(label);

      let input;
      if (field.type === 'textarea') {
        input = document.createElement('textarea');
      } else if (field.type === 'select') {
        input = document.createElement('select');
        field.options.forEach(opt => {
          const option = document.createElement('option');
          option.value = opt;
          option.textContent = opt;
          input.appendChild(option);
        });
      } else {
        input = document.createElement('input');
        input.type = field.type || 'text';
      }

      input.name = field.name;
      input.required = field.required || false;
      input.className = 'xui-input';
      group.appendChild(input);
      form.appendChild(group);
    });

    const actions = document.createElement('div');
    actions.className = 'xui-form-actions';

    const submitBtn = document.createElement('button');
    submitBtn.type = 'submit';
    submitBtn.className = 'xui-btn xui-btn-primary';
    submitBtn.textContent = data.submit_label || 'Enviar';
    actions.appendChild(submitBtn);

    if (data.cancel_label) {
      const cancelBtn = document.createElement('button');
      cancelBtn.type = 'button';
      cancelBtn.className = 'xui-btn xui-btn-ghost';
      cancelBtn.textContent = data.cancel_label;
      cancelBtn.onclick = () => this.options.onAction({ type: 'form-cancel', formId: data.title });
      actions.appendChild(cancelBtn);
    }

    form.appendChild(actions);
    return form;
  }

  // ── Component: progress-bar ─────────────────────────────────
  renderProgressBar(data) {
    const el = document.createElement('div');
    el.className = 'xui-progress-wrapper';
    
    const label = document.createElement('div');
    label.className = 'xui-progress-label';
    label.innerHTML = `<span>${this.escapeHtml(data.label)}</span><span>${data.percent}%</span>`;
    el.appendChild(label);

    const bar = document.createElement('div');
    bar.className = 'xui-progress-bar';
    
    const fill = document.createElement('div');
    fill.className = `xui-progress-fill xui-progress-${data.status || 'default'}`;
    fill.style.width = `${data.percent}%`;
    bar.appendChild(fill);
    el.appendChild(bar);

    return el;
  }

  // ── Component: code-block ───────────────────────────────────
  renderCodeBlock(data) {
    const wrapper = document.createElement('div');
    wrapper.className = 'xui-code-block';

    const header = document.createElement('div');
    header.className = 'xui-code-header';
    header.innerHTML = `
      <span class="xui-code-lang">${data.language || 'text'}</span>
      ${data.filename ? `<span class="xui-code-filename">${this.escapeHtml(data.filename)}</span>` : ''}
    `;
    wrapper.appendChild(header);

    const pre = document.createElement('pre');
    const code = document.createElement('code');
    code.textContent = data.code;
    pre.appendChild(code);
    wrapper.appendChild(pre);

    if (data.collapsible) {
      wrapper.classList.add('xui-collapsible');
      wrapper.addEventListener('click', () => {
        wrapper.classList.toggle('xui-expanded');
      });
    }

    return wrapper;
  }

  // ── Component: timeline ────────────────────────────────────
  renderTimeline(data) {
    const el = document.createElement('div');
    el.className = 'xui-timeline';

    if (data.title) {
      const title = document.createElement('h3');
      title.textContent = data.title;
      el.appendChild(title);
    }

    const list = document.createElement('div');
    list.className = 'xui-timeline-list';

    data.events.forEach(event => {
      const item = document.createElement('div');
      item.className = `xui-timeline-item xui-timeline-${event.status || 'default'}`;
      item.innerHTML = `
        <div class="xui-timeline-marker"></div>
        <div class="xui-timeline-content">
          <div class="xui-timeline-date">${this.escapeHtml(event.date)}</div>
          <div class="xui-timeline-title">${this.escapeHtml(event.title)}</div>
          <div class="xui-timeline-desc">${this.escapeHtml(event.description)}</div>
        </div>
      `;
      list.appendChild(item);
    });

    el.appendChild(list);
    return el;
  }

  // ── Component: confirm-dialog ───────────────────────────────
  renderConfirmDialog(data) {
    const el = document.createElement('div');
    el.className = 'xui-confirm-dialog';
    el.innerHTML = `
      <div class="xui-confirm-content">
        <div class="xui-confirm-message">${this.escapeHtml(data.message)}</div>
        ${data.description ? `<div class="xui-confirm-desc">${this.escapeHtml(data.description)}</div>` : ''}
        <div class="xui-confirm-actions">
          <button class="xui-btn xui-btn-${data.confirm_style || 'primary'} xui-confirm-yes">
            ${data.confirm_label || 'Confirmar'}
          </button>
          <button class="xui-btn xui-btn-ghost xui-confirm-no">
            ${data.cancel_label || 'Cancelar'}
          </button>
        </div>
      </div>
    `;

    el.querySelector('.xui-confirm-yes').onclick = () => {
      this.options.onAction({ type: 'confirm', result: true, message: data.message });
    };
    el.querySelector('.xui-confirm-no').onclick = () => {
      this.options.onAction({ type: 'confirm', result: false, message: data.message });
    };

    return el;
  }

  // ── Component: status-badge ─────────────────────────────────
  renderStatusBadge(data) {
    return this.createStatusBadge(data.text, data.variant);
  }

  createStatusBadge(text, variant = 'default') {
    const el = document.createElement('span');
    el.className = `xui-badge xui-badge-${variant}`;
    el.textContent = text;
    return el;
  }

  // ── Component: chart-bar ────────────────────────────────────
  renderChartBar(data) {
    const wrapper = document.createElement('div');
    wrapper.className = 'xui-chart-bar';

    if (data.title) {
      const title = document.createElement('h3');
      title.textContent = data.title;
      wrapper.appendChild(title);
    }

    const chart = document.createElement('div');
    chart.className = 'xui-chart-container';

    const maxValue = Math.max(...data.values);
    data.labels.forEach((label, i) => {
      const bar = document.createElement('div');
      bar.className = 'xui-chart-item';
      const percent = (data.values[i] / maxValue) * 100;
      bar.innerHTML = `
        <div class="xui-chart-bar-visual" style="height: ${percent}%; background: var(--xui-${data.colors?.[i] || 'blue'})"></div>
        <div class="xui-chart-bar-label">${this.escapeHtml(label)}</div>
        <div class="xui-chart-bar-value">${data.values[i]}</div>
      `;
      chart.appendChild(bar);
    });

    wrapper.appendChild(chart);
    return wrapper;
  }

  // ── Component: list-group ───────────────────────────────────
  renderListGroup(data) {
    const el = document.createElement('div');
    el.className = 'xui-list-group';

    if (data.title) {
      const title = document.createElement('h3');
      title.textContent = data.title;
      el.appendChild(title);
    }

    const list = document.createElement('div');
    list.className = 'xui-list';

    data.items.forEach((item, idx) => {
      const row = document.createElement('div');
      row.className = `xui-list-item xui-list-item-${item.status || 'default'}`;
      
      const label = document.createElement('span');
      label.className = 'xui-list-label';
      label.textContent = item.label;
      row.appendChild(label);

      if (item.actions) {
        const actions = document.createElement('div');
        actions.className = 'xui-list-actions';
        item.actions.forEach(action => {
          const btn = document.createElement('button');
          btn.className = 'xui-btn xui-btn-sm xui-btn-outline';
          btn.textContent = action;
          btn.onclick = () => this.options.onAction({
            type: 'list-action', action, item, itemIndex: idx
          });
          actions.appendChild(btn);
        });
        row.appendChild(actions);
      }

      list.appendChild(row);
    });

    el.appendChild(list);
    return el;
  }

  // ── Helpers ─────────────────────────────────────────────────
  createMiniProgress(percent) {
    const el = document.createElement('div');
    el.className = 'xui-mini-progress';
    el.innerHTML = `<div class="xui-mini-progress-fill" style="width: ${percent}%"></div>`;
    return el;
  }

  escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  defaultActionHandler(event) {
    console.log('[XavierUI] Action:', event);
  }

  defaultSubmitHandler(event) {
    console.log('[XavierUI] Submit:', event);
  }

  renderFallback(data) {
    const el = document.createElement('pre');
    el.className = 'xui-fallback';
    el.textContent = JSON.stringify(data, null, 2);
    return el;
  }

  renderError(err) {
    const el = document.createElement('div');
    el.className = 'xui-error';
    el.textContent = `Error: ${err.message}`;
    return el;
  }

  // ── Styles ──────────────────────────────────────────────────
  injectStyles() {
    if (document.getElementById('xavier-ui-styles')) return;
    
    const styles = document.createElement('style');
    styles.id = 'xavier-ui-styles';
    styles.textContent = `
      :root {
        --xui-blue: #3b82f6; --xui-green: #10b981; --xui-yellow: #f59e0b;
        --xui-red: #ef4444; --xui-purple: #8b5cf6; --xui-gray: #6b7280;
        --xui-bg: #111827; --xui-surface: #1f2937; --xui-border: #374151;
        --xui-text: #f9fafb; --xui-text-secondary: #9ca3af;
        --xui-radius: 8px; --xui-spacing: 1rem;
      }
      .xui-data-table, .xui-info-card, .xui-form, .xui-code-block,
      .xui-timeline, .xui-confirm-dialog, .xui-chart-bar, .xui-list-group {
        background: var(--xui-surface);
        border: 1px solid var(--xui-border);
        border-radius: var(--xui-radius);
        padding: var(--xui-spacing);
        margin-bottom: var(--xui-spacing);
        color: var(--xui-text);
        font-family: system-ui, -apple-system, sans-serif;
      }
      .xui-table { width: 100%; border-collapse: collapse; }
      .xui-table th, .xui-table td { padding: 0.5rem; text-align: left; border-bottom: 1px solid var(--xui-border); }
      .xui-table th { color: var(--xui-text-secondary); font-weight: 600; }
      .xui-btn { padding: 0.5rem 1rem; border-radius: 6px; border: none; cursor: pointer; font-size: 0.875rem; }
      .xui-btn-primary { background: var(--xui-blue); color: white; }
      .xui-btn-outline { background: transparent; border: 1px solid var(--xui-border); color: var(--xui-text); }
      .xui-btn-ghost { background: transparent; color: var(--xui-text-secondary); }
      .xui-btn-sm { padding: 0.25rem 0.5rem; font-size: 0.75rem; }
      .xui-btn-danger { background: var(--xui-red); color: white; }
      .xui-badge { padding: 0.25rem 0.5rem; border-radius: 9999px; font-size: 0.75rem; font-weight: 600; }
      .xui-badge-success { background: rgba(16,185,129,0.2); color: var(--xui-green); }
      .xui-badge-warning { background: rgba(245,158,11,0.2); color: var(--xui-yellow); }
      .xui-badge-danger { background: rgba(239,68,68,0.2); color: var(--xui-red); }
      .xui-badge-default { background: rgba(107,114,128,0.2); color: var(--xui-gray); }
      .xui-progress-bar { height: 8px; background: var(--xui-border); border-radius: 4px; overflow: hidden; }
      .xui-progress-fill { height: 100%; background: var(--xui-blue); border-radius: 4px; transition: width 0.3s; }
      .xui-progress-label { display: flex; justify-content: space-between; margin-bottom: 0.5rem; font-size: 0.875rem; }
      .xui-code-block pre { background: #0d1117; padding: 1rem; border-radius: 6px; overflow-x: auto; margin: 0; }
      .xui-code-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.5rem; }
      .xui-form-group { margin-bottom: 1rem; }
      .xui-form-group label { display: block; margin-bottom: 0.25rem; font-size: 0.875rem; color: var(--xui-text-secondary); }
      .xui-input { width: 100%; padding: 0.5rem; background: var(--xui-bg); border: 1px solid var(--xui-border); border-radius: 6px; color: var(--xui-text); }
      .xui-info-card { display: flex; align-items: center; gap: 1rem; }
      .xui-card-icon { font-size: 2rem; width: 48px; height: 48px; display: flex; align-items: center; justify-content: center; border-radius: 12px; background: rgba(59,130,246,0.2); }
      .xui-card-value { font-size: 1.5rem; font-weight: 700; }
      .xui-card-title { font-size: 0.875rem; color: var(--xui-text-secondary); }
      .xui-chart-container { display: flex; align-items: flex-end; gap: 1rem; height: 150px; padding-top: 1rem; }
      .xui-chart-item { flex: 1; display: flex; flex-direction: column; align-items: center; gap: 0.5rem; }
      .xui-chart-bar-visual { width: 100%; border-radius: 4px 4px 0 0; min-height: 4px; }
      .xui-mini-progress { height: 4px; background: var(--xui-border); border-radius: 2px; width: 60px; }
      .xui-mini-progress-fill { height: 100%; background: var(--xui-blue); border-radius: 2px; }
    `;
    document.head.appendChild(styles);
  }
}

// Export
// ES Module export for Vite bundlers
export { XavierUIRenderer };
