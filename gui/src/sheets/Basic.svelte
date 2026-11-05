<script lang="ts">
  // Ordinary confirmations (design §8.1): writes inside `.organizer` and new
  // files. Each shows what it writes and the equivalent command.
  import Archive from "./Archive.svelte";
  import Cmd from "./Cmd.svelte";
  import Icon from "../components/Icon.svelte";
  import Sheet from "../components/Sheet.svelte";
  import { app } from "../stores/app.svelte";

  const s = $derived(app.sheet);
  const v = $derived(app.view!);
  const o = $derived(v.overview);
  let ack = $state(false);
  let name = $state(app.sheet?.kind === "copy" ? app.sheet.name : "new.tree");
  let invalid = $state<string | null>(null);
  const close = () => (app.sheet = null);
  const onerror = (e: string | null) => (invalid = e);
</script>

{#if s?.kind === "init"}
  <Sheet title="初始化这个文件夹" oncancel={close} note="Return 键 = 取消">
    <dl class="kv"><dt>根目录</dt><dd class="mono">{v.root}</dd><dt>将创建</dt><dd class="mono">.organizer/（structure.tree、state/index.sqlite、.git）</dd></dl>
    <p class="muted" style="margin:0">只读取文件名和元数据，不移动、不修改任何文件。</p>
    <Cmd action={{ kind: "init" }} {onerror} />
    {#snippet actions()}<button class="btn pri" disabled={!!invalid || !!app.busy} onclick={() => app.run({ kind: "init" }, "正在初始化")}>初始化</button>{/snippet}
  </Sheet>
{:else if s?.kind === "sync" && o}
  <Sheet title="同步磁盘变化" oncancel={close} note="Return 键 = 取消">
    <p class="muted" style="margin:0 0 8px">把以下在 TreeSpec Organizer 之外发生的变化记录到基线：</p>
    <dl class="kv"><dt>新增</dt><dd>{o.added.length} 项</dd><dt>移动</dt><dd>{o.moved.length} 项（保留原 ID）</dd><dt>仅内容变化</dt><dd>{o.changed.length} 项（只更新扫描缓存）</dd><dt>缺失</dt><dd>{o.missing.length} 项</dd><dt>未知身份</dt><dd>{o.unknown.length} 项</dd></dl>
    <div class="notice info"><Icon name="info" /><span>缺失只从结构快照移除，不删除磁盘文件，也无法恢复其内容。</span></div>
    {#if o.unknown.length}
      <div class="list" style="margin:8px 0">{#each o.unknown as u, i (i)}<div class="li"><span class="grow mono">{u.path}</span><span class="muted tiny">{u.note}</span></div>{/each}</div>
      <label class="ck"><input type="checkbox" bind:checked={ack} /><span>将这些记录为旧节点消失 + 新节点出现（<span class="mono">--accept-unknown</span>）</span></label>
    {/if}
    <Cmd action={{ kind: "sync", accept_unknown: ack }} {onerror} />
    {#snippet actions()}<button class="btn pri" disabled={(o.unknown.length > 0 && !ack) || !!invalid || !!app.busy} onclick={() => app.run({ kind: "sync", accept_unknown: ack }, "正在同步")}>同步</button>{/snippet}
  </Sheet>
{:else if s?.kind === "copy"}
  <Sheet title="复制当前结构为草案" oncancel={close}>
    <div class="fld"><span>文件名</span><span><input class="inp mono" aria-label="文件名" bind:value={name} spellcheck="false" autocomplete="off" /><span class="hint">在 drafts/ 下只新建，不覆盖。</span></span></div>
    <Cmd action={{ kind: "copy_baseline", name }} {onerror} />
    {#snippet actions()}<button class="btn pri" disabled={!!invalid || !!app.busy} onclick={() => app.run({ kind: "copy_baseline", name }, "正在复制")}>创建并编辑</button>{/snippet}
  </Sheet>
{:else if s?.kind === "export"}
  <Sheet title="导出脚本" oncancel={close}>
    <div class="fld"><span>文件名</span><span class="mono">plans/{s.plan.replace(/\.json$/, ".sh")}</span></div>
    <p class="muted tiny" style="margin:0">脚本只调用不带 --yes 的 organizer apply，与 CLI 相同。</p>
    <Cmd action={{ kind: "export", plan: s.plan }} {onerror} />
    {#snippet actions()}<button class="btn pri" disabled={!!invalid || !!app.busy} onclick={() => app.run({ kind: "export", plan: s.plan })}>导出</button>{/snippet}
  </Sheet>
{:else if s?.kind === "finish_git"}
  <Sheet title="补记 Git" oncancel={close} note="Return 键 = 取消">
    <p style="margin:0 0 8px">只补齐 <span class="mono">{s.run}</span> 的结构提交与确认，绝不重放文件移动。库会先核对目标、身份和 Git 前置条件。</p>
    <Cmd action={{ kind: "finish_git", run: s.run }} {onerror} />
    {#snippet actions()}<button class="btn pri" disabled={!!invalid || !!app.busy} onclick={() => app.run({ kind: "finish_git", run: s.run })}>补记 Git</button>{/snippet}
  </Sheet>
{:else if s?.kind === "resolve"}
  <Sheet title="标记 {s.run} 已人工处理" oncancel={close} note="Return 键 = 取消">
    <p style="margin:0 0 8px">只修改操作日志，不移动文件，不修改基线、索引或 Git。这份计划之后永远不能再执行。</p>
    <label class="ck"><input type="checkbox" bind:checked={ack} /><span>我已人工核对每个步骤的源和目标</span></label>
    <p class="muted tiny">之后先归档这一轮，再同步，记录磁盘的实际状态。</p>
    <Cmd action={{ kind: "mark_resolved", run: s.run }} {onerror} />
    {#snippet actions()}<button class="btn pri" disabled={!ack || !!invalid || !!app.busy} onclick={() => app.run({ kind: "mark_resolved", run: s.run })}>标记已处理</button>{/snippet}
  </Sheet>
{:else if s?.kind === "archive" || s?.kind === "new"}
  <Archive />
{/if}
