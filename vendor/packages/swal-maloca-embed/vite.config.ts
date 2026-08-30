import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [
    svelte({
      compilerOptions: {
        customElement: true
      }
    })
  ],
  build: {
    emptyOutDir: false,
    lib: {
      entry: 'src/index.custom-element.ts',
      name: 'MalocaEmbed',
      fileName: 'maloca-embed',
      formats: ['es', 'umd'],
    },
    rollupOptions: {
      external: [
        '@swal/maloca-wasm/main'
      ]
    }
  }
});
