// What the next-step banner, buttons and menu items open. Buttons only open
// sheets; the sheets run the action after confirmation (design §4.2, §8.1).

import { api, message } from "../ipc/api";
import { app, type Page } from "./app.svelte";

function gateOr(ok: boolean, reason: string | null | undefined) {
  if (!ok) app.notify(reason ?? "现在不可用");
  return ok;
}

/** Leaves the editor only after asking about unsaved changes. */
export function guarded(then: () => void) {
  if (app.dirty) app.sheet = { kind: "unsaved", then };
  else then();
}

export function goto(page: Page) {
  const v = app.view;
  if (v?.state.code === "uninit" && page !== "overview") return app.notify("先初始化这个文件夹");
  if (page === "recovery" && !v?.recoveries.length) return app.notify("没有未完成的迁移");
  guarded(() => (app.page = page));
}

export function newDraft(kind: "chat" | "copy" | "menu") {
  const v = app.view;
  if (!v) return;
  if (kind === "copy") {
    if (!gateOr(v.gates.copy_baseline.ok, v.gates.copy_baseline.reason)) return;
    const used = new Set(v.rounds.map((r) => r.name));
    let name = "new.tree";
    for (let i = 2; used.has(name); i++) name = `new-${i}.tree`;
    app.sheet = { kind: "copy", name };
    return;
  }
  if (!gateOr(v.gates.draft.ok, v.gates.draft.reason)) return;
  if (kind === "chat" && !v.llm.available) return app.notify(v.llm.reason ?? "LLM 不可用");
  app.sheet = kind === "chat" ? { kind: "chat" } : { kind: "new" };
}

export function plan(round: string) {
  const v = app.view;
  if (!v || !gateOr(v.gates.plan.ok, v.gates.plan.reason)) return;
  if (app.editor?.round === round && app.dirty) return app.notify("先保存编辑器中的修改（⌘S）：计划总是基于磁盘上的文件");
  app.sheet = { kind: "plan", round };
}

function activePlan(round: string | null) {
  return app.view?.rounds.find((r) => r.name === round)?.active ?? null;
}

export function dryRun(round: string | null) {
  const p = activePlan(round);
  if (p) void app.run({ kind: "dry_run", plan: p }, "正在预览");
}

/** Preview first, then the strong confirmation (design §7.5). */
export function apply(round: string | null) {
  const p = activePlan(round);
  if (!p) return app.notify("这一轮没有可执行的计划");
  app.sheet = { kind: "apply_run", plan: p };
  void app.run({ kind: "dry_run", plan: p }, "正在预览");
}

export function archive(round: string | null) {
  const r = app.view?.rounds.find((x) => x.name === round);
  if (r) app.sheet = { kind: "archive", round: r.name, selectors: r.selectors };
}

export function archiveAll() {
  const v = app.view;
  if (!v || !gateOr(v.gates.archive.ok, v.gates.archive.reason)) return;
  const selectors = [...v.rounds.flatMap((r) => r.selectors), ...v.unpaired.map((p) => `plans/${p.name}`), ...v.others.map((o) => o.selector)];
  app.sheet = { kind: "archive", round: null, selectors };
}

export async function refresh() {
  try {
    await api.refresh();
    app.busy ??= "正在扫描";
  } catch (e) {
    app.notify(message(e));
  }
}

export async function saveEditor() {
  const e = app.editor;
  if (!e || e.text === e.saved) return;
  try {
    const r = await api.saveDraft(e.round, e.digest, e.text);
    app.record(r);
    if (r.kind === "failed") return (app.sheet = { kind: "result", result: r });
    if (r.outcome?.kind === "saved") {
      e.saved = e.text;
      e.digest = r.outcome.digest;
      e.external = false;
      void api.setGuard(false);
    }
    app.notify(r.message);
  } catch (err) {
    app.notify(message(err));
  }
}

/** The banner's button (`NextView.intent`). */
export function intent(name: string | null, round: string | null) {
  const v = app.view;
  switch (name) {
    case "init": return (app.sheet = { kind: "init" });
    case "reveal_meta": return void api.reveal({ kind: "meta" }).catch((e) => app.notify(message(e)));
    case "recovery": return goto("recovery");
    case "save_baseline": return newDraft("copy");
    case "refresh": return void refresh();
    case "sync": return v && gateOr(v.gates.sync.ok, v.gates.sync.reason) && (app.sheet = { kind: "sync" });
    case "archive_all": return archiveAll();
    case "apply": return round && (app.selectRound(round, "plan"), apply(round));
    case "plan": return round && (app.selectRound(round, "review"), plan(round));
    case "edit": return round && app.selectRound(round, "edit");
    case "archive": return archive(round);
    case "new_draft": return newDraft("menu");
  }
}

const PAGES: Record<string, Page> = {
  "page-overview": "overview", "page-structure": "structure", "page-organize": "organize",
  "page-history": "history", "page-recovery": "recovery",
};

/** Menu bar items forwarded by the Tauri shell (design §4.3). */
export function menu(id: string) {
  if (PAGES[id]) return goto(PAGES[id]);
  const r = app.round;
  const run = app.view?.recoveries[0]?.run_id;
  switch (id) {
    case "settings": return (app.sheet = { kind: "settings" });
    case "find": return document.querySelector<HTMLInputElement>("[data-find]")?.focus();
    case "refresh": case "recheck": return void refresh();
    case "toggle-inspector":
      app.inspector = !app.inspector;
      return void api.system({ kind: "inspector", open: app.inspector }).catch(() => {});
    case "toggle-log": return (app.showLog = !app.showLog);
    case "toggle-changes": return (app.onlyChanges = !app.onlyChanges);
    case "reveal-root": return void api.reveal({ kind: "root" }).catch((e) => app.notify(message(e)));
    case "new-chat": return newDraft("chat");
    case "new-copy": return newDraft("copy");
    case "save": return void saveEditor();
    case "plan": return r && plan(r);
    case "dry-run": return dryRun(r);
    case "apply": return apply(r);
    case "export": { const p = activePlan(r); return p && (app.sheet = { kind: "export", plan: p }); }
    case "archive": return archive(r);
    case "archive-all": return archiveAll();
    case "finish-git": return run && (app.sheet = { kind: "finish_git", run });
    case "mark-resolved": return run && (app.sheet = { kind: "resolve", run });
  }
}
