//! Dialogs. Key handling returns a [`Reply`]; the App performs what follows.
//! Confirmation dialogs focus “取消” first, so repeated Enter never confirms.

use crate::tui::{action::Action, llm_info::LlmInfo};
use organizer::llm::{MAX_PROMPT_BYTES, ScopePreview};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::path::PathBuf;

pub enum Dialog {
    Confirm(Confirm),
    Apply(ApplyConfirm),
    Chat(Box<ChatForm>),
    Input(Input),
    Choice(Choice),
    Message(Message),
    Help(usize),
    Log(usize),
}

pub enum Reply {
    Stay,
    Close,
    Then(Next),
}

pub enum Next {
    Run(Action),
    Open(Box<Dialog>),
    Edit(PathBuf),
    Chat,
    AskDraftName,
    Submit(Purpose, String),
}

#[derive(Clone)]
pub enum Purpose {
    PlanName { to: PathBuf, ack: bool },
    ScriptName { plan: PathBuf },
    DraftName,
    Search,
}

pub struct Confirm {
    pub title: String,
    pub lines: Vec<String>,
    pub command: Option<String>,
    /// A statement the user must tick before confirming.
    pub check: Option<(String, bool)>,
    /// 0 = check box, 1 = 取消, 2 = 确认.
    pub focus: u8,
    pub next: Option<Next>,
    pub note: Option<String>,
}

impl Confirm {
    pub fn new(
        title: impl Into<String>,
        lines: Vec<String>,
        command: Option<String>,
        next: Next,
    ) -> Self {
        Self {
            title: title.into(),
            lines,
            command,
            check: None,
            focus: 1,
            next: Some(next),
            note: None,
        }
    }
    pub fn checked(mut self, label: impl Into<String>) -> Self {
        self.check = Some((label.into(), false));
        self
    }
    fn key(&mut self, key: KeyEvent) -> Reply {
        let first = if self.check.is_some() { 0 } else { 1 };
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => Reply::Close,
            KeyCode::Tab | KeyCode::Down | KeyCode::Right => {
                self.focus = if self.focus >= 2 {
                    first
                } else {
                    self.focus + 1
                };
                Reply::Stay
            }
            KeyCode::BackTab | KeyCode::Up | KeyCode::Left => {
                self.focus = if self.focus <= first {
                    2
                } else {
                    self.focus - 1
                };
                Reply::Stay
            }
            KeyCode::Char(' ') => {
                if let Some((_, value)) = &mut self.check {
                    *value = !*value;
                }
                Reply::Stay
            }
            KeyCode::Char('y') => self.accept(),
            KeyCode::Enter => match self.focus {
                0 => {
                    if let Some((_, value)) = &mut self.check {
                        *value = !*value;
                    }
                    Reply::Stay
                }
                1 => Reply::Close,
                _ => self.accept(),
            },
            _ => Reply::Stay,
        }
    }
    fn accept(&mut self) -> Reply {
        if self.check.as_ref().is_some_and(|(_, ticked)| !ticked) {
            self.note = Some("请先勾选确认项（Space）".into());
            return Reply::Stay;
        }
        self.next.take().map_or(Reply::Close, Reply::Then)
    }
}

/// Strong confirmation before files move: the user must type `apply`.
pub struct ApplyConfirm {
    pub lines: Vec<String>,
    pub command: String,
    pub typed: String,
    pub action: Option<Action>,
    pub note: Option<String>,
}

pub struct Input {
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub purpose: Purpose,
    pub error: Option<String>,
}

pub struct Choice {
    pub focus: usize,
    /// Why the LLM option is unavailable, if it is.
    pub llm_blocked: Option<String>,
}

pub struct Message {
    pub title: String,
    pub lines: Vec<String>,
    pub error: bool,
    pub scroll: usize,
}

pub struct ChatForm {
    pub root: PathBuf,
    pub drafts: PathBuf,
    pub prompt: String,
    pub scope_ids: Vec<String>,
    pub scope_label: String,
    pub classify: bool,
    pub read_content: bool,
    pub out: String,
    /// 0 请求, 1 重新分类, 2 文本片段, 3 输出, 4 取消, 5 发送.
    pub focus: usize,
    pub preview: Result<ScopePreview, String>,
    pub llm: LlmInfo,
    pub error: Option<String>,
}

