// swal-ui vite.config.ts (svelte-package or vite.config)
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import dts from 'vite-plugin-dts';
import { resolve } from 'path';

export default defineConfig({
  plugins: [
    svelte(),
    dts({
      include: ['src'],
      rollupTypes: true,
      tsconfigPath: './tsconfig.json'
    }),
  ],
  build: {
    outDir: 'dist',
    lib: {
      entry: resolve(__dirname, 'src/index.ts'),
      name: 'SwalUI',
      formats: ['es'],
      fileName: 'ui',
    },
    rollupOptions: {
      // Externalize Svelte and peer dependencies
      external: [
        /^svelte(\/.*)?$/,
        'd3'
      ],
      output: {
        assetFileNames: (assetInfo) => {
          if (assetInfo.name === 'style.css') {
            return 'ui.css';
          }
          return assetInfo.name || '[name].[ext]';
        }
      }
    },
  },
});
