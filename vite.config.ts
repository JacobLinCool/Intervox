import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "path";

export default defineConfig({
  plugins: [svelte()],
  resolve: { alias: { $lib: resolve(__dirname, "src/lib") } },
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        captions: resolve(__dirname, "captions.html"),
      },
    },
  },
});
