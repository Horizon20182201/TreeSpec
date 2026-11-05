//! One session per workspace window (design §3.3 item 1, §3.5): a worker
//! thread that runs jobs serially through the shared `frontend::worker`, the
//! last snapshot, apply tokens, and the close/quit guard.

use crate::{
    action::{self, GuiAction},
    dto::{self, ActionResult, GuiEvent, OutcomeView, ViewModel},
    secrets::SecretStore,
    settings::{Settings, now_ms},
    system::Caffeinate,
    token::Tokens,
};
use organizer::{
    frontend::{
        action::Action,
        check::check_draft,
        llm_info,
        state::Snapshot,
        text::{clean, hint},
        worker::{self, Context, Event, Job},
    },
    llm::provider::OpenAiCompatible,
    workspace,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
};

/// Settings, Keychain and the settings file path, shared by all windows.
pub struct AppCore {
    pub settings: Mutex<Settings>,
    pub secrets: Box<dyn SecretStore>,
    pub settings_path: PathBuf,
    /// Whether the Keychain holds a key; refreshed when it is set.
    pub key_saved: Mutex<bool>,
}

impl AppCore {
    pub fn new(settings_path: PathBuf, secrets: Box<dyn SecretStore>) -> Arc<Self> {
        let settings = Settings::load(&settings_path);
        let key_saved = secrets.get().ok().flatten().is_some();
        Arc::new(Self {
            settings: Mutex::new(settings),
            secrets,
            settings_path,
            key_saved: Mutex::new(key_saved),
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let settings = self.settings.lock().unwrap().clone();
        settings
            .save(&self.settings_path)
            .map_err(|e| format!("无法保存设置：{e}"))
    }

    pub fn settings_view(&self) -> dto::SettingsView {
        let preferences = self.settings.lock().unwrap().preferences.clone();
        let locked = [
            "ORGANIZER_LLM_DISABLED",
            "ORGANIZER_LLM_LOCAL_ONLY",
            "ORGANIZER_LLM_NO_CONTENT",
        ]
        .into_iter()
        .filter(|n| std::env::var(n).is_ok_and(|v| v == "1"))
        .map(Into::into)
        .collect();
        let ignored = [
            "ORGANIZER_LLM_BASE_URL",
            "ORGANIZER_LLM_MODEL",
            "ORGANIZER_LLM_API_KEY",
        ]
        .into_iter()
        .filter(|n| std::env::var_os(n).is_some())
        .map(Into::into)
        .collect();
        let tools = crate::system::command_line_tools();
        let diagnostics = format!(
            "TreeSpec Organizer {}\nmacOS {}\n命令行工具：{}\nLLM：{}",
            env!("CARGO_PKG_VERSION"),
            crate::system::os_version(),
            if tools { "已安装" } else { "未安装" },
            self.llm_view().label
        );
        dto::SettingsView {
            preferences,
            key_saved: *self.key_saved.lock().unwrap(),
            env_locked: locked,
            env_ignored: ignored,
            tools,
            diagnostics,
        }
    }

    pub fn set_preferences(&self, prefs: crate::settings::Preferences) -> Result<(), String> {
        let bad = |s: &str| s.chars().any(char::is_control) || s.len() > 2048;
        let url = prefs.llm.base_url.trim();
        if bad(url)
            || !(url.is_empty() || url.starts_with("https://") || url.starts_with("http://"))
        {
            return Err("服务地址必须以 https:// 开头（本地服务可用 http://）".into());
        }
        if bad(&prefs.llm.model) {
            return Err("模型名称不能包含控制字符".into());
        }
        if let Some(app) = &prefs.editor_app
            && (bad(app) || !app.starts_with('/') || !app.ends_with(".app"))
        {
            return Err("外部编辑器必须是一个 .app 应用".into());
        }
        self.settings.lock().unwrap().preferences = prefs;
        self.save()
    }

    /// `None` or an empty key removes it from the Keychain.
    pub fn set_api_key(&self, key: Option<String>) -> Result<(), String> {
        let key = key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty());
        if key
            .as_ref()
            .is_some_and(|k| k.len() > 4096 || k.chars().any(char::is_control))
        {
            return Err("API Key 格式不正确".into());
        }
        self.secrets.set(key.as_deref())?;
        *self.key_saved.lock().unwrap() = key.is_some();
        Ok(())
    }

    /// The LLM summary for the current settings, validated offline.
    pub fn llm_view(&self) -> dto::LlmView {
        let llm = self.settings.lock().unwrap().preferences.llm.clone();
        let key_saved = *self.key_saved.lock().unwrap();
        let config = llm.provider_config(key_saved.then(|| "placeholder".into()));
        let no_content = llm.policy().effective().map_or(true, |p| p.no_content);
        let info = llm_info::detect_with(config, no_content);
        dto::llm_view(&info, &llm.base_url, &llm.model, key_saved)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Refresh,
    Apply,
    Chat,
    Other,
}

/// Why closing the window or quitting needs confirmation (design §8.3, §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    Applying,
    Chatting,
    Unsaved,
}

pub type Sink = Arc<dyn Fn(GuiEvent) + Send + Sync>;

#[derive(Default)]
struct Inner {
    snapshot: Option<Arc<Snapshot>>,
    scanned_ms: f64,
    queue: VecDeque<Kind>,
    tokens: Tokens,
    caffeinate: Option<Caffeinate>,
    dirty: bool,
}

pub struct Session {
    pub root: PathBuf,
    jobs: mpsc::Sender<Job>,
    inner: Arc<Mutex<Inner>>,
    core: Arc<AppCore>,
    sink: Sink,
}

fn busy_label(queue: &VecDeque<Kind>) -> Option<String> {
    queue.iter().find(|k| **k != Kind::Refresh).map(|k| {
        match k {
            Kind::Apply => "正在执行迁移",
            Kind::Chat => "正在生成草案",
            _ => "正在处理",
        }
        .to_owned()
    })
}

impl Session {
    /// `provider` overrides the real LLM provider (tests use a mock).
    pub fn spawn(
        root: PathBuf,
        core: Arc<AppCore>,
        sink: Sink,
        provider: Option<Box<worker::ProviderFactory>>,
    ) -> Arc<Self> {
        let provider = provider.unwrap_or_else(|| {
            let core = core.clone();
            Box::new(move || {
                let llm = core.settings.lock().unwrap().preferences.llm.clone();
                let key = core.secrets.get()?;
                let config = llm.provider_config(key).map_err(|e| e.to_string())?;
                Ok(Box::new(OpenAiCompatible::new(config).map_err(|e| e.to_string())?) as _)
            })
        });
        let policy = {
            let core = core.clone();
            Box::new(move || core.settings.lock().unwrap().preferences.llm.policy())
        };
        let (jobs, events) = worker::spawn(Context {
            root: root.clone(),
            provider,
            policy,
        });
        let session = Arc::new(Self {
            root,
            jobs,
            inner: Arc::default(),
            core,
            sink,
        });
        // Weak: closing the window drops the session, which drops the job
        // sender, which ends the worker and then this thread.
        let forwarder = Arc::downgrade(&session);
        std::thread::Builder::new()
            .name("organizer-gui-events".into())
            .spawn(move || {
                for event in events {
                    let Some(session) = forwarder.upgrade() else {
                        break;
                    };
                    session.forward(event);
                }
            })
            .expect("cannot start the event thread");
        session.send(Job::Refresh, Kind::Refresh);
        session
    }

