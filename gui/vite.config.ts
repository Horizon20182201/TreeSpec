import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// `--mode mock` builds the page with an in-memory backend for browser previews
// and Playwright tests (tests/e2e). The Tauri build never includes it.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "safari16", sourcemap: false },
});
