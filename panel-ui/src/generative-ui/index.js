/**
 * Xavier2 Generative UI — Entry Point
 * 
 * Uso:
 *   const ui = new XavierUIRenderer('container', { theme: 'dark' });
 *   ui.render({
 *     component: 'data-table',
 *     title: 'Proyectos',
 *     columns: [...],
 *     rows: [...]
 *   });
 */

import { XavierUIRenderer } from './renderer.js';

export { XavierUIRenderer };

// Auto-init si hay un container con id="xavier-ui"
if (typeof window !== 'undefined') {
  document.addEventListener('DOMContentLoaded', () => {
    const container = document.getElementById('xavier-ui');
    if (container) {
      window.xavierUI = new XavierUIRenderer('xavier-ui', {
        theme: 'auto',
        onAction: (event) => {
          console.log('[Xavier2] UI Action:', event);
          // Enviar al agente via WebSocket/API
          if (window.xavierAgent) {
            window.xavierAgent.send({ type: 'ui_action', payload: event });
          }
        },
        onSubmit: (event) => {
          console.log('[Xavier2] Form Submit:', event);
          if (window.xavierAgent) {
            window.xavierAgent.send({ type: 'form_submit', payload: event });
          }
        }
      });
    }
  });
}
