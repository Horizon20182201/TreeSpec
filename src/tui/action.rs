//! Every TUI write action, described once. The worker executes it through the
//! library and the confirmation dialogs/log show [`Action::command`]; the
//! parity tests parse [`Action::argv`] with the real CLI parser.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Init {
        root: PathBuf,
    },
    Sync {
        root: PathBuf,
        accept_unknown: bool,
    },
    Chat {
        root: PathBuf,
        prompt: String,
        scope_ids: Vec<String>,
        classify: bool,
        read_content: bool,
        out: PathBuf,
    },
    /// The TUI writes `out` only when the plan has no unresolved items.
    Plan {
        root: PathBuf,
        to: PathBuf,
        acknowledge_review: bool,
        out: PathBuf,
    },
    DryRun {
        root: PathBuf,
        plan: PathBuf,
    },
    Apply {
        root: PathBuf,
        plan: PathBuf,
    },
    ExportScript {
        plan: PathBuf,
        out: PathBuf,
    },
    /// Empty `entries` archives everything pending.
    Archive {
        root: PathBuf,
        entries: Vec<String>,
    },
    FinishGit {
        root: PathBuf,
        run_id: String,
    },
    MarkResolved {
        root: PathBuf,
        run_id: String,
    },
    /// README's `cp -n structure.tree drafts/NAME`; not an organizer command.
    CopyBaseline {
        root: PathBuf,
        name: String,
    },
}

fn path(value: &Path) -> String {
    value.to_string_lossy().into_owned()
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Init { .. } => "初始化",
            Self::Sync { .. } => "同步",
            Self::Chat { .. } => "生成 LLM 草案",
            Self::Plan { .. } => "生成计划",
            Self::DryRun { .. } => "预览",
            Self::Apply { .. } => "执行迁移",
            Self::ExportScript { .. } => "导出脚本",
            Self::Archive { .. } => "归档",
            Self::FinishGit { .. } => "补记 Git",
            Self::MarkResolved { .. } => "标记已人工处理",
            Self::CopyBaseline { .. } => "复制当前结构",
        }
    }

    /// Whether the action may change `.organizer` (a refresh follows it).
    pub fn writes(&self) -> bool {
        !matches!(self, Self::DryRun { .. })
    }

    /// Arguments after the program name.
    pub fn argv(&self) -> Vec<String> {
        let s = |v: &str| v.to_owned();
        match self {
            Self::Init { root } => vec![s("init"), s("--root"), path(root)],
            Self::Sync {
                root,
                accept_unknown,
            } => {
                let mut args = vec![s("sync"), s("--root"), path(root)];
                if *accept_unknown {
                    args.push(s("--accept-unknown"));
                }
                args
            }
            Self::Chat {
                root,
                prompt,
                scope_ids,
                classify,
                read_content,
                out,
            } => {
                let mut args = vec![s("chat"), s("--root"), path(root)];
                for id in scope_ids {
                    args.extend([s("--scope"), id.clone()]);
                }
                if *classify {
                    args.push(s("--classify"));
                }
                if *read_content {
                    args.push(s("--read-content"));
                }
                args.extend([s("--out"), path(out), s("--"), prompt.clone()]);
                args
            }
            Self::Plan {
                root,
                to,
                acknowledge_review,
                out,
            } => {
                let mut args = vec![
                    s("plan"),
                    s("--root"),
                    path(root),
                    s("--from"),
                    path(&root.join(".organizer/structure.tree")),
                    s("--to"),
                    path(to),
                ];
                if *acknowledge_review {
                    args.push(s("--acknowledge-review"));
                }
                args.extend([s("--out"), path(out)]);
                args
            }
            Self::DryRun { root, plan } => {
                vec![
                    s("apply"),
                    path(plan),
                    s("--root"),
                    path(root),
                    s("--dry-run"),
                ]
            }
            Self::Apply { root, plan } => {
                vec![s("apply"), path(plan), s("--root"), path(root), s("--yes")]
            }
            Self::ExportScript { plan, out } => {
                vec![s("export-script"), path(plan), s("--out"), path(out)]
            }
            Self::Archive { root, entries } => {
                let mut args = vec![s("archive"), s("--root"), path(root)];
                if entries.is_empty() {
                    args.push(s("--all"));
                } else {
                    args.extend(entries.iter().cloned());
                }
                args
            }
            Self::FinishGit { root, run_id } => {
                vec![
                    s("recover"),
                    run_id.clone(),
                    s("--root"),
                    path(root),
                    s("--finish-git"),
                ]
            }
            Self::MarkResolved { root, run_id } => {
                vec![
                    s("recover"),
                    run_id.clone(),
                    s("--root"),
                    path(root),
                    s("--mark-resolved"),
                ]
            }
            Self::CopyBaseline { root, name } => vec![
                s("-n"),
                path(&root.join(".organizer/structure.tree")),
                path(&root.join(".organizer/drafts").join(name)),
            ],
        }
    }

    /// Shell form for display, quoted like `export-script`.
    pub fn command(&self) -> String {
        let program = match self {
            Self::CopyBaseline { .. } => "cp",
            _ => "organizer",
        };
        std::iter::once(program.to_owned())
            .chain(self.argv().iter().map(|arg| quote(arg)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Single-quote unless the argument is plainly safe, matching export-script.
pub fn quote(value: &str) -> String {
    let safe = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c));
    if safe {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
