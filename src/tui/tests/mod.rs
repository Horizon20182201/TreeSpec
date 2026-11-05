//! TUI tests: argv parity with the real CLI parser, and an inline harness that
//! drives the App against real temporary roots (no terminal, no threads).

mod flows;
mod parity;
mod recovery;

use super::{
    app::App,
    llm_info::LlmInfo,
    worker::{self, Context},
};
use organizer::{
    llm::provider::{LlmError, LlmProvider, LlmRequest},
    tree::{parse_normalized_tree, relative_paths},
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

pub struct Harness {
    pub app: App,
    ctx: Context,
}

fn no_llm() -> LlmInfo {
    LlmInfo {
        available: false,
        label: "未配置".into(),
        host: None,
        remote: false,
        no_content: false,
        reason: Some("LLM is not configured".into()),
    }
}

impl Harness {
    pub fn new(root: &Path) -> Self {
        Self::build(
            root,
            no_llm(),
            Box::new(|| Err("no provider in tests".into())),
        )
    }

    /// A harness whose `chat` calls pop replies from `replies`.
    pub fn with_llm(root: &Path, replies: Vec<Value>) -> Self {
        let queue = Arc::new(Mutex::new(VecDeque::from(replies)));
        let info = LlmInfo {
            available: true,
            label: "mock@127.0.0.1（本地）".into(),
            host: Some("127.0.0.1".into()),
            remote: false,
            no_content: false,
            reason: None,
        };
        Self::build(
            root,
            info,
            Box::new(move || Ok(Box::new(Mock(queue.clone())) as Box<dyn LlmProvider>)),
        )
    }

    fn build(root: &Path, llm: LlmInfo, provider: Box<worker::ProviderFactory>) -> Self {
        let root = root.canonicalize().unwrap();
        let mut harness = Self {
            app: App::new(root.clone(), llm),
            ctx: Context { root, provider },
        };
        harness.pump();
        harness
    }

    /// Runs queued jobs inline, in order, like the worker thread would.
    pub fn pump(&mut self) {
        while !self.app.outbox.is_empty() {
            for job in std::mem::take(&mut self.app.outbox) {
                let mut events = vec![];
                worker::execute(&self.ctx, job, &mut |event| events.push(event));
                for event in events {
                    self.app.handle(event);
                }
            }
        }
    }

    pub fn key(&mut self, code: KeyCode) -> &mut Self {
        self.app.key(KeyEvent::new(code, KeyModifiers::NONE));
        self.pump();
        self
    }

    pub fn keys(&mut self, text: &str) -> &mut Self {
        for c in text.chars() {
            self.key(KeyCode::Char(c));
        }
        self
    }

    pub fn screen(&mut self, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| super::view::draw(frame, &mut self.app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            let mut skip = 0;
            for x in 0..width {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let symbol = buffer[(x, y)].symbol();
                skip = unicode_width::UnicodeWidthStr::width(symbol).saturating_sub(1);
                out.push_str(symbol);
            }
            out.push('\n');
        }
        out
    }
}

struct Mock(Arc<Mutex<VecDeque<Value>>>);

impl LlmProvider for Mock {
    fn complete_json(&self, _: &LlmRequest) -> Result<Value, LlmError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected LLM call"))
    }
}

/// A managed root with `Inbox/note.txt` and `Papers/a.txt`.
pub fn managed() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for path in ["Inbox/note.txt", "Papers/a.txt"] {
        let path = temp.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "user bytes").unwrap();
    }
    organizer::sync::init(temp.path()).unwrap();
    temp
}

/// Baseline path → ID.
pub fn ids(root: &Path) -> HashMap<String, String> {
    let tree =
        parse_normalized_tree(&fs::read_to_string(root.join(".organizer/structure.tree")).unwrap())
            .unwrap();
    relative_paths(&tree)
        .unwrap()
        .into_iter()
        .map(|(id, path)| (path, id))
        .collect()
}
