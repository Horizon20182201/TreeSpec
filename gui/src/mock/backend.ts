// In-memory backend with the same command surface as the Tauri shell, for
// `vite --mode mock` (browser previews) and Playwright. It fakes the state
// machine only; every real check lives in Rust.

import { useTransport, type Transport } from "../ipc/api";
import type { ActionResult } from "../ipc/bindings/ActionResult";
import type { GuiAction } from "../ipc/bindings/GuiAction";
import type { OutcomeView } from "../ipc/bindings/OutcomeView";
import type { ViewModel } from "../ipc/bindings/ViewModel";
import { BASE_TEXT, detail, INBOX_OPS, INBOX_ROWS, INBOX_TEXT, NODES, PAPERS_OPS, PAPERS_ROWS, PAPERS_TEXT, ROOT, STAGES, type MockRound } from "./fixtures";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const q = (s: string) => `'${s.replaceAll("'", "'\\''")}'`;
const OZ = `${ROOT}/.organizer`;

const state = {
  scenario: "clean",
  rounds: [] as MockRound[],
  changes: { added: [] as string[], moved: [] as [string, string][] },
  commits: [{ hash: "3f8e2aa", kind: "init", subject: "init: 11 nodes" }],
  unfinished: false,
  key: true,
  scanned: Date.now(),
  token: "",
};

function rounds(): MockRound[] {
  return [
    { name: "papers.tree", generated: true, stage: "needs_plan", ack: "有 1 个待审核项", plan: null, text: PAPERS_TEXT, rows: PAPERS_ROWS },
    { name: "inbox.tree", generated: false, stage: "needs_plan", ack: null, plan: null, text: INBOX_TEXT, rows: INBOX_ROWS },
  ];
}