impl ApplyConfirm {
    fn key(&mut self, key: KeyEvent) -> Reply {
        match key.code {
            KeyCode::Esc => Reply::Close,
            KeyCode::Backspace => {
                self.typed.pop();
                Reply::Stay
            }
            KeyCode::Char(c) if self.typed.len() < 16 => {
                self.typed.push(c);
                Reply::Stay
            }
            KeyCode::Enter if self.typed == "apply" => self
                .action
                .take()
                .map_or(Reply::Close, |a| Reply::Then(Next::Run(a))),
            KeyCode::Enter => {
                self.note = Some("请输入 apply 确认，或按 Esc 取消".into());
                Reply::Stay
            }
            _ => Reply::Stay,
        }
    }
}

impl ChatForm {
    fn key(&mut self, key: KeyEvent) -> Reply {
        let skip = |f: usize| f == 2 && self.llm.no_content;
        match key.code {
            KeyCode::Esc => return Reply::Close,
            KeyCode::Tab | KeyCode::Down => {
                self.focus = (self.focus + 1) % 6;
                if skip(self.focus) {
                    self.focus += 1;
                }
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.focus = (self.focus + 5) % 6;
                if skip(self.focus) {
                    self.focus -= 1;
                }
            }
            KeyCode::Enter => match self.focus {
                4 => return Reply::Close,
                5 => return self.submit(),
                _ => self.focus += 1,
            },
            KeyCode::Char(' ') if self.focus == 1 => {
                self.classify = !self.classify;
                self.read_content &= self.classify;
            }
            KeyCode::Char(' ') if self.focus == 2 => {
                self.read_content = !self.read_content;
                self.classify |= self.read_content;
            }
            KeyCode::Backspace => {
                if let Some(field) = self.field() {
                    field.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(field) = self.field() {
                    field.push(c);
                }
            }
            _ => {}
        }
        Reply::Stay
    }
    fn field(&mut self) -> Option<&mut String> {
        match self.focus {
            0 => Some(&mut self.prompt),
            3 => Some(&mut self.out),
            _ => None,
        }
    }
    fn submit(&mut self) -> Reply {
        let prompt = self.prompt.trim();
        let out = self.out.trim();
        let problem = if prompt.is_empty() || self.prompt.len() > MAX_PROMPT_BYTES {
            Some(format!("请求不能为空，且不超过 {MAX_PROMPT_BYTES} 字节"))
        } else if let Err(error) = &self.preview {
            Some(format!("范围不可用：{error}"))
        } else {
            super::keys::check_name(&self.drafts, out, ".tree").err()
        };
        if let Some(problem) = problem {
            self.error = Some(problem);
            return Reply::Stay;
        }
        Reply::Then(Next::Run(Action::Chat {
            root: self.root.clone(),
            prompt: prompt.to_owned(),
            scope_ids: self.scope_ids.clone(),
            classify: self.classify,
            read_content: self.read_content,
            out: self.drafts.join(out),
        }))
    }
}

impl Dialog {
    pub fn key(&mut self, key: KeyEvent) -> Reply {
        match self {
            Self::Confirm(d) => d.key(key),
            Self::Apply(d) => d.key(key),
            Self::Chat(d) => d.key(key),
            Self::Input(d) => match key.code {
                KeyCode::Esc => Reply::Close,
                KeyCode::Enter => {
                    Reply::Then(Next::Submit(d.purpose.clone(), d.value.trim().to_owned()))
                }
                KeyCode::Backspace => {
                    d.value.pop();
                    Reply::Stay
                }
                KeyCode::Char(c) => {
                    d.value.push(c);
                    Reply::Stay
                }
                _ => Reply::Stay,
            },
            Self::Choice(d) => match key.code {
                KeyCode::Esc => Reply::Close,
                KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k') | KeyCode::Tab => {
                    d.focus = 1 - d.focus;
                    Reply::Stay
                }
                KeyCode::Char('1') => d.pick(0),
                KeyCode::Char('2') => d.pick(1),
                KeyCode::Enter => d.pick(d.focus),
                _ => Reply::Stay,
            },
            Self::Message(Message { scroll, .. }) | Self::Help(scroll) | Self::Log(scroll) => {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        *scroll += 1;
                        Reply::Stay
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        *scroll = scroll.saturating_sub(1);
                        Reply::Stay
                    }
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => Reply::Close,
                    _ => Reply::Stay,
                }
            }
        }
    }
}

impl Choice {
    fn pick(&mut self, option: usize) -> Reply {
        match option {
            0 if self.llm_blocked.is_some() => {
                self.focus = 1;
                Reply::Stay
            }
            0 => Reply::Then(Next::Chat),
            _ => Reply::Then(Next::AskDraftName),
        }
    }
}
