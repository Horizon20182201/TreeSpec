<script lang="ts">
  // 计划 (design §6.4): operations, preview, export and apply.
  import type { RoundView } from "../../ipc/bindings/RoundView";
  import { OPS, stageTone } from "../../lib/format";
  import { apply, archive, dryRun, plan } from "../../stores/actions";
  import { app } from "../../stores/app.svelte";

  let { round }: { round: RoundView } = $props();
  const v = $derived(app.view!);
  const p = $derived(app.detail?.name === round.name ? app.detail.plan : null);
  const executable = $derived(p?.stage_code === "executable");
  const done = $derived(p?.stage_code === "applied" || p?.stage_code === "nothing_to_apply");
  const preview = $derived(p ? app.previews[p.name] : undefined);
</script>

{#if !p}
  <div class="empty" style="padding:60px 0">
    还没有与当前草案配对的计划。<br /><br />
    <button class="btn pri" disabled={!v.gates.plan.ok} title={v.gates.plan.reason ?? ""} onclick={() => plan(round.name)}>生成计划…</button>
  </div>
{:else}
  <div class="card">
    <dl class="kv" style="margin:0">
      <dt>计划</dt><dd class="mono">plans/{p.name} · plan {p.sha}…</dd>
      <dt>阶段</dt><dd><span class="badge {stageTone(p.stage_code)}">{p.stage}</span></dd>
      <dt>预览</dt>
      <dd>
        {#if done}已执行
        {:else if preview}<span class="badge ok">{preview} 通过</span> <span class="muted tiny">执行前会重新检查</span>
        {:else}<span class="muted">尚未预览</span>{/if}
      </dd>
    </dl>
  </div>
  <h3>操作（{p.ops.length}）</h3>
  {#if p.ops.length}
    <div class="list">
      <table>
        <thead><tr><th>#</th><th>类型</th><th>源 → 目标</th></tr></thead>
        <tbody>
          {#each p.ops as op, i (i)}
            <tr><td>{i + 1}</td><td><span class="mono">{op.kind}</span><br /><span class="muted tiny">{OPS[op.kind]}</span></td>
              <td class="mono selectable">{op.from ?? op.to}{#if op.from}<br /><span class="muted">→ {op.to}</span>{/if}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}<div class="muted">只更新结构记录，没有文件操作。</div>{/if}
  <h3>未解决</h3>
  {#if p.unresolved.length}<div class="err">{p.unresolved.join("\n")}</div>{:else}<div class="muted">无</div>{/if}
  {#if p.warnings.length}<h3>警告</h3><div class="err">{p.warnings.join("\n")}</div>{/if}
  <h3>扫描边界</h3>
  {#if p.boundaries.length}
    {#each p.boundaries as b, i (i)}<div class="muted tiny">[{b.tag}] {b.path} · {b.note}</div>{/each}
  {:else}<div class="muted">无</div>{/if}
  <div class="row" style="margin-top:16px">
    <button class="btn" disabled={done || !!p.script} title={p.script ? `已导出 plans/${p.script}` : ""} onclick={() => (app.sheet = { kind: "export", plan: p.name })}>导出脚本…</button>
    <span class="sp"></span>
    {#if done}
      <button class="btn pri" onclick={() => archive(round.name)}>归档本轮…</button>
    {:else}
      <button class="btn" disabled={!executable || !v.gates.plan.ok || !!app.busy} title={v.gates.plan.reason ?? ""} onclick={() => dryRun(round.name)}>预览</button>
      <button class="btn pri" disabled={!executable || !v.gates.plan.ok || !!app.busy} title={executable ? (v.gates.plan.reason ?? "") : p.stage} onclick={() => apply(round.name)}>执行迁移…</button>
    {/if}
  </div>
{/if}
