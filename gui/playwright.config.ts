import { defineConfig } from "@playwright/test";

// Runs the page with the in-memory backend (`--mode mock`); see tests/e2e.
export default defineConfig({
  testDir: "tests/e2e",
  use: { baseURL: "http://localhost:4173", viewport: { width: 1280, height: 820 } },
  webServer: { command: "npm run build:mock && npx vite preview --port 4173 --strictPort", port: 4173, reuseExistingServer: true },
});