function view(): ViewModel {
  const s = state.scenario;
  const code = s === "uninit" ? "uninit" : state.unfinished ? "unfinished" : state.changes.added.length + state.changes.moved.length ? "changed" : "clean";
  const label = { uninit: "未初始化", unfinished: "迁移未完成", changed: "磁盘有变化，待同步", clean: "已同步" }[code]!;
  const pending = state.rounds.length > 0;
  const blocked = code === "changed" ? "受阻：磁盘有变化" : code === "unfinished" ? "受阻：迁移未完成" : null;
  const gate = (ok: boolean, reason: string | null) => ({ ok, reason: ok ? null : reason ?? "现在不可用" });
  const ex = state.rounds.find((r) => r.stage === "executable"), np = state.rounds.find((r) => r.stage === "needs_plan"), dn = state.rounds.find((r) => r.stage === "applied");
  const next = code === "uninit" ? { text: "初始化这个文件夹：扫描目录并创建 .organizer/，不移动任何文件。", tone: "", button: "初始化…", intent: "init", round: null }
    : code === "unfinished" ? { text: "迁移未完成：在恢复页检查，其他页面的写操作已禁用。", tone: "bad", button: "前往恢复", intent: "recovery", round: null }
    : code === "changed" ? { text: `磁盘有 ${state.changes.added.length + state.changes.moved.length} 项变化：${pending ? "先归档进行中的轮次，再同步。" : "同步以记录这些变化，之后才能新建草案。"}`, tone: "warn", button: pending ? "全部归档…" : "同步…", intent: pending ? "archive_all" : "sync", round: null }
    : ex ? { text: `${ex.name} 可执行：预览后执行迁移。`, tone: "", button: "执行迁移…", intent: "apply", round: ex.name }
    : np ? { text: `${np.name} 待计划：${np.ack ? "先检查审核视图，再确认审核并生成计划。" : "检查目标结构后生成计划。"}`, tone: "", button: np.ack ? "确认并生成计划…" : "生成计划…", intent: "plan", round: np.name }
    : dn ? { text: `${dn.name}：已执行。归档用完的草案和计划。`, tone: "", button: "归档本轮…", intent: "archive", round: dn.name }
    : { text: "已同步：可以开始新一轮整理。", tone: "", button: "新建草案…", intent: "new_draft", round: null };
  const ch = (p: string, old: string | null = null) => ({ path: p, old_path: old, note: null });
  return {
    root: s === "uninit" ? "/Users/me/Downloads/Scans" : ROOT, name: s === "uninit" ? "Scans" : "Workspace",
    state: { code, label, tone: { uninit: "mute", unfinished: "bad", changed: "warn", clean: "ok" }[code]! }, next,
    gates: { draft: gate(code === "clean", blocked), plan: gate(code === "clean", blocked), sync: gate(code === "changed" && !pending, pending ? "先归档进行中的整理轮次" : blocked), archive: gate(pending && code !== "uninit" && code !== "unfinished", "没有待归档的文件"), copy_baseline: gate(code === "clean", blocked) },
    llm: { available: true, label: "gpt-x@api.example.com（远程）", host: "api.example.com", remote: true, no_content: false, reason: null, env_prefix: "ORGANIZER_LLM_BASE_URL='https://api.example.com/v1' ORGANIZER_LLM_MODEL='gpt-x' ORGANIZER_LLM_API_KEY=<钥匙串中的密钥>" },
    scanned_ms: state.scanned, busy: null, error: null, init_warning: null, interrupted: null,
    overview: code === "uninit" || code === "unfinished" ? null : {
      added: state.changes.added.map((p) => ch(p)), missing: [], moved: state.changes.moved.map(([a, b]) => ch(b, a)), changed: [], unknown: [], conflicts: [],
      boundaries: [{ tag: "P", path: "Projects/tool", note: "嵌套 Git 项目，受保护" }, { tag: "S", path: "latest.pdf", note: "符号链接或特殊文件，已跳过" }],
      errors: [], node_count: 11, git_head: state.commits[0].hash,
    },
    nodes: s === "uninit" ? [] : NODES,
    rounds: state.rounds.map((r) => ({ name: r.name, generated: r.generated, stage: STAGES[r.stage], stage_code: r.stage, blocked: r.stage === "applied" ? null : blocked, ack: r.plan ? null : r.ack, done: r.stage === "applied", invalid: null,
      plans: r.plan ? [{ name: r.plan.name, sha: "sha256:41a9d0e3", stage: STAGES[r.stage], stage_code: r.stage, ops: r.plan.ops.length, summary: "", script: r.plan.script }] : [], active: r.plan?.name ?? null,
      selectors: [`drafts/${r.name}`, ...(r.generated ? [`drafts/${r.name}.review.json`] : []), ...(r.plan ? [`plans/${r.plan.name}`] : []), ...(r.plan?.script ? [`plans/${r.plan.script}`] : [])] })),
    unpaired: [], others: [], commits: state.commits, history_error: null,
    runs: state.unfinished ? [{ run_id: "run_7b2e41c9", plan_id: "sha256:41a9d0e3", status: "failed", completed: 3, total: 4, error: "Permission denied" }] : [],
    recoveries: state.unfinished ? [{
      run_id: "run_7b2e41c9", status: "failed", completed: 3, total: 4, plan_id: "sha256:41a9d0e3",
      error: "migration run_7b2e41c9 stopped after 3/4 acknowledged operations: cannot move Research/Papers/ASLR.pdf: Permission denied (os error 13)",
      hint: "权限不足：检查相关目录的权限。", guidance: "Inspect each step; never re-run the plan blindly.",
      steps: PAPERS_OPS.map((op, i) => ({ position: i + 1, status: i < 3 ? "done" : "pending", op, disk: i < 3 ? "在目标" : "在源" })),
      finish_git: { ok: false, reason: "只有全部步骤已确认、且没有发布标记时才能补记 Git" }, mark_resolved: { ok: true, reason: null }, report_error: null,
    }] : [],
  };
}
const listeners = new Map<string, Set<(p: unknown) => void>>();
const emit = (event: string, payload: unknown) => listeners.get(event)?.forEach((h) => h(payload));
const result = (label: string, command: string, message: string, outcome: OutcomeView | null, kind = "ok"): ActionResult =>
  ({ label, command, kind, message, error: null, hint: null, go_recovery: false, outcome });

function command(a: GuiAction): string {
  switch (a.kind) {
    case "init": return `organizer init --root '/Users/me/Downloads/Scans'`;
    case "sync": return `organizer sync --root ${q(ROOT)}${a.accept_unknown ? " --accept-unknown" : ""}`;
    case "chat": return `ORGANIZER_LLM_MODEL='gpt-x' ORGANIZER_LLM_API_KEY=<钥匙串中的密钥> organizer chat --root ${q(ROOT)}${a.scope.map((s) => ` --scope ${s}`).join("")}${a.classify ? " --classify" : ""} --out ${q(`${OZ}/drafts/${a.out}`)} ${q(a.prompt)}`;
    case "copy_baseline": if (state.rounds.some((r) => r.name === a.name) || !a.name.endsWith(".tree")) throw `drafts/${a.name} 已存在或名称无效`; return `cp -n ${q(`${OZ}/structure.tree`)} ${q(`${OZ}/drafts/${a.name}`)}`;
    case "plan": return `organizer plan --root ${q(ROOT)} --from ${q(`${OZ}/structure.tree`)} --to ${q(`${OZ}/drafts/${a.round}`)}${a.acknowledge ? " --acknowledge-review" : ""} --out ${q(`${OZ}/plans/${a.out}`)}`;
    case "dry_run": return `organizer apply ${q(`${OZ}/plans/${a.plan}`)} --root ${q(ROOT)} --dry-run`;
    case "apply": return `organizer apply ${q(`${OZ}/plans/${a.plan}`)} --root ${q(ROOT)} --yes`;
    case "export": return `organizer export-script ${q(`${OZ}/plans/${a.plan}`)} --out ${q(`${OZ}/plans/${a.plan.replace(/\.json$/, ".sh")}`)}`;
    case "archive": return `organizer archive --root ${q(ROOT)} ${a.selectors.length ? a.selectors.filter((s) => !s.endsWith(".review.json")).map(q).join(" ") : "--all"}`;
    case "finish_git": return `organizer recover ${a.run} --root ${q(ROOT)} --finish-git`;
    case "mark_resolved": return `organizer recover ${a.run} --root ${q(ROOT)} --mark-resolved`;
  }
}

