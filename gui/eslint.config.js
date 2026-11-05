import js from "@eslint/js";
import ts from "typescript-eslint";
import svelte from "eslint-plugin-svelte";
import globals from "globals";

export default ts.config(
  { ignores: ["dist/", "node_modules/", "src-tauri/", "src/ipc/bindings/", "test-results/", "playwright-report/"] },
  js.configs.recommended,
  ...ts.configs.recommended,
  ...svelte.configs.recommended,
  { languageOptions: { globals: { ...globals.browser } } },
  { files: ["**/*.svelte", "**/*.svelte.ts"], languageOptions: { parserOptions: { parser: ts.parser } } },
  {
    rules: {
      // Untrusted names, descriptions, LLM reasons and errors are rendered as text only (GUI design §8.2).
      "svelte/no-at-html-tags": "error",
      // Sets/Maps here are local temporaries inside $derived and helpers, never state.
      "svelte/prefer-svelte-reactivity": "off",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
    },
  },
);
