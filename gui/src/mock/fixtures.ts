// Example data for the in-memory backend (browser previews, Playwright). The
// same workspace as docs/GUI_MOCKUP.html. Not part of the Tauri build.

import type { NodeView } from "../ipc/bindings/NodeView";
import type { OpView } from "../ipc/bindings/OpView";
import type { RoundDetail } from "../ipc/bindings/RoundDetail";
import type { TreeRowView } from "../ipc/bindings/TreeRowView";
import type { ViewModel } from "../ipc/bindings/ViewModel";

export const ROOT = "/Users/me/Documents/Workspace";

type N = [string, string | null, string, boolean, string, string?];
const RAW: N[] = [
  ["d_inbox", null, "Inbox", true, "暂未分类"], ["d_projects", null, "Projects", true, ""], ["d_research", null, "Research", true, "科研资料"],
  ["f_env", null, ".env", false, "", "敏感名称或受保护项：不发送给 LLM，不能选为范围"],
  ["f_scan", "d_inbox", "scan_0412.pdf", false, ""], ["f_todo", "d_inbox", "todo.txt", false, ""],
  ["d_tool", "d_projects", "tool", true, "", "受保护的嵌套 Git 项目：不发送给 LLM，不能选为范围"],
  ["d_papers", "d_research", "Papers", true, "学术论文"],
  ["f_aslr", "d_papers", "ASLR.pdf", false, ""], ["f_crush", "d_papers", "CRUSH.pdf", false, ""], ["f_notes", "d_papers", "notes.pdf", false, ""],
];
const ORDER = ["d_inbox", "f_scan", "f_todo", "d_projects", "d_tool", "d_research", "d_papers", "f_aslr", "f_crush", "f_notes", "f_env"];

const path = (id: string): string => {
  const n = RAW.find((r) => r[0] === id)!;
  return n[1] ? `${path(n[1])}/${n[2]}` : n[2];
};
export const NODES: NodeView[] = ORDER.map((id) => {
  const [, parent, name, dir, description, excluded] = RAW.find((r) => r[0] === id)!;
  const kids = RAW.filter((r) => r[1] === id);
  return { id, parent, name, dir, description, excluded: excluded ?? null, path: path(id), depth: path(id).split("/").length - 1, child_dirs: kids.filter((k) => k[3]).length, child_files: kids.filter((k) => !k[3]).length };
});

const row = (id: string, depth: number, name: string, dir: boolean, mark: string | null = null, note: string | null = null, detail: string[] = []): TreeRowView =>
  ({ id, depth, name, dir, description: "", path: name, mark, note, excluded: id === "f_env" || id === "d_tool", detail });

export const PAPERS_ROWS: TreeRowView[] = [
  row("d_inbox", 0, "Inbox", true), row("f_scan", 1, "scan_0412.pdf", false), row("f_todo", 1, "todo.txt", false),
  row("d_projects", 0, "Projects", true), row("d_tool", 1, "tool", true), row("d_research", 0, "Research", true),
  row("d_papers", 1, "Papers", true, "#", "说明已改", ["Research/Papers  d_papers"]),
  row("d_c2rust", 2, "C2Rust", true, "+", "新目录", ["Research/Papers/C2Rust  d_c2rust", "新节点"]),
  row("f_crush", 3, "CRUSH.pdf", false, ">", "自 Papers/", ["Research/Papers/C2Rust/CRUSH.pdf  f_crush", "原路径 Research/Papers/CRUSH.pdf", "决定 place → Research/Papers/C2Rust", "理由 标题是 “Semantics-guided C-to-Rust translation”，属于 C2Rust 主题。"]),
  row("d_security", 2, "Security", true, "+", "新目录", ["Research/Papers/Security  d_security", "新节点"]),
  row("f_aslr", 3, "ASLR.pdf", false, ">", "自 Papers/", ["Research/Papers/Security/ASLR.pdf  f_aslr", "原路径 Research/Papers/ASLR.pdf", "决定 place → Research/Papers/Security", "理由 ASLR 是地址空间布局随机化，属于系统安全主题。"]),
  row("f_notes", 2, "notes.pdf", false, "?", "待审核", ["Research/Papers/notes.pdf  f_notes", "决定 review", "理由 文件名无法判断主题，保留原位，等待人工决定。", "待审核：文件名无法判断主题"]),
  row("f_env", 0, ".env", false),
];
export const INBOX_ROWS: TreeRowView[] = [
  row("d_inbox", 0, "Inbox", true), row("d_archive", 1, "Archive", true, "+", "新目录", ["Inbox/Archive  d_archive", "新节点"]),
  row("f_scan", 2, "scan_0412.pdf", false, ">", "自 Inbox/", ["Inbox/Archive/scan_0412.pdf  f_scan", "原路径 Inbox/scan_0412.pdf"]),
  row("f_todo", 1, "todo.txt", false), row("d_projects", 0, "Projects", true), row("d_tool", 1, "tool", true),
  row("d_research", 0, "Research", true), row("d_papers", 1, "Papers", true), row("f_aslr", 2, "ASLR.pdf", false),
  row("f_crush", 2, "CRUSH.pdf", false), row("f_notes", 2, "notes.pdf", false), row("f_env", 0, ".env", false),
];
const op = (kind: string, to: string, from: string | null = null): OpView => ({ kind, from, to });
export const PAPERS_OPS = [op("mkdir", "Research/Papers/C2Rust"), op("mkdir", "Research/Papers/Security"), op("move_file", "Research/Papers/C2Rust/CRUSH.pdf", "Research/Papers/CRUSH.pdf"), op("move_file", "Research/Papers/Security/ASLR.pdf", "Research/Papers/ASLR.pdf")];
export const INBOX_OPS = [op("mkdir", "Inbox/Archive"), op("move_file", "Inbox/Archive/scan_0412.pdf", "Inbox/scan_0412.pdf")];

