<script lang="ts">
  // 结构 (design §6.3): the read-only baseline outline, search, and the LLM
  // scope (`--scope`) with an offline preview.
  import Icon from "../components/Icon.svelte";
  import { api } from "../ipc/api";
  import type { ScopeView } from "../ipc/bindings/ScopeView";
  import { newDraft } from "../stores/actions";
  import { app } from "../stores/app.svelte";

  const v = $derived(app.view!);
  let closed = $state<Record<string, boolean>>({});
  let query = $state("");
  let preview = $state<ScopeView | null>(null);
  let focusId = $state<string | null>(null);

  const byId = $derived(new Map(v.nodes.map((n) => [n.id, n])));
  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return null;
    const keep = new Set<string>();
    for (const n of v.nodes) {
      if (!n.name.toLowerCase().includes(q)) continue;
      for (let id: string | null = n.id; id && !keep.has(id); id = byId.get(id)?.parent ?? null) keep.add(id);
    }
    return keep;
  });
  const rows = $derived.by(() => {
    const hidden = new Set<string>();
    return v.nodes.filter((n) => {
      if (n.parent && hidden.has(n.parent)) return hidden.add(n.id), false;
      if (matches && !matches.has(n.id)) return false;
      if (!matches && closed[n.id]) hidden.add(n.id);
      return true;
    });
  });

  $effect(() => {
    const ids = [...app.scope];
    const timer = setTimeout(() => {
      api.previewScope(ids).then((p) => (preview = p), () => (preview = null));
    }, 150);
    return () => clearTimeout(timer);
  });

  function toggleScope(id: string, on: boolean) {
    app.scope = on ? [...app.scope, id] : app.scope.filter((x) => x !== id);
  }

  function onkey(e: KeyboardEvent, i: number) {
    const n = rows[i];
    const go = (j: number) => {
      const r = rows[Math.max(0, Math.min(rows.length - 1, j))];
      if (!r) return;
      focusId = r.id;
      app.selected = r.id;
      document.getElementById(`row-${r.id}`)?.focus();
    };
    if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i - 1);
    else if (e.key === "ArrowLeft" && n.dir) closed[n.id] = true;
    else if (e.key === "ArrowRight" && n.dir) closed[n.id] = false;
    else if (e.key === " " && n.dir && !n.excluded) toggleScope(n.id, !app.scope.includes(n.id));
    else return;
    e.preventDefault();
  }

  const names = $derived(app.scope.map((id) => byId.get(id)?.path ?? id));
</script>

<div class="row" style="margin-bottom:10px">
  <h2 style="flex:1">基线结构</h2>
  <span class="muted tiny">只读 · 来自 structure.tree</span>
  <input class="search" type="search" placeholder="按名称过滤（⌘F）" aria-label="按名称过滤" data-find bind:value={query} />
</div>
<div class="tree" role="tree" aria-label="基线结构">
  {#each rows as n, i (n.id)}
    <div id="row-{n.id}" class="tr" role="treeitem" aria-level={n.depth + 1} aria-expanded={n.dir ? !closed[n.id] : undefined}
      aria-selected={app.selected === n.id} tabindex={focusId === n.id || (!focusId && i === 0) ? 0 : -1}
      style="padding-left:{8 + n.depth * 18}px" onclick={() => ((app.selected = n.id), (focusId = n.id))} onkeydown={(e) => onkey(e, i)}>
      <span class="nm">
        {#if n.dir && n.child_dirs + n.child_files}
          <button class="tw" tabindex="-1" aria-label={closed[n.id] ? "展开" : "折叠"} onclick={(e) => { e.stopPropagation(); closed[n.id] = !closed[n.id]; }}>{closed[n.id] && !matches ? "▸" : "▾"}</button>
        {:else}<span class="tw"></span>{/if}
        {#if n.dir}
          {#if n.excluded}<span class="lock" title={n.excluded}><Icon name="lock" class="ic" /></span>
          {:else}<input type="checkbox" tabindex="-1" aria-label="选为 LLM 范围" checked={app.scope.includes(n.id)} onclick={(e) => e.stopPropagation()} onchange={(e) => toggleScope(n.id, e.currentTarget.checked)} />{/if}
        {:else}<span style="width:17px"></span>{/if}
        <Icon name={n.dir ? "folder" : "file"} class="ic" /><span class="n">{n.name}{n.dir ? "/" : ""}</span>
      </span>
      <span class="ds">{n.description}</span>
      <span class="mk">{#if n.excluded}<span class="badge" title={n.excluded}><Icon name="lock" class="ic" />{n.excluded.startsWith("受保护") ? "受保护" : "已排除"}</span>{/if}</span>
    </div>
  {:else}
    <div class="empty">没有匹配的节点</div>
  {/each}
</div>
<div class="scope">
  <span style="flex:1">
    <b>范围</b>：{names.length ? names.join("、") : "整个工作区"}
    {#if preview}
       <span class="muted">{preview.ok ? `将发送 ${preview.visible_nodes} 个节点的名称与路径（含只读祖先），${preview.editable_files} 个可分类文件；${preview.excluded} 个已排除` : preview.error}</span>
      {#if preview.warn_files}<span class="badge warn">分类可能超过 256 个文件</span>{/if}
    {/if}
  </span>
  <button class="btn sm" disabled={!app.scope.length} onclick={() => (app.scope = [])}>清除</button>
  <button class="btn sm pri" disabled={!v.gates.draft.ok || !v.llm.available || preview?.ok === false} title={v.gates.draft.reason ?? v.llm.reason ?? ""} onclick={() => newDraft("chat")}>以所选范围新建 LLM 草案…</button>
</div>
