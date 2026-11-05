<script lang="ts">
  // 编辑 (design §6.5): plain-text draft editor with a live check. Saving
  // replaces the file only if it is unchanged on disk; no normalization.
  import { onDestroy } from "svelte";
  import type { EditorView } from "@codemirror/view";
  import Icon from "../../components/Icon.svelte";
  import { createEditor, setError, setText } from "../../editor/treespec";
  import { api, message } from "../../ipc/api";
  import type { DraftCheckView } from "../../ipc/bindings/DraftCheckView";
  import type { RoundView } from "../../ipc/bindings/RoundView";
  import { saveEditor } from "../../stores/actions";
  import { app } from "../../stores/app.svelte";

  let { round }: { round: RoundView } = $props();
  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | null = null;
  let check = $state<DraftCheckView | null>(null);
  let error = $state<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const e = $derived(app.editor?.round === round.name ? app.editor : null);

  function schedule(text: string) {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      try {
        check = await api.checkDraft(round.name, text);
        if (view) setError(view, check.line);
      } catch (err) {
        check = null;
        error = message(err);
      }
    }, 300);
  }

  async function load(name: string) {
    try {
      const d = await api.draftText(name);
      app.editor = { round: name, text: d.text, saved: d.text, digest: d.digest, generated: d.generated, external: false };
      error = null;
    } catch (err) {
      error = message(err);
    }
  }

  $effect(() => {
    if (app.editor?.round !== round.name) void load(round.name);
  });

  $effect(() => {
    if (!host || !e || view) return;
    view = createEditor(host, e.text, e.generated, (text) => {
      if (app.editor) app.editor.text = text;
      schedule(text);
    });
    schedule(e.text);
  });

  function reload() {
    app.editor = null;
    view?.destroy();
    view = null;
    void load(round.name);
  }

  function revert() {
    if (view && e) setText(view, e.saved);
  }

  onDestroy(() => {
    clearTimeout(timer);
    view?.destroy();
  });
</script>

{#if error}<div class="notice bad"><Icon name="warn" /><span>{error}</span></div>{/if}
{#if e?.generated}
  <div class="notice"><Icon name="info" /><span>第 1 行是审核标记，只读。编辑后生成计划需要确认审核。</span></div>
{/if}
{#if e?.external}
  <div class="notice bad"><Icon name="warn" /><span>文件已在外部修改。保存会被拒绝：可以重新载入（放弃这里的修改），或先拷贝你的文本再重新载入。</span>
    <button class="btn sm" onclick={reload}>重新载入</button>
    <button class="btn sm" onclick={() => e && navigator.clipboard.writeText(e.text).then(() => app.notify("已拷贝编辑器中的文本"))}>拷贝我的文本</button>
  </div>
{/if}
<div class="ed">
  <div class="cm-host" bind:this={host}></div>
  {#if check}
    <div class="card chk" aria-live="polite">
      {#if check.ok}<b>格式正确</b>{:else}<b>第 {check.line ?? "?"} 行：{check.error}</b>{#if check.hint}<div class="hint">{check.hint}</div>{/if}{/if}
      {#if check.ok}
        <ul><li>新增 {check.added}</li><li>移到新目录 {check.moved}</li><li>改名 {check.renamed}</li><li>说明变化 {check.described}</li></ul>
      {/if}
      {#if check.missing_files.length}
        <div class="notice bad"><Icon name="warn" /><span>将有 {check.missing_files.length} 个文件缺失：{check.missing_files.join("、")}。计划会出现未解决项。保留所有已有的 @ID。</span></div>
      {/if}
      {#if check.new_files.length}
        <div class="notice bad"><Icon name="warn" /><span>新增 {check.new_files.length} 个无来源文件：{check.new_files.join("、")}。草案只能移动已有文件。</span></div>
      {/if}
      {#if check.needs_ack}<div class="hint">生成计划时需要确认审核。</div>{/if}
      {#if check.review_error}<div class="notice bad"><Icon name="warn" /><span>{check.review_error}</span></div>{/if}
      <div class="hint">停止输入 300 ms 后检查，不写盘。</div>
    </div>
  {/if}
</div>
<div class="row" style="margin-top:12px">
  <span class="muted tiny">{app.dirty && e ? "有未保存的修改" : "已保存"} · drafts/{round.name}</span>
  <span class="sp"></span>
  <button class="btn" onclick={() => api.system({ kind: "open_with", round: round.name }).catch((err) => app.notify(message(err)))}>用其他应用打开</button>
  <button class="btn" disabled={!app.dirty || !e} onclick={revert}>还原</button>
  <button class="btn pri" disabled={!app.dirty || !e || e.external || !!app.busy} onclick={saveEditor}>保存 ⌘S</button>
</div>
