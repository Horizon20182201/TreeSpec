<script lang="ts">
  // 概览 (design §6.1–6.2): initialization, the state card, disk changes,
  // rounds and scan boundaries; special cards for the blocking states.
  import Icon from "../components/Icon.svelte";
  import { api, message } from "../ipc/api";
  import type { ChangeView } from "../ipc/bindings/ChangeView";
  import { stageTone } from "../lib/format";
  import { archiveAll, newDraft, refresh } from "../stores/actions";
  import { app, hhmm } from "../stores/app.svelte";

  const v = $derived(app.view!);
  const o = $derived(v.overview);
  const TILES = [["added", "新增"], ["missing", "缺失"], ["moved", "移动"], ["changed", "仅内容变化"], ["unknown", "未知身份"]] as const;
  let tab = $state<(typeof TILES)[number][0] | null>(null);
  const rows = $derived<ChangeView[]>(o && tab ? o[tab] : []);
  const code = $derived(v.state.code);
  const total = $derived(o ? TILES.reduce((n, [k]) => n + o[k].length, 0) : 0);
  const reveal = (target: Parameters<typeof api.reveal>[0]) => api.reveal(target).catch((e) => app.notify(message(e)));
</script>

{#if code === "uninit"}
  <div class="card" style="max-width:660px;margin:10px auto">
    <div class="hero"><div class="big"><Icon name="folder" /></div><div><h2>这个文件夹还没有初始化</h2><div class="mono muted selectable">{v.root}</div></div></div>
    <h3>初始化会做什么</h3>
    <ul style="margin:0;padding-left:18px;line-height:1.8">
      <li>扫描目录，只读取文件名和元数据，不读取文件内容。</li>
      <li>创建 <code>.organizer/</code>：结构文本、SQLite 索引、只记录结构的 Git 仓库。</li>
      <li>不移动、不修改任何已有文件。</li>
    </ul>
    {#if v.init_warning}<div class="notice"><Icon name="warn" /><span>{v.init_warning}</span></div>{/if}
    <div class="notice info"><Icon name="info" /><span>如果这个文件夹在桌面、文稿、下载、iCloud 云盘或外置卷上，macOS 可能询问是否允许访问。请选择“允许”；拒绝后扫描会不完整。</span></div>
    <div class="row" style="justify-content:flex-end"><button class="btn pri" onclick={() => (app.sheet = { kind: "init" })}>初始化…</button></div>
  </div>
{:else if code === "failed"}
  <div class="card hero bad"><div class="big"><Icon name="warn" /></div><div style="flex:1"><h2>无法读取状态</h2><div class="muted">界面保持只读。修正问题后重新扫描。</div></div><button class="btn pri" onclick={refresh}>重新扫描</button></div>
  <h3>错误</h3><div class="err">{v.error?.message}</div>{#if v.error?.hint}<div class="hint">{v.error.hint}</div>{/if}
{:else if code === "interrupted"}
  <div class="card hero bad"><div class="big"><Icon name="warn" /></div><div style="flex:1"><h2>元数据更新曾中断</h2><div class="muted">先备份 .organizer，再人工检查下面的标记和日志。GUI 不会自动修复。</div></div><button class="btn" onclick={() => reveal({ kind: "meta" })}>在 Finder 中显示 .organizer</button></div>
  {#if v.interrupted?.recovery_files.length}<h3>SQLite 恢复文件</h3><div class="err">{v.interrupted.recovery_files.join("\n")}</div>{/if}
  {#if v.interrupted?.pending_marker}<h3>state/pending.json</h3><div class="err">{v.interrupted.pending_marker}</div>{/if}
  <h3>sync.log 末尾</h3><div class="err">{v.interrupted?.sync_log_tail || "（空）"}</div>
{:else if code === "unfinished"}
  <div class="card hero bad"><div class="big"><Icon name="warn" /></div><div style="flex:1"><h2>迁移未完成</h2><div class="muted">检查完成之前，同步、草案和执行都不可用。</div></div><button class="btn pri" onclick={() => (app.page = "recovery")}>前往恢复</button></div>
{:else if o}
  {#if code === "conflict"}
    <div class="card hero bad"><div class="big"><Icon name="warn" /></div><div style="flex:1"><h2>基线冲突</h2><div class="muted">structure.tree、索引或 Git 不一致。可以另存当前 structure.tree 为草案，作为人工恢复的第一步。</div></div><button class="btn pri" disabled={!v.gates.copy_baseline.ok} onclick={() => newDraft("copy")}>另存为草案…</button></div>
  {:else if code === "scan_incomplete"}
    <div class="card hero warn"><div class="big"><Icon name="warn" /></div><div style="flex:1"><h2>扫描不完整</h2><div class="muted">有路径无法读取。权限不足时，重新在“打开”面板中选择这个文件夹，或在系统设置中允许访问。</div></div>
      <button class="btn" onclick={() => api.system({ kind: "reauthorize" }).catch((e) => app.notify(message(e)))}>重新授权…</button>
      <button class="btn" onclick={() => api.system({ kind: "files_and_folders" }).catch((e) => app.notify(message(e)))}>打开系统设置</button></div>
  {:else if code === "changed" || code === "git_pending"}
    <div class="card hero warn"><div class="big"><Icon name="refresh" /></div><div style="flex:1"><h2>{code === "changed" ? `磁盘有 ${total} 项变化，待同步` : "Git 待补记"}</h2>
      <div class="muted">{v.rounds.length + v.unpaired.length + v.others.length ? "有进行中的整理轮次，需要先归档。" : code === "changed" ? "这些变化发生在 TreeSpec Organizer 之外。同步后才能新建草案。" : "结构和索引已保存，只差 Git 记录。同步以重试。"}</div></div>
      {#if !v.gates.sync.ok && v.gates.archive.ok}<button class="btn pri" onclick={archiveAll}>全部归档…</button>{/if}
      <button class="btn" class:pri={v.gates.sync.ok} disabled={!v.gates.sync.ok} title={v.gates.sync.reason ?? ""} onclick={() => (app.sheet = { kind: "sync" })}>同步…</button></div>
  {:else}
    <div class="card hero ok"><div class="big"><Icon name="check" /></div><div style="flex:1"><h2>已同步</h2><div class="muted">磁盘与基线一致。上次扫描 {hhmm(v.scanned_ms)}，共 {o.node_count} 个节点。</div></div>
      <button class="btn" disabled={!v.gates.copy_baseline.ok} onclick={() => newDraft("copy")}>复制当前结构为草案…</button>
      <button class="btn pri" disabled={!v.gates.draft.ok || !v.llm.available} title={v.llm.reason ?? ""} onclick={() => newDraft("chat")}>新建 LLM 草案…</button></div>
  {/if}
  <div class="cols">
    <section>
      <h3>磁盘变化（相对基线）</h3>
      <div class="tiles">
        {#each TILES as [k, label] (k)}
          <button class="tile" class:zero={!o[k].length} aria-pressed={tab === k} onclick={() => (tab = tab === k ? null : k)}><b>{o[k].length}</b><span>{label}</span></button>
        {/each}
      </div>
      {#if tab}
        {#if rows.length}
          <div class="list" style="margin-top:10px">
            {#each rows as c, i (i)}
              <div class="li"><Icon name="file" class="ic" /><span class="grow mono selectable" title={c.path}>{c.old_path ? `${c.old_path} → ${c.path}` : c.path}</span>{#if c.note}<span class="muted tiny">{c.note}</span>{/if}</div>
            {/each}
          </div>
        {:else}<div class="empty">没有这一类变化</div>{/if}
      {:else}<p class="hint">点击某一类查看路径。</p>{/if}
      {#if o.errors.length}<h3>扫描出错</h3><div class="list">{#each o.errors as e, i (i)}<div class="li"><span class="badge bad"><b>!</b></span><span class="grow mono">{e.path}</span><span class="muted tiny">{e.note}</span></div>{/each}</div>{/if}
      <h3>冲突</h3>
      {#if o.conflicts.length}<div class="err">{o.conflicts.join("\n")}</div>{:else}<div class="muted">无</div>{/if}
    </section>
    <section>
      <h3>进行中的整理轮次</h3>
      {#if v.rounds.length || v.unpaired.length || v.others.length}
        <div class="list">
          {#each v.rounds as r (r.name)}<button class="li" onclick={() => app.selectRound(r.name)}><span class="grow">{r.name}</span><span class="badge {r.blocked ? 'warn' : stageTone(r.stage_code)}">{r.blocked ?? r.stage}</span></button>{/each}
          {#each v.unpaired as p (p.name)}<button class="li" onclick={() => (app.page = "organize")}><span class="grow">{p.name} <span class="muted tiny">未关联计划</span></span><span class="badge warn">{p.stage}</span></button>{/each}
          {#each v.others as x (x.selector)}<div class="li"><span class="grow mono">{x.selector}</span><span class="muted tiny">{x.note}</span></div>{/each}
        </div>
      {:else}<div class="empty">没有进行中的整理轮次</div>{/if}
      <h3>扫描边界</h3>
      {#if o.boundaries.length}
        <div class="list">{#each o.boundaries as b, i (i)}<div class="li"><span class="badge"><b>{b.tag}</b></span><span class="grow mono">{b.path}</span><span class="muted tiny">{b.note}</span></div>{/each}</div>
      {:else}<div class="muted">无</div>{/if}
    </section>
  </div>
{/if}
