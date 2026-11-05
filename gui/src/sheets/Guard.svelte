<script lang="ts">
  // Close/quit guards (design §8.3, §10) and “unsaved changes” before leaving
  // the editor. The safe choice is always the default button.
  import Sheet from "../components/Sheet.svelte";
  import { api } from "../ipc/api";
  import { saveEditor } from "../stores/actions";
  import { app } from "../stores/app.svelte";

  const s = $derived(app.sheet);
  const close = () => (app.sheet = null);

  async function saveThen(next: () => void) {
    await saveEditor();
    if (!app.dirty) next();
  }
  function discard(next: () => void) {
    app.editor = null;
    void api.setGuard(false);
    next();
  }
  /** Runs the pending navigation; read before closing, since `s` follows app.sheet. */
  function leave() {
    const then = app.sheet?.kind === "unsaved" ? app.sheet.then : null;
    app.sheet = null;
    then?.();
  }
  const force = (quit: boolean) => () => {
    app.sheet = null;
    void api.forceClose(quit);
  };
</script>

{#if s?.kind === "guard"}
  {@const what = s.guard.quit ? "退出" : "关闭窗口"}
  {#if s.guard.guard === "applying"}
    <Sheet title="迁移正在进行" cancelLabel="继续等待" oncancel={close}>
      {#if s.guard.quit}
        <p style="margin:0">现在退出会留下部分完成的迁移，需要人工恢复。等它完成通常更安全。</p>
      {:else}
        <p style="margin:0">迁移完成之前不能关闭这个窗口：关闭不会停止迁移，只会让它失去保护。</p>
      {/if}
      {#snippet actions()}{#if s.guard.quit}<button class="btn danger" onclick={force(true)}>仍然退出</button>{/if}{/snippet}
    </Sheet>
  {:else if s.guard.guard === "chatting"}
    <Sheet title="正在生成草案" cancelLabel="继续等待" oncancel={close}>
      <p style="margin:0">现在{what}会放弃这次草案生成。最坏情况只留下一个孤立的审核报告，可以之后归档。</p>
      {#snippet actions()}<button class="btn danger" onclick={force(s.guard.quit)}>仍然{what}</button>{/snippet}
    </Sheet>
  {:else}
    <Sheet title="草案有未保存的修改" oncancel={close}>
      <p style="margin:0">{what}前要保存 drafts/{app.editor?.round} 吗？</p>
      {#snippet actions()}
        <button class="btn" onclick={() => discard(force(s.guard.quit))}>不保存</button>
        <button class="btn pri" onclick={() => saveThen(force(s.guard.quit))}>保存</button>
      {/snippet}
    </Sheet>
  {/if}
{:else if s?.kind === "unsaved"}
  <Sheet title="草案有未保存的修改" oncancel={close}>
    <p style="margin:0">离开编辑器前要保存 drafts/{app.editor?.round} 吗？</p>
    {#snippet actions()}
      <button class="btn" onclick={() => discard(leave)}>不保存</button>
      <button class="btn pri" onclick={() => saveThen(leave)}>保存</button>
    {/snippet}
  </Sheet>
{/if}
