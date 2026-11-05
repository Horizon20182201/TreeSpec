<script lang="ts">
  // 审核 (design §6.4): the annotated target tree.
  import Icon from "../../components/Icon.svelte";
  import type { RoundView } from "../../ipc/bindings/RoundView";
  import { MARKS } from "../../lib/format";
  import { archive, plan } from "../../stores/actions";
  import { app } from "../../stores/app.svelte";

  let { round }: { round: RoundView } = $props();
  const v = $derived(app.view!);
  const d = $derived(app.detail?.name === round.name ? app.detail : null);
  const done = $derived(round.stage_code === "applied" || round.stage_code === "nothing_to_apply");
  const rows = $derived.by(() => {
    if (!d) return [];
    if (!app.onlyChanges) return d.rows;
    const keep = d.rows.map((r) => !!r.mark);
    for (let i = d.rows.length - 1; i >= 0; i--) {
      if (!keep[i]) continue;
      for (let j = i - 1, depth = d.rows[i].depth; j >= 0 && depth > 0; j--) {
        if (d.rows[j].depth < depth) {
          keep[j] = true;
          depth = d.rows[j].depth;
        }
      }
    }
    return d.rows.filter((_, i) => keep[i]);
  });
</script>

{#if done}
  <div class="notice info"><Icon name="info" /><span>这一轮已{round.stage}，目标结构与当前基线相同。草案和计划会阻止同步，用完后请归档。</span></div>
  <div class="row"><button class="btn pri" onclick={() => archive(round.name)}>归档本轮…</button></div>
{:else if !d}
  <div class="empty">正在载入…</div>
{:else}
  {#if round.ack}
    <div class="notice"><Icon name="warn" /><span><b>需确认审核：</b>{round.ack}。生成计划前需要确认你已检查完整草案和报告；这一步不绕过路径、身份和冲突检查。</span></div>
  {/if}
  {#if d.invalid}
    <div class="notice bad"><Icon name="warn" /><span><b>{round.stage}：</b>{d.invalid}</span></div>
  {/if}
  <div class="row" style="margin:8px 0">
    {#each d.counts as c (c.mark)}
      <span class="badge {MARKS[c.mark]?.[0] ?? ''}"><b>{c.mark}</b>{MARKS[c.mark]?.[1]} {c.count}</span>
    {:else}<span class="muted">与基线相比没有变化</span>{/each}
    <span class="sp"></span>
    <label class="row tiny"><input type="checkbox" bind:checked={app.onlyChanges} />只看变化</label>
  </div>
  <div class="tree" role="tree" aria-label="目标结构">
    {#each rows as r (r.id)}
      {@const m = r.mark ? MARKS[r.mark] : null}
      <div class="tr two" role="treeitem" aria-level={r.depth + 1} aria-selected={app.selected === r.id} tabindex="0"
        style="padding-left:{12 + r.depth * 18}px" onclick={() => (app.selected = r.id)} onkeydown={(e) => e.key === "Enter" && (app.selected = r.id)}>
        <span class="nm"><Icon name={r.dir ? "folder" : "file"} class="ic" /><span class="n">{r.name}{r.dir ? "/" : ""}</span>{#if r.excluded}<span class="lock" title="不发送给 LLM"><Icon name="lock" class="ic" /></span>{/if}</span>
        <span>{#if m}<span class="badge {m[0]}" title={m[1]}><b>{r.mark}</b>{r.note ?? m[1]}</span>{/if}</span>
      </div>
    {/each}
  </div>
  {#if d.missing.length}
    <h3>目标中缺失的节点</h3>
    <div class="list">{#each d.missing as path (path)}<div class="li"><span class="badge bad"><b>!</b>缺失</span><span class="grow mono">{path}</span></div>{/each}</div>
    <p class="hint">缺失不是删除命令：计划会把它们列为未解决项。保留所有已有节点和 @ID。</p>
  {/if}
  <div class="row" style="margin-top:14px">
    <button class="btn" onclick={() => (app.segment = "edit")}>编辑草案</button>
    <span class="sp"></span>
    <button class="btn" disabled={!v.gates.archive.ok} onclick={() => archive(round.name)}>归档本轮…</button>
    <button class="btn pri" disabled={!v.gates.plan.ok || !!d.invalid} title={v.gates.plan.reason ?? d.invalid ?? ""} onclick={() => plan(round.name)}>{round.active ? "重新生成计划…" : "生成计划…"}</button>
  </div>
{/if}
