<script lang="ts">
  // 恢复 (design §6.7): read-only check of unfinished runs; finish Git or close
  // a run after manual checking. No resume, no rollback.
  import Icon from "../components/Icon.svelte";
  import { api, message } from "../ipc/api";
  import { OPS } from "../lib/format";
  import { refresh } from "../stores/actions";
  import { app } from "../stores/app.svelte";

  const v = $derived(app.view!);
  let index = $state(0);
  const r = $derived(v.recoveries[Math.min(index, v.recoveries.length - 1)] ?? null);
  const reveal = (path: string) => r && api.reveal({ kind: "step", run: r.run_id, path }).catch((e) => app.notify(message(e)));
</script>

{#if !r}
  <div class="empty" style="padding-top:120px">没有未完成的迁移。<br />迁移执行失败或被中断时，这里会显示逐步状态。</div>
{:else}
  {#if v.recoveries.length > 1}
    <div class="row" style="margin-bottom:10px">
      <label class="row tiny">未完成的运行
        <select class="inp" style="width:auto" bind:value={index}>
          {#each v.recoveries as x, i (x.run_id)}<option value={i}>{x.run_id}（{x.status}）</option>{/each}
        </select>
      </label>
    </div>
  {/if}
  <div class="card hero bad">
    <div class="big"><Icon name="warn" /></div>
    <div style="flex:1"><h2 class="mono">{r.run_id}</h2><div class="muted">状态 {r.status} · 已确认 {r.completed}/{r.total} · plan {r.plan_id}…</div></div>
  </div>
  {#if r.error}<h3>错误</h3><div class="err">{r.error}</div>{#if r.hint}<div class="hint">{r.hint}</div>{/if}{/if}
  {#if r.report_error}<div class="notice bad"><Icon name="warn" /><span>无法检查这个运行：{r.report_error}</span></div>{/if}
  {#if r.steps.length}
    <h3>步骤</h3>
    <div class="list">
      <table>
        <thead><tr><th>#</th><th>状态</th><th>操作</th><th>当前磁盘</th><th></th></tr></thead>
        <tbody>
          {#each r.steps as s (s.position)}
            <tr>
              <td>{s.position}</td>
              <td><span class="badge {s.status === 'done' ? 'ok' : 'warn'}">{s.status}</span></td>
              <td><span class="mono">{s.op.kind}</span> <span class="muted tiny">{OPS[s.op.kind]}</span><br /><span class="mono selectable">{s.op.from ? `${s.op.from} → ${s.op.to}` : s.op.to}</span></td>
              <td>{s.disk}</td>
              <td class="row">
                {#if s.op.from}<button class="btn sm" onclick={() => reveal(s.op.from!)}>显示源</button>{/if}
                <button class="btn sm" onclick={() => reveal(s.op.to)}>显示目标</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
  {#if r.guidance}<p class="hint">{r.guidance}</p>{/if}
  <div class="notice"><Icon name="info" /><span>只读检查。pending 可能已经落盘但未确认，不要重新执行计划。部分完成的文件步骤需要人工核对；GUI 不提供续跑或回滚。“当前磁盘”只是供核对的提示，不代表身份确认。</span></div>
  <div class="row">
    <button class="btn" disabled={!!app.busy} onclick={refresh}>重新检查</button>
    <span class="sp"></span>
    <button class="btn" disabled={!r.finish_git.ok || !!app.busy} title={r.finish_git.reason ?? ""} onclick={() => (app.sheet = { kind: "finish_git", run: r.run_id })}>补记 Git…</button>
    <button class="btn pri" disabled={!r.mark_resolved.ok || !!app.busy} title={r.mark_resolved.reason ?? ""} onclick={() => (app.sheet = { kind: "resolve", run: r.run_id })}>标记已人工处理…</button>
  </div>
{/if}