    /// Queue entry and channel send happen under one lock, so the queue
    /// always matches the order in which the worker finishes jobs.
    fn send(&self, job: Job, kind: Kind) {
        let mut inner = self.inner.lock().unwrap();
        if self.jobs.send(job).is_ok() {
            inner.queue.push_back(kind);
        }
    }

    fn forward(&self, event: Event) {
        match event {
            Event::Snapshot(snapshot) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tokens.clear();
                inner.scanned_ms = now_ms();
                let state = snapshot.classify().label();
                inner.snapshot = Some(Arc::from(snapshot));
                let view = self.build(&inner);
                drop(inner);
                self.note_state(state);
                (self.sink)(GuiEvent::View {
                    view: Box::new(view),
                });
            }
            Event::Progress(done, total) => (self.sink)(GuiEvent::Progress {
                done: u32::try_from(done).unwrap_or(u32::MAX),
                total: u32::try_from(total).unwrap_or(u32::MAX),
            }),
            Event::Done(done) => {
                let (action, result) = *done;
                let token = match (&action, &result) {
                    (Action::DryRun { plan, .. }, Ok(worker::Outcome::DryRun(report))) => {
                        let name = plan
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        Some(
                            self.inner
                                .lock()
                                .unwrap()
                                .tokens
                                .issue(&name, &report.plan_id),
                        )
                    }
                    _ => None,
                };
                let prefix = self.core.llm_view().env_prefix;
                let result = dto::action_result(&action, &result, &prefix, token);
                (self.sink)(GuiEvent::Done {
                    result: Box::new(result),
                });
            }
            Event::Finished => {
                let mut inner = self.inner.lock().unwrap();
                inner.queue.pop_front();
                if !inner.queue.contains(&Kind::Apply) {
                    inner.caffeinate = None;
                }
                let idle = inner.queue.is_empty();
                drop(inner);
                if idle {
                    (self.sink)(GuiEvent::Idle);
                }
            }
        }
    }