export const BASE_TEXT = `!tree 1

Inbox/ @d_inbox # 暂未分类
  scan_0412.pdf @f_scan
  todo.txt @f_todo
Projects/ @d_projects
  tool/ @d_tool
Research/ @d_research # 科研资料
  Papers/ @d_papers # 学术论文
    ASLR.pdf @f_aslr
    CRUSH.pdf @f_crush
    notes.pdf @f_notes
.env @f_env
`;
export const PAPERS_TEXT = "# organizer-review-v1: 6f1d2c0e-91b4-4c55-a0d2-3e8c41f7b9aa\n" + BASE_TEXT
  .replace("Papers/ @d_papers # 学术论文\n    ASLR.pdf @f_aslr\n    CRUSH.pdf @f_crush\n", "Papers/ @d_papers # 学术论文（按主题）\n    C2Rust/ @d_c2rust\n      CRUSH.pdf @f_crush\n    Security/ @d_security\n      ASLR.pdf @f_aslr\n");
export const INBOX_TEXT = BASE_TEXT.replace("  scan_0412.pdf @f_scan\n", "  Archive/ # 扫描件\n    scan_0412.pdf @f_scan\n");

export interface MockRound {
  name: string;
  generated: boolean;
  stage: string;
  ack: string | null;
  plan: { name: string; ops: OpView[]; script: string | null } | null;
  text: string;
  rows: TreeRowView[];
}

export const STAGES: Record<string, string> = { needs_plan: "待计划", executable: "可执行", applied: "已执行", plan_stale: "计划已过期", no_changes: "无变化" };

export function detail(r: MockRound): RoundDetail {
  const counts = new Map<string, number>();
  for (const x of r.rows) if (x.mark) counts.set(x.mark, (counts.get(x.mark) ?? 0) + 1);
  return {
    name: r.name, generated: r.generated, stage: STAGES[r.stage], stage_code: r.stage, ack: r.plan ? null : r.ack, invalid: null,
    rows: r.rows, missing: [], counts: [...counts].map(([mark, count]) => ({ mark, count })),
    report: r.generated ? {
      edit_count: 3, bytes_read: 0, base_digest: "sha256:5c1e09ab", target_digest: "sha256:b07e1d44",
      assignments: [
        { file: "Research/Papers/CRUSH.pdf", decision: "place", target: "Research/Papers/C2Rust", reason: "标题是 “Semantics-guided C-to-Rust translation”，属于 C2Rust 主题。" },
        { file: "Research/Papers/ASLR.pdf", decision: "place", target: "Research/Papers/Security", reason: "ASLR 是地址空间布局随机化，属于系统安全主题。" },
        { file: "Research/Papers/notes.pdf", decision: "review", target: null, reason: "文件名无法判断主题，保留原位，等待人工决定。" },
      ],
      unresolved: ["文件名无法判断主题"], warnings: [],
    } : null,
    plan: r.plan ? { name: r.plan.name, sha: "sha256:41a9d0e3", stage: STAGES[r.stage], stage_code: r.stage, script: r.plan.script, ops: r.plan.ops, unresolved: [], boundaries: [{ tag: "P", path: "Projects/tool", note: "嵌套 Git 项目，受保护" }], warnings: [] } : null,
  };
}

export type { ViewModel };
