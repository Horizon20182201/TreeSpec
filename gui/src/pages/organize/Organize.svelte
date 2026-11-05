<script lang="ts">
  // 整理 (design §6.4): rounds, step bar and the four segments.
  import Editor from "./Editor.svelte";
  import Plan from "./Plan.svelte";
  import Report from "./Report.svelte";
  import Review from "./Review.svelte";
  import { stageTone } from "../../lib/format";
  import { archiveAll, guarded, newDraft } from "../../stores/actions";
  import { app, type Segment } from "../../stores/app.svelte";

  const v = $derived(app.view!);
  const r = $derived(app.currentRound);
  const segments = $derived<[Segment, string, boolean][]>([
    ["review", "审核", true],
    ["edit", "编辑", !!r],
    ["plan", "计划", true],
    ["report", "报告", !!r?.generated],
  ]);
  $effect(() => {
    if (!segments.find(([s, , ok]) => s === app.segment && ok)) app.segment = "review";
  });
  const at = $derived(
    !r ? 0 : r.stage_code === "applied" || r.stage_code === "nothing_to_apply" ? 4 : r.stage_code === "executable" ? 3 : app.segment === "plan" ? 2 : 1,
  );
  const STEPS = ["草案", "审核", "计划", "执行", "归档"];
  const pick = (name: string) => guarded(() => app.selectRound(name, app.segment));
</script>

<div class="org">
  <nav class="rounds" aria-label="整理轮次">
    <h6>轮次</h6>
    {#each v.rounds as round (round.name)}
      <button class="rd" aria-current={round.name === app.round} onclick={() => pick(round.name)}>
        <b>{round.name}{app.editor?.round === round.name && app.dirty ? " •" : ""}</b>
        <div class="row">
          <span class="badge {round.blocked ? 'warn' : stageTone(round.stage_code)}">{round.blocked ?? round.stage}</span>
          {#if round.ack}<span class="badge rev" title={round.ack}>需确认审核</span>{/if}
          <span class="muted tiny">{round.generated ? "LLM" : "手工"}</span>
        </div>
      </button>
    {:else}
      <div class="empty">没有草案</div>
    {/each}
    {#if v.unpaired.length}
      <h6>未关联计划</h6>
      {#each v.unpaired as p (p.name)}
        <div class="rd" title="草案已改变或不存在：只能查看和归档，不能执行"><b>{p.name}</b><div class="row"><span class="badge warn">{p.stage}</span><button class="btn sm" onclick={() => (app.sheet = { kind: "archive", round: null, selectors: [`plans/${p.name}`, ...(p.script ? [`plans/${p.script}`] : [])] })}>归档…</button></div></div>
      {/each}
    {/if}
    {#if v.others.length}
      <h6>其他文件</h6>
      {#each v.others as x (x.selector)}<div class="rd"><b class="mono">{x.selector}</b><div class="row"><span class="muted tiny">{x.note}</span></div></div>{/each}
    {/if}
    <div class="foot">
      <button class="btn sm" disabled={!v.gates.draft.ok && !v.gates.copy_baseline.ok} title={v.gates.draft.reason ?? ""} onclick={() => newDraft("menu")}>＋ 新建</button>
      <button class="btn sm" disabled={!v.gates.archive.ok} title={v.gates.archive.reason ?? ""} onclick={archiveAll}>全部归档…</button>
    </div>
  </nav>
  <div class="detail">
    {#if r}
      <div class="steps" aria-label="进度">
        {#each STEPS as label, i (label)}
          {#if i}<em></em>{/if}
          <span class:done={i < at} class:cur={i === at}><i>{i < at ? "✓" : i + 1}</i>{label}</span>
        {/each}
      </div>
      <div class="row" style="margin-bottom:6px">
        <h2 style="flex:1;word-break:break-all">{r.name}</h2>
        <div class="seg" role="tablist" aria-label="视图">
          {#each segments as [s, label, ok] (s)}
            <button role="tab" aria-selected={app.segment === s} disabled={!ok} onclick={() => (app.segment = s)}>{label}</button>
          {/each}
        </div>
      </div>
      {#if app.segment === "edit"}<Editor round={r} />
      {:else if app.segment === "plan"}<Plan round={r} />
      {:else if app.segment === "report"}<Report />
      {:else}<Review round={r} />{/if}
    {:else}
      <div class="empty" style="padding-top:120px">
        {v.rounds.length ? "选择一个轮次" : "没有进行中的整理轮次。"}<br /><br />
        {#if v.gates.draft.ok || v.gates.copy_baseline.ok}
          <button class="btn pri" disabled={!v.gates.draft.ok || !v.llm.available} onclick={() => newDraft("chat")}>新建 LLM 草案…</button>
          <button class="btn" disabled={!v.gates.copy_baseline.ok} onclick={() => newDraft("copy")}>复制当前结构为草案…</button>
        {:else}{v.gates.draft.reason}{/if}
      </div>
    {/if}
  </div>
</div>
