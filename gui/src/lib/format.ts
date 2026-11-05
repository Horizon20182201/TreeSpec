// Labels for marks and tones. Stage and state names come from Rust.

/** Review marks (TUI design §5.3): class and spoken label. */
export const MARKS: Record<string, [string, string]> = {
  "+": ["acc", "新目录"],
  "~": ["acc", "改名"],
  ">": ["acc", "移到新目录"],
  "^": ["", "随上级移动"],
  "#": ["", "说明变化"],
  "?": ["rev", "待审核"],
  "!": ["bad", "阻断"],
};

const STAGE_TONE: Record<string, string> = {
  needs_plan: "acc",
  executable: "ok",
  applied: "",
  nothing_to_apply: "",
  no_changes: "",
  plan_stale: "warn",
  stale: "warn",
  blocked: "bad",
  invalid: "bad",
};

export const stageTone = (code: string) => STAGE_TONE[code] ?? "";

export const OPS: Record<string, string> = {
  mkdir: "新建目录",
  move_file: "移动文件",
  move_directory: "移动目录",
};

/** Single-quoted like `export-script`, for commands assembled in the page. */
export const quote = (s: string) => `'${s.replaceAll("'", "'\\''")}'`;

export const bytes = (s: string) => new TextEncoder().encode(s).length;

/** Random `chat_XXXXXXXX.tree`, like the TUI default. */
export function chatName() {
  const hex = Array.from(crypto.getRandomValues(new Uint8Array(4)), (b) => b.toString(16).padStart(2, "0")).join("");
  return `chat_${hex}.tree`;
}

export const isMock = import.meta.env.VITE_MOCK === "1";
