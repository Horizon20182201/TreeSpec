<script lang="ts">
  // 设置 (design §6.8). The API key goes to the Keychain and is never shown.
  // Restrictions set in the environment are locked on.
  import { onMount } from "svelte";
  import Sheet from "../components/Sheet.svelte";
  import { api, message } from "../ipc/api";
  import type { Preferences } from "../ipc/bindings/Preferences";
  import type { SettingsView } from "../ipc/bindings/SettingsView";
  import { app } from "../stores/app.svelte";

  let view = $state<SettingsView | null>(null);
  let prefs = $state<Preferences | null>(null);
  let key = $state<string | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    view = await api.settings();
    prefs = structuredClone($state.snapshot(view.preferences));
  });
  const locked = (name: string) => !!view?.env_locked.includes(`ORGANIZER_LLM_${name}`);

  async function attempt(f: () => Promise<unknown>) {
    try {
      error = null;
      await f();
      return true;
    } catch (e) {
      error = message(e);
      return false;
    }
  }
  async function saveKey(value: string | null) {
    if (await attempt(() => api.setApiKey(value))) {
      key = null;
      view = await api.settings();
      app.notify(value ? "API Key 已保存到钥匙串" : "已从钥匙串移除 API Key");
    }
  }
  async function pickEditor() {
    if (await attempt(() => api.system({ kind: "pick_editor" }))) {
      view = await api.settings();
      if (prefs) prefs.editor_app = view.preferences.editor_app;
    }
  }
  async function done() {
    if (prefs && (await attempt(() => api.setSettings($state.snapshot(prefs!) as Preferences)))) app.sheet = null;
  }
</script>

<Sheet title="设置" oncancel={() => (app.sheet = null)}>
  {#if view && prefs}
    {#if error}<div class="notice bad"><span>{error}</span></div>{/if}
    <h3 style="margin-top:6px">LLM</h3>
    <div class="fld"><span>服务地址</span><input class="inp mono" aria-label="服务地址" placeholder="https://api.openai.com/v1" bind:value={prefs.llm.base_url} spellcheck="false" /></div>
    <div class="fld"><span>模型</span><input class="inp mono" aria-label="模型" bind:value={prefs.llm.model} spellcheck="false" /></div>
    <div class="fld"><span>API Key</span>
      {#if key !== null}
        <span class="row"><input class="inp mono" type="password" aria-label="API Key" bind:value={key} autocomplete="off" style="flex:1" />
          <button class="btn sm pri" onclick={() => saveKey(key)}>存入钥匙串</button><button class="btn sm" onclick={() => (key = null)}>取消</button></span>
      {:else}
        <span class="row"><span class="badge {view.key_saved ? 'ok' : ''}">{view.key_saved ? "已保存在钥匙串" : "未设置"}</span>
          <button class="btn sm" onclick={() => (key = "")}>{view.key_saved ? "替换…" : "设置…"}</button>
          {#if view.key_saved}<button class="btn sm" onclick={() => saveKey(null)}>移除</button>{/if}</span>
      {/if}
    </div>
    {#each [["LOCAL_ONLY", "local_only", "只允许本地服务（loopback）"], ["NO_CONTENT", "no_content", "禁止发送文本片段"], ["DISABLED", "disabled", "禁用 LLM（只用手工草案）"]] as const as [env, field, label] (env)}
      <label class="ck"><input type="checkbox" checked={prefs.llm[field] || locked(env)} disabled={locked(env)} onchange={(e) => prefs && (prefs.llm[field] = e.currentTarget.checked)} />
        <span>{label}{#if locked(env)} <span class="badge">由环境变量 ORGANIZER_LLM_{env}=1 设置</span>{/if}</span></label>
    {/each}
    {#if view.env_ignored.length}<p class="muted tiny">进程环境中有 {view.env_ignored.join("、")}；GUI 使用这里的设置。</p>{/if}
    <p class="muted tiny">不提供“测试连接”：它会在你确认发送内容之前联网。配置在打开“新建 LLM 草案”时离线校验。</p>
    <h3>通用</h3>
    <div class="fld"><span>外部编辑器</span><span class="row"><span class="muted mono tiny">{prefs.editor_app ?? "未设置"}</span><button class="btn sm" onclick={pickEditor}>选择应用…</button>
      {#if prefs.editor_app}<button class="btn sm" onclick={() => prefs && (prefs.editor_app = null)}>清除</button>{/if}</span></div>
    <label class="ck"><input type="checkbox" bind:checked={prefs.scan_on_activate} /><span>窗口激活时自动重新扫描（默认关闭；扫描大目录可能较慢）</span></label>
    <label class="ck"><input type="checkbox" bind:checked={prefs.reopen_last} /><span>启动时重新打开上次的工作区</span></label>
    <h3>高级</h3>
    <div class="fld"><span>Git</span><span>{#if view.tools}<span class="badge ok">/usr/bin/git · 命令行工具已安装</span>{:else}<span class="badge bad">未安装命令行工具</span> <button class="btn sm" onclick={() => attempt(() => api.system({ kind: "install_tools" }))}>安装…</button>{/if}</span></div>
    <div class="fld"><span>诊断</span><span><button class="btn sm" onclick={() => navigator.clipboard.writeText(view!.diagnostics).then(() => app.notify("已拷贝诊断信息（不含 API Key 和文件路径）"))}>拷贝诊断信息</button></span></div>
  {:else}
    <div class="empty">正在载入…</div>
  {/if}
  {#snippet actions()}<button class="btn pri" disabled={!prefs} onclick={done}>完成</button>{/snippet}
</Sheet>