    fn note_state(&self, state: &str) {
        let mut settings = self.core.settings.lock().unwrap();
        settings.note_state(&self.root, state);
        drop(settings);
        let _ = self.core.save();
    }

    fn build(&self, inner: &Inner) -> ViewModel {
        let snapshot = inner.snapshot.as_ref().expect("snapshot");
        dto::view_model(
            &self.root,
            snapshot,
            self.core.llm_view(),
            busy_label(&inner.queue),
            inner.scanned_ms,
        )
    }

    /// The last view model, if the first refresh has finished.
    pub fn view(&self) -> Option<ViewModel> {
        let inner = self.inner.lock().unwrap();
        inner.snapshot.as_ref().map(|_| self.build(&inner))
    }

    /// Re-sends the view (after settings changed).
    pub fn reemit(&self) {
        if let Some(view) = self.view() {
            (self.sink)(GuiEvent::View {
                view: Box::new(view),
            });
        }
    }

    pub fn refresh(&self) {
        self.send(Job::Refresh, Kind::Refresh);
    }

    pub fn busy(&self) -> Option<String> {
        busy_label(&self.inner.lock().unwrap().queue)
    }

    pub fn guard(&self) -> Option<Guard> {
        let inner = self.inner.lock().unwrap();
        if inner.queue.contains(&Kind::Apply) {
            Some(Guard::Applying)
        } else if inner.queue.contains(&Kind::Chat) {
            Some(Guard::Chatting)
        } else if inner.dirty {
            Some(Guard::Unsaved)
        } else {
            None
        }
    }

    pub fn set_dirty(&self, dirty: bool) {
        self.inner.lock().unwrap().dirty = dirty;
    }

