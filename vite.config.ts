import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port and ignores the src-tauri directory for HMR
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  // Two pages: the advanced program (index.html) and the basic one.
  // The page is loaded from the disk by a desktop window, so the size of a
  // chunk does not matter the way it does on the web: no warning about it.
  build: {
    chunkSizeWarningLimit: 2000,
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
