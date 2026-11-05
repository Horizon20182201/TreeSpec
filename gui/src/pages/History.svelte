<script lang="ts">
  // 历史 (design §6.6): structure commits and migration runs.
  import { app } from "../stores/app.svelte";

  const v = $derived(app.view!);
  const tone: Record<string, string> = { complete: "ok", resolved: "", failed: "bad" };
</script>

<h2>历史</h2>
<p class="sub">结构提交只记录 structure.tree，不包含文件内容。</p>
<div class="cols" style="margin-top:0">
  <section>
    <h3>结构提交</h3>
    {#if v.history_error}<div class="err">{v.history_error}</div>{/if}
    <div class="list">
      {#each v.commits as c (c.hash)}
        <div class="li"><span class="mono">{c.hash}</span><span class="badge {c.kind === 'refactor' ? 'acc' : ''}">{c.kind}</span><span class="grow selectable" title={c.subject}>{c.subject}</span></div>
      {:else}<div class="empty">没有提交</div>{/each}
    </div>
  </section>
  <section>
    <h3>迁移运行</h3>
    <div class="list">
      {#each v.runs as r (r.run_id)}
        <button class="li" aria-pressed={app.selected === r.run_id} onclick={() => (app.selected = r.run_id)}>
          <span class="grow mono">{r.run_id}</span><span class="muted tiny">{r.completed}/{r.total}</span><span class="badge {tone[r.status] ?? 'warn'}">{r.status}</span>
        </button>
      {:else}<div class="empty">还没有执行过迁移</div>{/each}
    </div>
  </section>
</div>
<p class="hint">之后的版本将支持按提交查看当时的结构，并与当前基线比较。</p>
