<script lang="ts">
  // 报告 (design §6.4): the LLM review report. Reasons are model output.
  import { app } from "../../stores/app.svelte";

  const r = $derived(app.detail?.report ?? null);
</script>

{#if !r}
  <div class="empty">这一轮没有审核报告。</div>
{:else}
  <div class="card">
    <dl class="kv" style="margin:0;grid-template-columns:110px 1fr">
      <dt>编辑数</dt><dd>{r.edit_count}</dd>
      <dt>读取正文</dt><dd>{r.bytes_read} 字节{r.bytes_read ? "" : "（未发送文本片段）"}</dd>
      <dt>基线摘要</dt><dd class="mono">{r.base_digest}</dd>
      <dt>目标摘要</dt><dd class="mono">{r.target_digest}</dd>
    </dl>
  </div>
  <h3>分配（{r.assignments.length}）</h3>
  {#if r.assignments.length}
    <div class="list">
      <table>
        <thead><tr><th>文件</th><th>决定</th><th>理由（模型输出，仅供参考）</th></tr></thead>
        <tbody>
          {#each r.assignments as a, i (i)}
            <tr><td class="mono selectable">{a.file}{#if a.target}<br /><span class="muted">→ {a.target}</span>{/if}</td>
              <td><span class="badge {a.decision === 'review' ? 'rev' : a.decision === 'place' ? 'acc' : ''}">{a.decision}</span></td>
              <td class="selectable">{a.reason}</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}<div class="muted">没有文件分配（只有结构编辑）。</div>{/if}
  <h3>待审核</h3>
  {#if r.unresolved.length}<ul class="chk">{#each r.unresolved as u, i (i)}<li>{u}</li>{/each}</ul>{:else}<div class="muted">无</div>{/if}
  <h3>警告</h3>
  {#if r.warnings.length}<div class="err">{r.warnings.join("\n")}</div>{:else}<div class="muted">无</div>{/if}
{/if}
