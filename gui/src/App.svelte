<script lang="ts">
  import { onMount } from "svelte";
  import { api, events, message } from "./ipc/api";
  import type { WindowInfo } from "./ipc/bindings/WindowInfo";
  import Welcome from "./pages/Welcome.svelte";
  import Workspace from "./pages/Workspace.svelte";

  let info = $state<WindowInfo | null>(null);
  let error = $state<string | null>(null);

  async function load() {
    try {
      info = await api.windowInfo();
    } catch (e) {
      error = message(e);
    }
  }

  onMount(() => {
    let off: (() => void) | undefined;
    void events.welcome(() => void load()).then((u) => (off = u));
    void load();
    return () => off?.();
  });
</script>

{#if info?.kind === "workspace"}
  <Workspace inspector={info.inspector} />
{:else if info?.kind === "welcome"}
  <Welcome {info} reload={load} />
{:else if error}
  <div class="empty" style="padding-top:120px">{error}</div>
{/if}
