<script lang="ts">
  // Workspace window layout (design §4.2): sidebar, toolbar, next-step banner,
  // page, inspector, activity log and sheets.
  import { onMount } from "svelte";
  import Icon from "../components/Icon.svelte";
  import Inspector from "../components/Inspector.svelte";
  import Sheets from "../sheets/Sheets.svelte";
  import History from "./History.svelte";
  import Organize from "./organize/Organize.svelte";
  import Overview from "./Overview.svelte";
  import Recovery from "./Recovery.svelte";
  import Structure from "./Structure.svelte";
  import { api, events, message } from "../ipc/api";
  import { isMock } from "../lib/format";
  import { goto, intent, menu, newDraft, refresh } from "../stores/actions";
  import { app, hhmm, type Page } from "../stores/app.svelte";

  let { inspector }: { inspector: boolean } = $props();
  let now = $state(Date.now());
  const PAGES: [Page, string][] = [["overview", "概览"], ["structure", "结构"], ["organize", "整理"], ["history", "历史"], ["recovery", "恢复"]];
  const v = $derived(app.view);
  const count = $derived(v ? v.rounds.length + v.unpaired.length + v.others.length : 0);
  const stale = $derived(!!v && now - v.scanned_ms > 60_000);

  async function externalCheck() {
    const e = app.editor;
    if (!e) return;
    try {
      const d = await api.draftText(e.round);
      if (d.digest === e.digest) return;
      if (e.text === e.saved) Object.assign(e, { text: d.text, saved: d.text, digest: d.digest, external: false });
      else e.external = true;
    } catch {
      /* the round may be gone; the next snapshot says so */
    }
  }

  onMount(() => {
    app.inspector = inspector;
    const offs: (() => void)[] = [];
    void (async () => {
      offs.push(await events.gui((e) => app.handle(e)));
      offs.push(await events.menu((id) => menu(id)));
      offs.push(await events.guard((guard) => (app.sheet = { kind: "guard", guard })));
      offs.push(await events.activated(() => void externalCheck()));
      try {
        const view = await api.snapshot();
        if (view) app.setView(view);
      } catch (e) {
        app.notify(message(e));
      }
    })();
    const timer = setInterval(() => (now = Date.now()), 15_000);
    return () => {
      offs.forEach((off) => off());
      clearInterval(timer);
    };
  });

  $effect(() => {
    void api.setGuard(app.dirty).catch(() => {});
  });

  // In Tauri the menu bar owns these shortcuts; the browser preview has none.
  const KEYS: Record<string, string> = { "1": "page-overview", "2": "page-structure", "3": "page-organize", "4": "page-history", "5": "page-recovery", s: "save", r: "refresh", n: "new-chat", f: "find", ",": "settings" };
  function onkeydown(e: KeyboardEvent) {
    if (!isMock || !e.metaKey || app.sheet || !KEYS[e.key]) return;
    e.preventDefault();
    menu(KEYS[e.key]);
  }
</script>

<svelte:window {onkeydown} />

<div class="win">
  <nav class="side" aria-label="工作区">
    <div class="lights" data-tauri-drag-region>{#if isMock}<span class="fake-lights" aria-hidden="true"><i></i><i></i><i></i></span>{/if}</div>
    <h6>工作区</h6>
    {#each PAGES as [page, label], i (page)}
      {@const alarm = page === "recovery" && !!v?.recoveries.length}
      {@const off = (page === "recovery" && !alarm) || (v?.state.code === "uninit" && page !== "overview")}
      <button class="nav" class:off class:alarm aria-current={app.page === page ? "page" : undefined} aria-disabled={off} title="⌘{i + 1}" onclick={() => goto(page)}>
        <Icon name={page} />{label}
        {#if page === "organize" && count}<span class="cnt">{count}</span>{/if}
        {#if alarm}<span class="cnt">{v?.recoveries.length}</span>{/if}
      </button>
    {/each}
    <button class="llm" title="打开设置（⌘,）" onclick={() => (app.sheet = { kind: "settings" })}>LLM<b>{v?.llm.label ?? "…"}</b>设置…</button>
  </nav>

  <header class="bar" data-tauri-drag-region>
    <div class="title"><b>{v?.name ?? "TreeSpec Organizer"}</b><span>{v?.root ?? ""}</span></div>
    {#if v}
      <button class="pill {v.state.tone}" title="工作区状态" onclick={() => goto("overview")}>
        <i></i>{v.state.label}<small class:stale>{stale ? "可能已过期 · " : "扫描 "}{hhmm(v.scanned_ms)}</small>
      </button>
    {/if}
    {#if app.busy}<span class="muted tiny" role="status">{app.busy}…</span>{/if}
    <span class="sp"></span>
    <button class="tb" class:hot={stale} title="重新扫描（⌘R）" aria-label="重新扫描" disabled={!!app.busy || v?.state.code === "uninit"} onclick={refresh}><Icon name="refresh" /></button>
    <button class="tb" title={v?.gates.draft.ok ? "新建草案（⌘N）" : (v?.gates.draft.reason ?? "")} aria-label="新建草案" disabled={!v?.gates.draft.ok} onclick={() => newDraft("menu")}><Icon name="plus" /></button>
    <button class="tb" aria-pressed={app.showLog} title="活动日志（⇧⌘L）" aria-label="活动日志" onclick={() => (app.showLog = !app.showLog)}><Icon name="log" /></button>
    <button class="tb" aria-pressed={app.inspector} title="检查器（⌥⌘0）" aria-label="检查器" onclick={() => menu("toggle-inspector")}><Icon name="insp" /></button>
  </header>

  <div class="body">
    <div class="main">
      {#if v}
        <div class="next {v.next.tone}" role="status">
          <b>下一步</b><span>{v.next.text}</span>
          {#if v.next.button}<button class="btn sm pri" disabled={!!app.busy} onclick={() => intent(v.next.intent, v.next.round)}>{v.next.button}</button>{/if}
        </div>
        {#if app.page === "organize"}
          <Organize />
        {:else}
          <div class="page">
            {#if app.page === "overview"}<Overview />{:else if app.page === "structure"}<Structure />{:else if app.page === "history"}<History />{:else}<Recovery />{/if}
          </div>
        {/if}
      {:else}
        <div class="empty" style="padding-top:140px">正在扫描…</div>
      {/if}
      {#if app.showLog}
        <section class="log" aria-label="活动日志">
          <div class="hd"><span style="flex:1">活动日志（本次会话，只在内存中）</span></div>
          {#each app.log as e, i (i)}
            <div class="en"><span class="muted">{e.time}</span><span>{e.label}</span><code>{e.command}</code><span class="badge {e.kind === 'ok' ? 'ok' : e.kind === 'failed' ? 'bad' : 'rev'}">{e.kind === "ok" ? "成功" : e.kind === "failed" ? "失败" : "有待审核项"}</span></div>
          {:else}
            <div class="empty">还没有动作</div>
          {/each}
        </section>
      {/if}
    </div>
    {#if app.inspector}<aside class="insp" aria-label="检查器"><Inspector /></aside>{/if}
  </div>
  <Sheets />
  {#if app.toast}<div class="toast" role="status">{app.toast}</div>{/if}
</div>
