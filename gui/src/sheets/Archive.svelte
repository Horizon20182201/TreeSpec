<script lang="ts">
  // 归档 (design §7.6) and the “new draft” chooser.
  import Cmd from "./Cmd.svelte";
  import Sheet from "../components/Sheet.svelte";
  import { newDraft } from "../stores/actions";
  import { app } from "../stores/app.svelte";

  const s = $derived(app.sheet);
  const v = $derived(app.view!);
  let invalid = $state<string | null>(null);
  const close = () => (app.sheet = null);
  // “Archive everything” runs as `--all`; one round lists its files.
  const selectors = $derived(s?.kind === "archive" && s.round ? s.selectors : []);
</script>

{#if s?.kind === "archive"}
  <Sheet title={s.round ? `归档 ${s.round}` : "全部归档"} oncancel={close} note="Return 键 = 取消">
    <p class="muted" style="margin:0 0 8px">以下文件将移到 <span class="mono">.organizer/archive/&lt;时间&gt;-&lt;名称&gt;/</span>，不删除，不覆盖：</p>
    <div class="list" style="max-height:220px;overflow:auto">
      {#each s.selectors as x (x)}<div class="li"><span class="grow mono">{x}</span></div>{/each}
    </div>
    <p class="hint">归档后这些文件不再阻止同步；它们在 .organizer 内部，不会被扫描成用户文件。</p>
    <Cmd action={{ kind: "archive", selectors }} onerror={(e) => (invalid = e)} />
    {#snippet actions()}
      <button class="btn pri" disabled={!!invalid || !!app.busy} onclick={() => app.run({ kind: "archive", selectors }, "正在归档")}>归档 {s.selectors.length} 个文件</button>
    {/snippet}
  </Sheet>
{:else if s?.kind === "new"}
  <Sheet title="新建草案" oncancel={close}>
    <div class="row" style="align-items:stretch">
      <button class="card" style="flex:1;text-align:left" disabled={!v.llm.available || !v.gates.draft.ok} onclick={() => newDraft("chat")}>
        <b>用 LLM 生成</b>
        <div class="muted tiny">描述想要的整理方式，模型提出完整的目标结构和文件归属。</div>
        {#if !v.llm.available}<div class="tiny" style="color:var(--warn);margin-top:4px">{v.llm.reason}</div>{/if}
      </button>
      <button class="card" style="flex:1;text-align:left" disabled={!v.gates.copy_baseline.ok} onclick={() => newDraft("copy")}>
        <b>复制当前结构</b>
        <div class="muted tiny">在 drafts/ 下新建一份基线副本，用内置编辑器修改。</div>
      </button>
    </div>
  </Sheet>
{/if}
