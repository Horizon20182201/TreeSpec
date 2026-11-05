<script lang="ts">
  // 生成计划 (design §7.5): only new files; review acknowledgement when needed.
  import Cmd from "./Cmd.svelte";
  import Icon from "../components/Icon.svelte";
  import Sheet from "../components/Sheet.svelte";
  import { app } from "../stores/app.svelte";

  const round = $derived(app.sheet?.kind === "plan" ? app.currentRound : null);
  const name = $derived(app.sheet?.kind === "plan" ? app.sheet.round : "");
  const r = $derived(app.view?.rounds.find((x) => x.name === name) ?? round);
  let acknowledge = $state(false);
  let invalid = $state<string | null>(null);
  const out = $derived.by(() => {
    const stem = name.replace(/\.tree$/, "") + (r?.ack ? ".reviewed" : "");
    const taken = new Set([...(app.view?.rounds.flatMap((x) => x.plans.map((p) => p.name)) ?? []), ...(app.view?.unpaired.map((p) => p.name) ?? [])]);
    let file = `${stem}.json`;
    for (let i = 2; taken.has(file); i++) file = `${stem}-${i}.json`;
    return file;
  });
  const action = $derived({ kind: "plan" as const, round: name, acknowledge: !!r?.ack && acknowledge, out });
</script>

{#if r}
  <Sheet title="为 {name} 生成计划" oncancel={() => (app.sheet = null)} note="Return 键 = 取消">
    {#if r.ack}
      <div class="notice"><Icon name="warn" /><span><b>需确认审核：</b>{r.ack}。</span></div>
      <label class="ck"><input type="checkbox" bind:checked={acknowledge} /><span>我已检查完整草案和报告；未分类文件保持原有归属。<br /><span class="muted tiny">这一步不绕过路径、身份、源文件和冲突检查。</span></span></label>
    {/if}
    <div class="fld"><span>输出</span><span class="mono">plans/{out}</span></div>
    <p class="muted tiny" style="margin:0">计划有未解决项时不写文件，只列出问题。</p>
    <Cmd {action} onerror={(e) => (invalid = e)} />
    {#snippet actions()}
      <button class="btn pri" disabled={(!!r.ack && !acknowledge) || !!invalid || !!app.busy} onclick={() => app.run(action, "正在生成计划")}>生成计划</button>
    {/snippet}
  </Sheet>
{/if}
