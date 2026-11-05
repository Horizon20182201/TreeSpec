<script lang="ts">
  // Equivalent command for a pending action, built by Rust from the same
  // `Action` that will run (design §8.1). Validation errors show here early.
  import Command from "../components/Command.svelte";
  import { api, message } from "../ipc/api";
  import type { GuiAction } from "../ipc/bindings/GuiAction";

  let { action, onerror }: { action: GuiAction; onerror?: (e: string | null) => void } = $props();
  let command = $state<string | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    const a = $state.snapshot(action) as GuiAction;
    let live = true;
    const t = setTimeout(() => {
      api.command(a).then(
        (c) => live && ((command = c), (error = null), onerror?.(null)),
        (e) => live && ((command = null), (error = message(e)), onerror?.(error)),
      );
    }, 120);
    return () => {
      live = false;
      clearTimeout(t);
    };
  });
</script>

{#if error}<div class="notice bad"><span>{error}</span></div>{:else}<Command {command} />{/if}
