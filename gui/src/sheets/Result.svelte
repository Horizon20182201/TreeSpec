<script lang="ts">
  // A failed action: the library's English error, the Chinese hint, and the
  // command that failed (TUI design §12 decision 6).
  import Command from "../components/Command.svelte";
  import Sheet from "../components/Sheet.svelte";
  import { goto } from "../stores/actions";
  import { app } from "../stores/app.svelte";

  const r = $derived(app.sheet?.kind === "result" ? app.sheet.result : null);
</script>

{#if r}
  <Sheet title={r.message || `${r.label}失败`} cancelLabel="关闭" oncancel={() => (app.sheet = null)}>
    {#if r.error}<div class="err">{r.error}</div>{/if}
    {#if r.hint}<div class="hint">{r.hint}</div>{/if}
    {#if r.go_recovery}<p class="muted tiny">执行开始后失败：可能已有部分移动完成。不要重跑计划，到恢复页检查。</p>{/if}
    <Command command={r.command} />
    {#snippet actions()}
      {#if r.go_recovery}<button class="btn pri" onclick={() => ((app.sheet = null), goto("recovery"))}>前往恢复</button>{/if}
    {/snippet}
  </Sheet>
{/if}