    fn with_snapshot<T>(
        &self,
        f: impl FnOnce(&Snapshot) -> Result<T, String>,
    ) -> Result<T, String> {
        // Cloned out so long checks never hold the session lock.
        let snapshot = self
            .inner
            .lock()
            .unwrap()
            .snapshot
            .clone()
            .ok_or("工作区尚未载入")?;
        f(&snapshot)
    }
    fn round_path(snapshot: &Snapshot, name: &str) -> Result<PathBuf, String> {
        snapshot
            .rounds
            .rounds
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.path.clone())
            .ok_or_else(|| format!("没有轮次 {}", clean(name)))
    }

    pub fn round_detail(&self, name: &str) -> Result<dto::RoundDetail, String> {
        self.with_snapshot(|s| {
            dto::round_detail(s, name).ok_or_else(|| format!("没有轮次 {}", clean(name)))
        })
    }

    pub fn preview_scope(&self, ids: &[String]) -> Result<dto::ScopeView, String> {
        self.with_snapshot(|s| {
            let saved = &s.detailed().ok_or("工作区状态不可用")?.saved;
            Ok(dto::scope_view(&organizer::llm::preview_scope(saved, ids)))
        })
    }

    pub fn draft_text(&self, name: &str) -> Result<dto::DraftTextView, String> {
        let generated = self.with_snapshot(|s| {
            Self::round_path(s, name)?;
            Ok(s.rounds
                .rounds
                .iter()
                .any(|r| r.name == name && r.draft.as_ref().is_ok_and(|d| d.generated)))
        })?;
        let draft = workspace::read_draft(&self.root, name).map_err(|e| e.to_string())?;
        Ok(dto::DraftTextView {
            text: draft.text,
            digest: draft.digest,
            generated,
        })
    }

    pub fn check_draft(&self, name: &str, text: &str) -> Result<dto::DraftCheckView, String> {
        if text.len() > workspace::MAX_DRAFT_BYTES {
            return Err("草案超过 16 MiB".into());
        }
        self.with_snapshot(|s| {
            let path = Self::round_path(s, name)?;
            let saved = &s.detailed().ok_or("工作区状态不可用")?.saved;
            Ok(dto::check_view(&check_draft(
                &self.root, saved, &path, text,
            )))
        })
    }

    /// The editor's save (design §6.5), outside the worker but never while a
    /// job runs. A refresh follows.
    pub fn save_draft(&self, name: &str, digest: &str, text: &str) -> Result<ActionResult, String> {
        if let Some(busy) = self.busy() {
            return Err(format!("{busy}，稍后再保存"));
        }
        self.with_snapshot(|s| Self::round_path(s, name).map(drop))?;
        let mut result = ActionResult {
            label: "保存草案".into(),
            command: "GUI 内部操作（workspace::save_draft）".into(),
            kind: "ok".into(),
            message: format!("已保存 drafts/{}，未做规范化", clean(name)),
            error: None,
            hint: None,
            go_recovery: false,
            outcome: None,
        };
        match workspace::save_draft(&self.root, name, digest, text) {
            Ok(digest) => {
                result.outcome = Some(OutcomeView::Saved {
                    name: clean(name),
                    digest,
                });
                self.set_dirty(false);
                self.refresh();
            }
            Err(e) => {
                let e = e.to_string();
                result.kind = "failed".into();
                result.message = "保存失败".into();
                result.hint = hint(&e).map(Into::into);
                result.error = Some(clean(&e));
            }
        }
        Ok(result)
    }

    /// The equivalent command a confirmation sheet shows, built by the same
    /// `Action::command` as the result; validation errors are returned as-is.
    pub fn command(&self, action: GuiAction) -> Result<String, String> {
        let prefix = self.core.llm_view().env_prefix;
        self.with_snapshot(|s| {
            action::resolve(&self.root, s, action)
                .map(|a| clean(&dto::display_command(&a, &prefix)))
        })
    }

    /// Queues a write action or preview. Its result arrives as events.
    pub fn run(&self, action: GuiAction) -> Result<(), String> {
        if let Some(busy) = self.busy() {
            return Err(format!("{busy}，请等它完成"));
        }
        let mut inner = self.inner.lock().unwrap();
        let snapshot = inner.snapshot.as_ref().ok_or("工作区尚未载入")?;
        let apply = match &action {
            GuiAction::Apply { plan, token } => {
                let (file, _) = action::find_plan(snapshot, plan).ok_or("没有这个计划")?;
                Some((plan.clone(), file.plan.plan_id.clone(), token.clone()))
            }
            _ => None,
        };
        let kind = match &action {
            GuiAction::Chat { .. } => Kind::Chat,
            GuiAction::Apply { .. } => Kind::Apply,
            _ => Kind::Other,
        };
        let resolved = action::resolve(&self.root, snapshot, action)?;
        if let Some((plan, plan_id, token)) = apply {
            inner.tokens.consume(&plan, &plan_id, &token)?;
            // What was previewed is what runs: the file must still hold that plan.
            if let Action::Apply { plan: path, .. } = &resolved {
                let on_disk = organizer::output::read_text(path)
                    .ok()
                    .and_then(|t| {
                        serde_json::from_str::<organizer::migration::MigrationPlan>(&t).ok()
                    })
                    .map(|p| p.plan_id);
                if on_disk.as_deref() != Some(plan_id.as_str()) {
                    return Err("计划文件在预览后被替换：请重新扫描并预览".into());
                }
            }
        }
        self.jobs
            .send(Job::Run(resolved))
            .map_err(|_| "后台任务已停止".to_owned())?;
        inner.queue.push_back(kind);
        if kind == Kind::Apply {
            inner.caffeinate = Some(Caffeinate::start());
        }
        Ok(())
    }

    /// Resolves a reveal target to a path inside the root (design §8.2 item 4).
    pub fn reveal_path(&self, target: &RevealTarget) -> Result<PathBuf, String> {
        let meta = self.root.join(".organizer");
        let path = self.with_snapshot(|s| {
            Ok(match target {
                RevealTarget::Root => self.root.clone(),
                RevealTarget::Meta => meta.clone(),
                RevealTarget::Node { id } => {
                    let rel = s.baseline.paths.get(id).ok_or("没有这个节点")?;
                    self.root.join(rel)
                }
                RevealTarget::File { selector } => {
                    if !action::pending_selectors(s).contains(selector) {
                        return Err("不是待处理文件".into());
                    }
                    meta.join(selector)
                }
                RevealTarget::Step { run, path } => {
                    let entry = s
                        .recoveries
                        .iter()
                        .find(|r| &r.run.run_id == run)
                        .ok_or("没有这个运行")?;
                    let report = entry.report.as_ref().map_err(|e| e.clone())?;
                    let known = report.steps.iter().any(|st| match &st.operation {
                        organizer::migration::Operation::Mkdir { path: p, .. } => p == path,
                        organizer::migration::Operation::MoveFile { from, to, .. }
                        | organizer::migration::Operation::MoveDirectory { from, to, .. } => {
                            from == path || to == path
                        }
                    });
                    if !known {
                        return Err("不是该运行的路径".into());
                    }
                    self.root.join(path)
                }
            })
        })?;
        if !path.starts_with(&self.root)
            || path
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return Err("路径不在工作区内".into());
        }
        if path.symlink_metadata().is_err() {
            return Err("磁盘上不存在这个路径".into());
        }
        Ok(path)
    }

    pub fn draft_path(&self, name: &str) -> Result<PathBuf, String> {
        self.with_snapshot(|s| Self::round_path(s, name))
    }
}

#[derive(Clone, Debug, serde::Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum RevealTarget {
    Root,
    Meta,
    Node { id: String },
    File { selector: String },
    Step { run: String, path: String },
}
