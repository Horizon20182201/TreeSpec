<script lang="ts">
  // Details of the selection on the current page (design §4.2).
  import { api, message } from "../ipc/api";
  import { app } from "../stores/app.svelte";

  const v = $derived(app.view);
  const node = $derived(v?.nodes.find((n) => n.id === app.selected) ?? null);
  const row = $derived(app.detail?.rows.find((r) => r.id === app.selected) ?? null);
  const run = $derived(v?.runs.find((r) => r.run_id === app.selected) ?? null);
  const reveal = (target: Parameters<typeof api.reveal>[0]) => api.reveal(target).catch((e) => app.notify(message(e)));
  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      app.notify(`已拷贝${what}`);
    } catch {
      app.notify("无法访问剪贴板");
    }
  }
  // LLM reasons are model output; show them apart from facts.
  const reason = (line: string) => line.startsWith("理由 ") || line.startsWith("待审核：");
</script>

{#if !v}
  <div class="empty">正在载入</div>
{:else if app.page === "structure" && node}
  <h4>{node.name}{node.dir ? "/" : ""}</h4>
  <dl class="kv">
    <dt>类型</dt><dd>{node.dir ? "目录" : "文件"}</dd>
    <dt>路径</dt><dd class="mono selectable">{node.path}</dd>
    <dt>ID</dt><dd class="mono selectable">{node.id}</dd>
    {#if node.description}<dt>说明</dt><dd>{node.description}</dd>{/if}
    {#if node.dir}<dt>子项</dt><dd>{node.child_dirs} 个目录，{node.child_files} 个文件</dd>{/if}
    <dt>LLM</dt><dd>{node.excluded ?? "可发送名称与路径"}</dd>
  </dl>
  <div class="row">
    <button class="btn sm" onclick={() => reveal({ kind: "node", id: node.id })}>在 Finder 中显示</button>
    <button class="btn sm" onclick={() => copy(node.id, " ID")}>拷贝 ID</button>
    <button class="btn sm" onclick={() => copy(node.path, "相对路径")}>拷贝相对路径</button>
  </div>
{:else if app.page === "organize" && app.segment === "review" && row}
  <h4>{row.name}{row.dir ? "/" : ""}</h4>
  {#if row.detail.length}
    <dl class="kv">
      {#each row.detail.filter((l) => !reason(l)) as line, i (i)}<dt>{i === 0 ? "路径" : ""}</dt><dd class="mono selectable">{line}</dd>{/each}
    </dl>
    {#each row.detail.filter(reason) as line, i (i)}
      <div class="quote" style="margin-bottom:8px">{line}<small>模型输出，仅供参考</small></div>
    {/each}
  {:else}
    <dl class="kv"><dt>路径</dt><dd class="mono selectable">{row.path}</dd><dt>ID</dt><dd class="mono selectable">{row.id}</dd></dl>
    <div class="tiny muted">与基线相同，没有 LLM 分配记录。</div>
  {/if}
{:else if app.page === "history" && run}
  <h4 class="mono">{run.run_id}</h4>
  <dl class="kv"><dt>计划</dt><dd class="mono">{run.plan_id}</dd><dt>状态</dt><dd>{run.status}</dd><dt>步骤</dt><dd>{run.completed}/{run.total}</dd></dl>
  {#if run.error}<div class="err">{run.error}</div>{/if}
{:else if app.page === "recovery" && v.recoveries.length}
  <h4>如何核对</h4>
  <ol class="tiny" style="padding-left:16px;margin:0;line-height:1.7">
    <li>在步骤表中用“在 Finder 中显示”查看源和目标。</li>
    <li>把文件放到你想要的位置（可以手工移回）。</li>
    <li>点击“标记已人工处理”。</li>
    <li>归档这一轮，再同步，记录磁盘的实际状态。</li>
  </ol>
{:else if app.page === "overview"}
  <h4>{v.name}</h4>
  <dl class="kv">
    <dt>根目录</dt><dd class="mono selectable">{v.root}</dd>
    <dt>状态</dt><dd>{v.state.label}</dd>
    {#if v.overview}<dt>节点</dt><dd>{v.overview.node_count}</dd>{/if}
    {#if v.overview?.git_head}<dt>Git</dt><dd class="mono">{v.overview.git_head}</dd>{/if}
  </dl>
  <button class="btn sm" onclick={() => reveal({ kind: "root" })}>在 Finder 中显示</button>
{:else}
  <div class="empty">选择一项以查看详情</div>
{/if}
