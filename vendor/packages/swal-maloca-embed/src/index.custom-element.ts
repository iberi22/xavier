import SwalMalocaPanelCE from './lib/SwalMalocaPanelCE.svelte';

if (typeof window !== 'undefined') {
  if (!customElements.get('swal-maloca-panel')) {
    customElements.define('swal-maloca-panel', SwalMalocaPanelCE as any);
  }
}

export default SwalMalocaPanelCE;
