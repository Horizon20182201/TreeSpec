<script lang="ts">
  import Apply from "./Apply.svelte";
  import Basic from "./Basic.svelte";
  import Chat from "./Chat.svelte";
  import Guard from "./Guard.svelte";
  import Plan from "./Plan.svelte";
  import Result from "./Result.svelte";
  import Settings from "./Settings.svelte";
  import { app } from "../stores/app.svelte";

  const kind = $derived(app.sheet?.kind ?? null);
</script>

{#if kind === "apply" || kind === "apply_run" || kind === "apply_done"}<Apply />
{:else if kind === "chat" || kind === "chat_wait"}<Chat />
{:else if kind === "plan"}<Plan />
{:else if kind === "settings"}<Settings />
{:else if kind === "guard" || kind === "unsaved"}<Guard />
{:else if kind === "result"}<Result />
{:else if kind}<Basic />{/if}
