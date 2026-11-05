<script lang="ts">
  // Welcome window (design §4.1): open a folder, recent workspaces, the
  // command line tools check and the “open the managed parent?” question.
  import Icon from "../components/Icon.svelte";
  import Sheet from "../components/Sheet.svelte";
  import { api, message } from "../ipc/api";
  import type { WindowInfo } from "../ipc/bindings/WindowInfo";
  import { isMock } from "../lib/format";
  import { hhmm } from "../stores/app.svelte";

  let { info, reload }: { info: Extract<WindowInfo, { kind: "welcome" }>; reload: () => void } = $props();
  let toast = $state<string | null>(null);

  async function act(f: () => Promise<unknown>) {
    try {
      await f();
    } catch (e) {
      toast = message(e);
      setTimeout(() => (toast = null), 3000);
    }
    reload();
  }

  const when = (ms: number) =>
    new Date(ms).toDateString() === new Date().toDateString() ? `今天 ${hhmm(ms)}` : new Date(ms).toLocaleDateString("zh-CN");
</script>

<div class="win welcome">
  <div class="wl" data-tauri-drag-region>
    {#if isMock}<span class="fake-lights" aria-hidden="true"><i></i><i></i><i></i></span>{/if}
    <div class="appicon"><Icon name="tree" /></div>
    <h2>TreeSpec Organizer</h2>
    <div class="muted">先看方案，再决定是否移动文件。</div>
    <button class="btn pri" style="margin-top:8px" disabled={!info.tools} onclick={() => act(api.pickFolder)}>打开文件夹… ⌘O</button>
  </div>
  <div class="wr">
    {#if !info.tools}
      <div class="notice">
        <Icon name="warn" />
        <span>
          需要先安装 Xcode 命令行工具：TreeSpec Organizer 用系统的 Git 记录结构历史。安装完成后重新打开文件夹。
          <br /><button class="btn sm" style="margin-top:6px" onclick={() => act(() => api.system({ kind: "install_tools" }))}>安装命令行工具…</button>
        </span>
      </div>
    {/if}
    <h3 style="margin-top:0">最近打开</h3>
    {#if info.recent.length}
      <div class="list">
        {#each info.recent as r (r.path)}
          <button class="li" disabled={!info.tools} onclick={() => act(() => api.openRecent(r.index))}>
            <Icon name="folder" class="ic" />
            <span class="grow"><b>{r.name}</b><br /><span class="muted tiny mono">{r.path}</span></span>
            <span class="tiny muted" style="text-align:right">{r.state}<br />{when(r.opened_ms)}</span>
          </button>
        {/each}
      </div>
    {:else}
      <div class="empty">还没有打开过工作区</div>
    {/if}
    <div class="drop">把文件夹拖到这里，或拖到 Dock 图标上</div>
  </div>
  {#if info.parent}
    <Sheet title="这个文件夹属于一个工作区" oncancel={() => act(() => api.openParent(false))}>
      <p style="margin:0 0 8px"><span class="mono">{info.child}</span> 位于已初始化的工作区 <span class="mono">{info.parent}</span> 内部。</p>
      <p class="muted" style="margin:0">不能在内部再初始化：扫描会跳过任意层级的 .organizer，两个根会同时管理同一批文件。需要时可以用 CLI 完成。</p>
      {#snippet actions()}
        <button class="btn pri" onclick={() => act(() => api.openParent(true))}>打开上级工作区</button>
      {/snippet}
    </Sheet>
  {/if}
  {#if toast}<div class="toast" role="status">{toast}</div>{/if}
</div>