async function run(a: GuiAction) {
  const cmd = command(a);
  const round = (n: string) => state.rounds.find((r) => r.name === n || r.plan?.name === n)!;
  await sleep(250);
  let done: ActionResult;
  switch (a.kind) {
    case "init": state.scenario = "clean"; done = result("初始化", cmd, "初始化完成，没有移动任何文件", { kind: "synced", structure_changed: true, git_head: "3f8e2aa" }); break;
    case "sync": state.changes = { added: [], moved: [] }; state.commits.unshift({ hash: "c27d8f0", kind: "reconcile", subject: "reconcile: 2 added, 1 moved" }); done = result("同步", cmd, "已同步，结构变化已记录", { kind: "synced", structure_changed: true, git_head: "c27d8f0" }); break;
    case "plan": { const r = round(a.round); r.plan = { name: a.out, ops: r.generated ? PAPERS_OPS : INBOX_OPS, script: null }; r.stage = "executable"; done = result("生成计划", cmd, `已生成计划 plans/${a.out}，${r.plan.ops.length} 个操作`, { kind: "planned", file: a.out, written: true, ops: r.plan.ops.length, unresolved: [] }); break; }
    case "dry_run": state.token = `t${Date.now()}`; emit("gui", { kind: "done", result: result("预览", cmd, "预览通过。执行前会重新检查。", { kind: "previewed", plan: a.plan, ops: round(a.plan).plan!.ops.length, token: state.token }) }); emit("gui", { kind: "idle" }); return;
    case "apply": {
      const r = round(a.plan);
      for (let i = 1; i <= r.plan!.ops.length; i++) { emit("gui", { kind: "progress", done: i, total: r.plan!.ops.length }); await sleep(450); }
      r.stage = "applied"; state.commits.unshift({ hash: "e19b2c4", kind: "refactor", subject: `refactor: apply plan sha256:41a9d0e3 (${r.name})` });
      done = result("执行迁移", cmd, `迁移完成：${r.plan!.ops.length} 个操作`, { kind: "applied", run_id: "run_c41e9a07", ops: r.plan!.ops.length, git_head: "e19b2c4" }); break;
    }
    case "archive": { const moved = a.selectors.length ? a.selectors : state.rounds.flatMap((r) => [`drafts/${r.name}`]); state.rounds = a.selectors.length ? state.rounds.filter((r) => !a.selectors.includes(`drafts/${r.name}`)) : []; done = result("归档", cmd, `已归档 ${moved.length} 个文件，没有删除任何文件`, { kind: "archived", into: "20261106T0100Z-all", moved }); break; }
    case "copy_baseline": state.rounds.push({ name: a.name, generated: false, stage: "no_changes", ack: null, plan: null, text: BASE_TEXT, rows: INBOX_ROWS.filter((r) => !r.mark) }); done = result("复制当前结构", cmd, `已创建 drafts/${a.name}`, { kind: "draft", name: a.name }); break;
    case "chat": await sleep(2200); state.rounds.unshift({ name: a.out, generated: true, stage: "needs_plan", ack: "有 1 个待审核项", plan: null, text: PAPERS_TEXT, rows: PAPERS_ROWS }); done = result("生成 LLM 草案", cmd, "草案已生成，有 1 个待审核项", { kind: "chatted", draft: a.out, review_items: 1, first_review: "f_notes" }, "review"); break;
    case "export": round(a.plan).plan!.script = a.plan.replace(/\.json$/, ".sh"); done = result("导出脚本", cmd, "已导出脚本。脚本只调用不带 --yes 的 apply。", { kind: "script", file: a.plan.replace(/\.json$/, ".sh") }); break;
    case "finish_git": case "mark_resolved":
      state.unfinished = false; state.changes.moved = [["Research/Papers/CRUSH.pdf", "Research/Papers/C2Rust/CRUSH.pdf"]]; state.rounds.forEach((r) => (r.stage = "plan_stale"));
      done = result("标记已人工处理", cmd, "运行 run_7b2e41c9 状态：resolved", { kind: "recovered", run_id: a.run, status: "resolved" }); break;
  }
  emit("gui", { kind: "done", result: done });
  state.scanned = Date.now();
  emit("gui", { kind: "view", view: view() });
  emit("gui", { kind: "idle" });
}

