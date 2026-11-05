// Window state (Svelte 5 runes). All decisions about what is allowed come from
// the Rust view model; this only remembers what the user is looking at.

import { api, message } from "../ipc/api";
import type { ActionResult } from "../ipc/bindings/ActionResult";
import type { GuardView } from "../ipc/bindings/GuardView";
import type { GuiEvent } from "../ipc/bindings/GuiEvent";
import type { RoundDetail } from "../ipc/bindings/RoundDetail";
import type { ViewModel } from "../ipc/bindings/ViewModel";

export type Page = "overview" | "structure" | "organize" | "history" | "recovery";
export type Segment = "review" | "edit" | "plan" | "report";

export type Sheet =
  | { kind: "init" }
  | { kind: "sync" }
  | { kind: "chat" }
  | { kind: "chat_wait"; started: number }
  | { kind: "copy"; name: string }
  | { kind: "new" }
  | { kind: "plan"; round: string }
  | { kind: "apply"; plan: string; token: string; ops: number }
  | { kind: "apply_run"; plan: string }
  | { kind: "apply_done"; result: ActionResult; round: string | null }
  | { kind: "archive"; round: string | null; selectors: string[] }
  | { kind: "export"; plan: string }
  | { kind: "finish_git"; run: string }
  | { kind: "resolve"; run: string }
  | { kind: "result"; result: ActionResult }
  | { kind: "guard"; guard: GuardView }
  | { kind: "settings" }
  | { kind: "unsaved"; then: () => void };

export interface LogEntry {
  time: string;
  label: string;
  command: string;
  kind: string;
  message: string;
}

export interface Editor {
  round: string;
  text: string;
  saved: string;
  digest: string;
  generated: boolean;
  external: boolean;
}

export const hhmm = (ms: number) =>
  new Date(ms).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false });

class AppState {
  view = $state<ViewModel | null>(null);
  page = $state<Page>("overview");
  round = $state<string | null>(null);
  segment = $state<Segment>("review");
  detail = $state<RoundDetail | null>(null);
  selected = $state<string | null>(null);
  scope = $state<string[]>([]);
  onlyChanges = $state(false);
  inspector = $state(true);
  showLog = $state(false);
  log = $state<LogEntry[]>([]);
  sheet = $state<Sheet | null>(null);
  busy = $state<string | null>(null);
  progress = $state<{ done: number; total: number } | null>(null);
  /** Session-only preview results by plan file name (design §3.2). */
  previews = $state<Record<string, string>>({});
  editor = $state<Editor | null>(null);
  toast = $state<string | null>(null);
  #toastTimer: ReturnType<typeof setTimeout> | undefined;
  #pending: string | null = null;

  get dirty() {
    return !!this.editor && this.editor.text !== this.editor.saved;
  }

  get currentRound() {
    return this.view?.rounds.find((r) => r.name === this.round) ?? null;
  }

  notify(text: string) {
    this.toast = text;
    clearTimeout(this.#toastTimer);
    this.#toastTimer = setTimeout(() => (this.toast = null), 3000);
  }

  record(r: ActionResult) {
    this.log.unshift({ time: hhmm(Date.now()), label: r.label, command: r.command, kind: r.kind, message: r.message });
  }

  /** Events from the session, in order (design §3.4). */
  handle(event: GuiEvent) {
    switch (event.kind) {
      case "view":
        this.setView(event.view);
        break;
      case "progress":
        this.progress = { done: event.done, total: event.total };
        break;
      case "done":
        this.onDone(event.result);
        break;
      case "idle":
        this.busy = null;
        this.progress = null;
        break;
    }
  }

  setView(view: ViewModel) {
    const first = !this.view;
    this.view = view;
    this.busy = view.busy;
    this.previews = {};
    if (first && view.recoveries.length) this.page = "recovery";
    if (view.state.code === "uninit") this.page = "overview";
    if (this.round && !view.rounds.some((r) => r.name === this.round)) this.round = null;
    this.round ??= view.rounds[0]?.name ?? null;
    this.scope = this.scope.filter((id) => view.nodes.some((n) => n.id === id && n.dir && !n.excluded));
    if (this.#pending && view.rounds.some((r) => r.name === this.#pending)) {
      this.round = this.#pending;
      this.#pending = null;
    }
    void this.loadDetail();
  }

  async loadDetail() {
    const name = this.round;
    if (!name || !this.view) {
      this.detail = null;
      return;
    }
    try {
      const detail = await api.roundDetail(name);
      if (this.round === name) this.detail = detail;
    } catch {
      this.detail = null;
    }
  }

  selectRound(name: string, segment: Segment = "review") {
    this.page = "organize";
    if (this.round !== name) {
      this.round = name;
      this.selected = null;
      this.detail = null;
      void this.loadDetail();
    }
    this.segment = segment;
  }

  onDone(r: ActionResult) {
    this.record(r);
    const o = r.outcome;
    if (r.kind === "failed") {
      this.sheet = { kind: "result", result: r };
      return;
    }
    switch (o?.kind) {
      case "previewed":
        this.previews = { ...this.previews, [o.plan]: hhmm(Date.now()) };
        if (this.sheet?.kind === "apply_run") this.sheet = { kind: "apply", plan: o.plan, token: o.token, ops: o.ops };
        else this.notify(r.message);
        break;
      case "applied":
        this.sheet = { kind: "apply_done", result: r, round: this.round };
        break;
      case "chatted":
        this.sheet = null;
        this.#pending = o.draft;
        this.round = o.draft;
        this.segment = "review";
        this.page = "organize";
        this.selected = o.first_review;
        this.notify(r.message);
        break;
      case "planned":
        this.sheet = null;
        this.segment = "plan";
        this.notify(r.message);
        break;
      case "draft":
        this.sheet = null;
        this.#pending = o.name;
        this.round = o.name;
        this.page = "organize";
        this.segment = "edit";
        break;
      default:
        if (this.sheet && this.sheet.kind !== "apply_done") this.sheet = null;
        this.notify(r.message);
    }
  }

  async run(action: Parameters<typeof api.run>[0], label = "正在处理") {
    try {
      this.busy = label;
      await api.run(action);
    } catch (e) {
      this.busy = null;
      if (this.sheet?.kind === "apply_run" || this.sheet?.kind === "chat_wait") this.sheet = null;
      this.notify(message(e));
    }
  }
}

export const app = new AppState();
