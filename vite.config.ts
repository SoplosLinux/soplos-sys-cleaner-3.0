import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port and ignores the src-tauri directory for HMR
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  // Two pages: the advanced program (index.html) and the basic one.
  build: {
    rollupOptions: {
      input: {
        main: "index.html",
        basic: "basic.html",
      },
    },
  },
  server: {
    port: 1424,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
