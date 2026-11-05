// Typed IPC wrappers (GUI design §3.4). The WebView only sends names and IDs;
// the Rust side turns them into paths and runs every check again.

import type { ActionResult } from "./bindings/ActionResult";
import type { DraftCheckView } from "./bindings/DraftCheckView";
import type { DraftTextView } from "./bindings/DraftTextView";
import type { GuardView } from "./bindings/GuardView";
import type { GuiAction } from "./bindings/GuiAction";
import type { GuiEvent } from "./bindings/GuiEvent";
import type { Preferences } from "./bindings/Preferences";
import type { RevealTarget } from "./bindings/RevealTarget";
import type { RoundDetail } from "./bindings/RoundDetail";
import type { ScopeView } from "./bindings/ScopeView";
import type { SettingsView } from "./bindings/SettingsView";
import type { SystemRequest } from "./bindings/SystemRequest";
import type { ViewModel } from "./bindings/ViewModel";
import type { WindowInfo } from "./bindings/WindowInfo";

export interface Transport {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** Listens to events sent to this window only. */
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

let transport: Transport | null = null;

export function useTransport(t: Transport) {
  transport = t;
}

async function tauri(): Promise<Transport> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  const window = getCurrentWebviewWindow();
  return {
    invoke: (command, args) => invoke(command, args),
    listen: (event, handler) => window.listen(event, (e) => handler(e.payload as never)),
  };
}

async function t(): Promise<Transport> {
  transport ??= await tauri();
  return transport;
}

const call = async <T>(command: string, args?: Record<string, unknown>) => (await t()).invoke<T>(command, args);

export const api = {
  windowInfo: () => call<WindowInfo>("window_info"),
  pickFolder: () => call<void>("pick_folder"),
  openRecent: (index: number) => call<void>("open_recent", { index }),
  openParent: (open: boolean) => call<void>("open_parent", { open }),
  snapshot: () => call<ViewModel | null>("snapshot"),
  refresh: () => call<void>("refresh"),
  roundDetail: (name: string) => call<RoundDetail>("round_detail", { name }),
  previewScope: (ids: string[]) => call<ScopeView>("preview_scope", { ids }),
  draftText: (name: string) => call<DraftTextView>("draft_text", { name }),
  checkDraft: (name: string, text: string) => call<DraftCheckView>("check_draft", { name, text }),
  saveDraft: (name: string, digest: string, text: string) => call<ActionResult>("save_draft", { name, digest, text }),
  run: (action: GuiAction) => call<void>("run", { action }),
  /** The equivalent command for a confirmation sheet (built in Rust). */
  command: (action: GuiAction) => call<string>("command", { action }),
  reveal: (target: RevealTarget) => call<void>("reveal", { target }),
  settings: () => call<SettingsView>("settings"),
  setSettings: (prefs: Preferences) => call<void>("set_settings", { prefs }),
  setApiKey: (key: string | null) => call<void>("set_api_key", { key }),
  system: (request: SystemRequest) => call<void>("system", { request }),
  setGuard: (dirty: boolean) => call<void>("set_guard", { dirty }),
  forceClose: (quit: boolean) => call<void>("force_close", { quit }),
};

export const events = {
  gui: async (h: (e: GuiEvent) => void) => (await t()).listen<GuiEvent>("gui", h),
  menu: async (h: (id: string) => void) => (await t()).listen<string>("menu", h),
  guard: async (h: (g: GuardView) => void) => (await t()).listen<GuardView>("guard", h),
  activated: async (h: () => void) => (await t()).listen<null>("activated", () => h()),
  welcome: async (h: () => void) => (await t()).listen<null>("welcome", () => h()),
};

/** Errors from commands are plain strings. */
export function message(error: unknown): string {
  return typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
}