const transport: Transport = {
  async invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    const a = args as Record<string, never>;
    await sleep(20);
    const r = (n: string) => state.rounds.find((x) => x.name === n);
    const out = ((): unknown => {
      switch (cmd) {
        case "window_info": return state.scenario === "welcome"
          ? { kind: "welcome", recent: [{ index: 0, name: "Workspace", path: "~/Documents/Workspace", state: "已同步", opened_ms: Date.now() - 3.6e6 }, { index: 1, name: "Thesis", path: "~/Desktop/Thesis", state: "迁移未完成", opened_ms: Date.now() - 9e7 }], tools: true, parent: null, child: null }
          : { kind: "workspace", root: ROOT, inspector: true };
        case "snapshot": return view();
        case "refresh": state.scanned = Date.now(); setTimeout(() => { emit("gui", { kind: "view", view: view() }); emit("gui", { kind: "idle" }); }, 200); return null;
        case "round_detail": { const x = r(a.name); if (!x) throw "没有轮次"; return detail(x); }
        case "preview_scope": return { ok: true, visible_nodes: (a.ids as string[]).length ? 6 : 11, editable_files: (a.ids as string[]).length ? 3 : 7, excluded: (a.ids as string[]).length ? 0 : 2, error: null, warn_files: false };
        case "draft_text": { const x = r(a.name)!; return { text: x.text, digest: `d${x.text.length}`, generated: x.generated }; }
        case "check_draft": { const text = a.text as string; const bad = text.split("\n").findIndex((l) => /^( *)/.exec(l)![1].length % 2 === 1);
          return { ok: bad < 0, line: bad < 0 ? null : bad + 1, error: bad < 0 ? null : "indentation must use two ASCII spaces per level", hint: null, added: 1, moved: 1, renamed: 0, described: 0, missing_files: text.includes("@f_todo") ? [] : ["Inbox/todo.txt"], new_files: [], needs_ack: false, review_error: null }; }
        case "save_draft": { const x = r(a.name)!; if (a.digest !== `d${x.text.length}`) return { ...result("保存草案", "GUI 内部操作（workspace::save_draft）", "保存失败", null, "failed"), error: "the draft changed on disk since it was loaded" }; x.text = a.text; setTimeout(() => emit("gui", { kind: "view", view: view() }), 100); return result("保存草案", "GUI 内部操作（workspace::save_draft）", `已保存 drafts/${a.name}，未做规范化`, { kind: "saved", name: a.name, digest: `d${(a.text as string).length}` }); }
        case "command": return command(a.action as GuiAction);
        case "run": { const act = a.action as GuiAction; command(act); if (act.kind === "apply" && act.token !== state.token) throw "预览已失效（界面已刷新或计划已变化），请重新预览"; void run(act); return null; }
        case "settings": return { preferences: { llm: { base_url: "https://api.example.com/v1", model: "gpt-x", local_only: false, no_content: false, disabled: false }, editor_app: null, scan_on_activate: false, reopen_last: true }, key_saved: state.key, env_locked: [], env_ignored: [], tools: true, diagnostics: "TreeSpec Organizer 0.1.0 (mock)" };
        case "set_api_key": state.key = a.key !== null; return null;
        case "reveal": throw "预览中不能打开 Finder";
        default: return null;
      }
    })();
    return out as T;
  },
  async listen<T>(event: string, handler: (payload: T) => void) {
    const set = listeners.get(event) ?? new Set();
    set.add(handler as (p: unknown) => void);
    listeners.set(event, set);
    return () => set.delete(handler as (p: unknown) => void);
  },
};

export function installMock(scenario: string) {
  state.scenario = scenario;
  state.rounds = scenario === "uninit" ? [] : rounds();
  if (scenario === "changed") state.changes = { added: ["Inbox/IMG_2031.heic", "Inbox/receipt.pdf"], moved: [["Inbox/todo.txt", "Projects/todo.txt"]] };
  if (scenario === "unfinished") {
    state.unfinished = true;
    state.rounds = rounds().slice(0, 1);
    Object.assign(state.rounds[0], { stage: "executable", ack: null, plan: { name: "papers.reviewed.json", ops: PAPERS_OPS, script: null } });
  }
  useTransport(transport);
  // Lets tests and previews send shell events (menu items, close guards).
  (globalThis as Record<string, unknown>).__mock = { emit };
}
