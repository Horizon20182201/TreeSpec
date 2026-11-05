# TreeSpec Organizer macOS GUI 设计

> 状态：第一期已实现（`gui/`），第 15 节的决定全部按推荐选项确认（2026-11-06）。实现与本文的差异和尚未在真实 macOS 上验证的部分见第 17 节；构建与开发见 [gui/README.md](../gui/README.md)。最初的界面示例见 [GUI_MOCKUP.html](GUI_MOCKUP.html)。

## 1. 目标与边界

GUI 是 TUI 之外的第二个交互前端，面向 macOS 上不习惯终端的用户。它覆盖与 TUI 第一期相同的流程（初始化 → 草案 → 审核 → 计划 → 预览 → 执行 → 归档，以及同步、历史和恢复），用窗口、侧栏、表格和工作表（sheet）代替页面键位，并补上终端里做不到或做得不顺的部分：内置的草案编辑器、Finder 集成、系统文件夹授权、用钥匙串保存 API Key。

术语与 [TUI 设计](TUI_DESIGN.md) 相同：“基线”指 `.organizer/structure.tree`，“草案”指 `drafts/` 下的目标 Tree，“计划”指 `plans/` 下的迁移计划 JSON，“轮次”见 TUI 设计 3.2。

TUI 设计第 1 节的五条约束全部沿用：

1. **不新增文件操作能力。** 用户文件只由 `apply_plan` 移动。
2. **安全判断以库为准。** 按钮可用性只是提示，真正的检查在库内执行。
3. **按风险分级确认**（第 8 节）。
4. **每个写动作显示等价命令。**
5. **不监听文件系统。** 只在打开、手动刷新和写操作后扫描。

GUI 另加三条：

6. **界面层不做判断。** 工作区状态、轮次阶段、动作可用性和下一步建议都由 Rust 侧的共享前端层计算（3.2），TUI 和 GUI 用同一份结果。Web 前端只负责显示和收集输入。
7. **不信任 WebView。** 前端拿不到文件系统、Shell 或任意路径的能力，只能调用一组白名单命令；命令参数是轮次名、节点 ID 这类标识，路径由 Rust 侧解析（8.2）。
8. **只做 macOS。** 只在 macOS 上构建、签名和测试。库保持跨平台，日后移植到 Linux 只需改界面层。

不在范围内：TUI 不做的这里也不做（删除、撤销、续跑、跨卷移动、多轮对话）；另外不做云同步、多个根目录的合并视图、菜单栏常驻、后台定时整理和 Mac App Store 上架（13.3）。

## 2. 技术选型

### 2.1 候选方案

| 方案 | 界面层 | 与 Rust 库的连接 | 优点 | 缺点 |
| --- | --- | --- | --- | --- |
| A. Tauri 2 + Svelte 5 | WKWebView 中的 TypeScript | 进程内调用库，`serde` JSON 传给前端 | 后端全是 Rust，可直接复用 `src/tui` 的状态模型和 worker；库的报告类型大多已派生 `Serialize`；前端能在浏览器里开发和测试；本示例 HTML 的样式可以延续到真实界面 | 外观要手工贴近 macOS；WebView 带来注入风险，需要 CSP 和命令白名单；macOS 上没有官方 WebDriver |
| B. SwiftUI + Rust 静态库（UniFFI） | Swift | UniFFI 生成 Swift 绑定 | 最原生：大纲、工作表、菜单、Quick Look、输入法、VoiceOver 都是系统控件；没有 WebView 注入面 | 多一门语言和一个 Xcode 工程；跨边界的类型要逐个写成 UniFFI 记录，或在 Swift 里重新解析 JSON；只能在 Mac 上开发和测试 |
| C. 纯 Rust GUI（egui、iced、Slint） | Rust | 直接调用 | 单一语言、单一构建 | 中文输入法、文本编辑、无障碍和 macOS 外观都弱于 A、B；编辑器要自己做 |

### 2.2 结论

选择 **方案 A：Tauri 2 + Svelte 5 + TypeScript**，编辑器用 **CodeMirror 6**。理由如下。

这个项目的复杂度在 Rust 侧：状态模型、轮次配对、确认规则。界面层主要是表格、树和表单。方案 A 让这些逻辑留在 Rust，前端可以很薄；方案 B 要么把类型逐个映射成 UniFFI 记录，要么在 Swift 里解析 JSON，状态模型容易出现第二份实现。

项目以 Rust 为主。方案 A 的后端、测试和 CI 与现有仓库一致，只多一层 TypeScript；前端还可以在 Linux 上用模拟 IPC 测试。方案 B 要同时维护 Swift 和 Xcode 工程，全部开发都要在 Mac 上进行。

方案 A 的主要风险是 WebView 注入。第 8.2 节的措施把它限制为“只能触发库本来就允许、并且仍要经过全部检查的动作”。

选 Svelte 而不是 React：组件基本就是 HTML 加少量脚本，编译后没有运行时虚拟 DOM，文本插值默认转义，对以 Rust 为主的开发者更好读。代价是生态比 React 小，但本项目用到的 CodeMirror 和虚拟列表都与框架无关。

如果以后更看重原生观感和输入体验，可以换成方案 B：第 3.2 节的共享前端层与界面技术无关，届时只替换 GUI 后端和前端。

### 2.3 版本与依赖基线

| 组件 | 要求 | 说明 |
| --- | --- | --- |
| Tauri | 2.12 或更新 | 2.11.1 修复了一个安全问题：没有 `AppManifest` 时，自定义命令会跳过 ACL 检查，任何来源都能调用。本项目在 `build.rs` 中显式声明全部命令（8.2）。当前最新为 2.12.3 |
| Svelte | 5.x（runes） | 截至 2026 年 10 月为 5.57。不用 SvelteKit，单页应用用 Vite 构建 |
| CodeMirror | 6 | 只引入 `@codemirror/state`、`view`、`commands`、`language`；TreeSpec 高亮自己写一个小的 `StreamLanguage` |
| Rust 侧新增依赖 | `tauri`、`tauri-build`、`ts-rs`、`security-framework` | `ts-rs` 从 DTO 生成 TypeScript 类型；`security-framework` 读写钥匙串 |
| Tauri 插件 | 只用 `dialog` | 只在 Rust 侧调用，不给前端开放权限。“在 Finder 中显示”直接运行 `/usr/bin/open -R`，窗口大小记在设置文件中，所以不需要 `opener` 和 `window-state`（第 17 节）。不用 `fs`、`shell`、`http`、`store`、`updater` |
| macOS | 13 或更新（待确认，见第 15 节） | Tauri 2 本身支持 10.15+，库要求 10.12+；设为 13 是为了 WebKit 版本较新，也减少测试组合 |

## 3. 架构

### 3.1 分层

```text
┌──────────────────────────┐   ┌───────────────────────┐
│ GUI 前端 (Svelte, TS)    │   │ TUI 视图与按键        │
│ WKWebView，只做显示和输入 │   │ (ratatui)             │
└────────────┬─────────────┘   └──────────┬────────────┘
             │ invoke / Channel（JSON）    │
┌────────────▼─────────────┐              │
│ GUI 后端 (Tauri, Rust)   │              │
│ 会话、命令白名单、DTO、   │              │
│ macOS 集成               │              │
└────────────┬─────────────┘              │
┌────────────▼─────────────────────────────▼────────────┐
│ organizer::frontend（新）                             │
│ 快照、工作区状态、轮次、审核标注、Action、worker、    │
│ 下一步、LLM 配置摘要、文本清理与中文提示               │
└────────────┬──────────────────────────────────────────┘
┌────────────▼──────────────────────────────────────────┐
│ organizer 库：sync / migration / llm / workspace / tree │
└───────────────────────────────────────────────────────┘
```

### 3.2 共享前端层 `organizer::frontend`

TUI 第一期的 `state.rs`、`rounds.rs`、`review.rs`、`load.rs`、`action.rs`、`worker.rs`、`llm_info.rs` 和 `text.rs` 中的大部分代码与 ratatui 无关。它们目前在二进制一侧，GUI 后端无法依赖。如果 GUI 再写一遍，两个前端迟早会对同一个工作区得出不同的结论。

所以 GUI 的第一步是把这些模块移到库里的 `organizer::frontend`，TUI 改为调用它，行为和测试保持不变：

| 现在的位置 | 移到 | 调整 |
| --- | --- | --- |
| `tui/state.rs` | `frontend::state` | 原样移动；`Workspace` 派生 `Serialize` |
| `tui/rounds.rs`、`review.rs`、`tree_index.rs` | `frontend::rounds`、`review`、`tree_index` | 原样移动；折叠状态留在 TUI |
| `tui/load.rs` | `frontend::load` | 原样移动 |
| `tui/action.rs` | `frontend::action` | 原样移动 |
| `tui/worker.rs` | `frontend::worker` | 依赖的 `plan_documents`、`output` 一并移入库（第 11 节） |
| `tui/llm_info.rs` | `frontend::llm_info` | 增加显式配置入口（9.3） |
| `tui/text.rs` | `frontend::text` | 只移动 `clean` 和 `hint`；终端宽度与截断函数留在 TUI |
| `tui/view/mod.rs` 的 `next_step` | `frontend::next` | 改为返回结构化的 `NextStep` 枚举，不再返回带键位的句子 |

`NextStep` 例如 `ArchiveThenSync { rounds }`、`Sync`、`RetryGit`、`OpenRecovery`、`Plan { round }`、`Apply { round }`。TUI 把它渲染成“先在整理页按 Z 归档……”，GUI 渲染成带按钮的横幅。中文状态名、阶段名和错误提示仍放在 Rust 侧，只有一份。

`frontend` 不依赖 ratatui，不需要 feature 开关，`cargo build --no-default-features` 同样包含它。检查命令行一致性的 `parity` 测试依赖 `Cli::try_parse_from`，仍留在二进制一侧；因为 GUI 用的也是同一个 `Action`，这些测试同时覆盖了 GUI。

### 3.3 GUI 后端

`gui/src-tauri` 是一个独立的 crate，以路径依赖引用根目录的 `organizer` 库，并关闭默认 feature（不带 ratatui）。它有自己的 `[workspace]` 和 `Cargo.lock`，根目录的 `cargo build`、`cargo test --locked` 和 CLI 安装都不受影响。

后端只做五件事：

1. **会话**：每个窗口对应一个会话，包含一个 worker 线程和最近一次快照。同一个根目录只开一个窗口，再次打开时聚焦已有窗口。
2. **命令白名单**：第 3.4 节的命令，参数只用名称和 ID。
3. **DTO**：把 `Snapshot` 转成面向显示的 `ViewModel`。文本先经过 `clean`；状态带代码和中文名；每个动作带“是否可用”和“不可用原因”。类型用 `ts-rs` 导出到 `gui/src/ipc/types.ts`，CI 检查生成文件是否最新。
4. **macOS 集成**：文件夹选择框、在 Finder 中显示、钥匙串、防止休眠、退出拦截（第 9、10 节）。
5. **设置**：最近打开的工作区、LLM 配置、外部编辑器，以 JSON 保存在 `~/Library/Application Support/<Bundle ID>/settings.json`；API Key 只存在钥匙串。

### 3.4 IPC 命令

| 命令 | 作用 | 对应 |
| --- | --- | --- |
| `pick_folder()` | 在 Rust 侧弹出系统“打开”面板，返回所选目录 | 无 |
| `open_workspace(path)` | 解析根目录（与 CLI 相同）；已打开则聚焦对应窗口，否则新建窗口和会话 | `resolve_root` |
| `snapshot()` | 返回当前会话最近一次的 `ViewModel` | 无 |
| `refresh()` | 排队一次完整刷新 | `Job::Refresh` |
| `round_detail(name)` | 返回该轮次的标注目标树、计划操作和报告 | `review::marks` 等 |
| `preview_scope(ids)` | 离线计算 LLM 范围 | `llm::preview_scope` |
| `draft_text(name)` | 读取草案原文及其 SHA-256 | `workspace::read_draft`（新） |
| `check_draft(name, text)` | 对未保存的文本做与载入时相同的解析和审核检查，不写盘 | `frontend::check_draft`（新） |
| `run(action)` | 执行一个写动作或预览 | `Job::Run(Action)` |
| `reveal(target)` | 在 Finder 中显示节点、草案、计划或恢复步骤的路径 | `/usr/bin/open -R` |
| `get_settings()`、`set_settings(s)`、`set_api_key(k)` | 设置；API Key 只能写入或清除，不能读出 | 无 |

`run` 的参数是带标签的枚举，与 `Action` 一一对应，但所有路径都换成名称：`Plan { round, acknowledge_review, out_name }`、`Apply { plan, token }`、`Archive { selectors }` 等。后端用 `workspace::check_new_name` 和已加载快照中的轮次表把名称解析为 `.organizer` 内的路径，再构造 `Action`。前端传来的名称不在快照里时直接拒绝。

`run` 立即返回任务 ID。进度、结果和随后的刷新通过窗口专属的 Tauri `Channel` 按顺序送回，事件与 `worker::Event` 相同：`Progress(k, n)` → `Done(action, result)` → `Snapshot` → `Finished`。任务进行中再次调用 `run` 返回“忙”，前端此时本来就已把写操作置灰。

### 3.5 线程、锁与刷新

沿用 TUI 设计 9.2：每个窗口一个 worker 线程串行执行任务；任务运行时写操作置灰，浏览仍可用；空闲时不持有元数据锁，每次库调用自行加锁和释放。另一个 `organizer` 进程（CLI、TUI 或另一个 GUI 窗口）占用锁时显示“元数据正被其他进程使用”，保留上次数据并标为可能过期。

完整刷新（扫描磁盘）只在三个时机发生：打开窗口、`⌘R`、每次写动作之后。窗口回到前台时不扫描，只做两件事：

- 工具栏的扫描时间超过 1 分钟后显示为“可能已过期”，刷新按钮高亮。
- 编辑器中打开的草案若在磁盘上被修改过（比较 mtime 和大小），显示“文件已在外部修改”，提供“重新载入”。

设置中可以打开“窗口激活时自动重新扫描”，默认关闭。界面数据随时可能过期，正确性仍由库在每个写动作前重新检查保证。

## 4. 窗口与导航

### 4.1 窗口

**欢迎窗口**在没有打开工作区时显示：“打开文件夹…”按钮、最近打开的工作区列表（路径、上次状态、打开时间）和拖放区域。把文件夹拖到窗口或 Dock 图标上，等同于打开该文件夹。

**工作区窗口**一个窗口对应一个管理根。标题是根目录名，副标题是完整路径。可以同时打开多个不同的根，根目录在窗口内固定。

打开的目录本身没有 `.organizer`、但某个上级目录有时，GUI 提示“该文件夹属于工作区 …，打开该工作区？”，只提供“打开上级工作区”和“取消”，不提供在内部再初始化：扫描会跳过任意层级的 `.organizer`，嵌套初始化会让两个根同时管理同一批文件。确实需要时仍可用 CLI 完成。

窗口最小 960×600，默认 1200×780，按根目录记住大小、侧栏宽度和检查器开关。

### 4.2 主窗口布局

```text
┌──────────┬───────────────────────────────────────────────────────────────────────┐
│ o o o    │ Workspace  ~/Documents/Workspace  [已同步 10:42]   刷新 ＋ 日志 检查器│
│ 工作区   ├────────────────────────────────────────────────────┬──────────────────┤
│ 概览     │ [下一步：papers.tree 待计划  确认并生成计划…]      │ 检查器           │
│ 结构     │                                                    │ 选中项的详情：   │
│ 整理  3  │                                                    │ 路径、ID、       │
│ 历史     │ 当前页面内容                                       │ LLM 理由等       │
│ 恢复     │                                                    │                  │
│          │                                                    │                  │
│ LLM      ├────────────────────────────────────────────────────┤                  │
│ gpt-x@…  │ 活动日志（可折叠）                                 │                  │
└──────────┴────────────────────────────────────────────────────┴──────────────────┘
```

侧栏的五项对应 TUI 的五个页面，`⌘1`–`⌘5` 切换。“整理”旁的数字是进行中的轮次数。“恢复”平时置灰；检测到未完成迁移时变红，窗口打开后直接进入恢复页，其他页面的写操作全部禁用，与 TUI 相同。侧栏底部显示 LLM 配置摘要（模型@主机、本地或远程、未配置、已禁用），点击打开设置。

工具栏的状态胶囊显示工作区状态和扫描时间，点击回到概览。右侧依次是刷新、新建草案、活动日志、检查器开关。

内容区顶部的“下一步”横幅由 `NextStep` 渲染，每个页面都显示。横幅上的按钮只会打开对应的确认工作表，不会直接执行写操作。

检查器（`⌥⌘0`）显示当前选中项的详情。活动日志对应 TUI 的命令日志，记录本次会话每个动作的时间、等价命令和结果，可复制，只保存在内存中。

### 4.3 菜单与快捷键

| 菜单 | 项目 | 快捷键 |
| --- | --- | --- |
| TreeSpec Organizer | 关于、设置…、退出 | `⌘,`、`⌘Q` |
| 文件 | 打开文件夹…、最近打开、关闭窗口、在 Finder 中显示工作区 | `⌘O`、—、`⌘W`、`⌥⌘R` |
| 编辑 | 撤销、重做、剪切、拷贝、粘贴、全选、查找 | 系统标准 |
| 显示 | 概览、结构、整理、历史、恢复；显示侧栏、检查器、活动日志；只看变化；重新扫描 | `⌘1`–`⌘5`；`⌃⌘S`、`⌥⌘0`、`⇧⌘L`；—；`⌘R` |
| 整理 | 新建 LLM 草案…、复制当前结构为草案…、保存草案、生成计划…、预览、执行迁移…、导出脚本…、归档本轮…、全部归档… | `⌘N`、`⇧⌘N`、`⌘S`、`⌥⌘P`、`⌥⌘D`、无、无、无、无 |
| 恢复 | 重新检查、补记 Git…、标记已人工处理… | `⌘R`、无、无 |
| 窗口、帮助 | 系统标准；帮助打开 README 与本设计文档 | — |

移动用户文件、批量归档、补记 Git 和标记已处理都不设快捷键，只能从按钮或菜单进入确认工作表，对应 TUI 用大写键降低误触的做法。生成计划和预览用 `⌥⌘P`、`⌥⌘D`，避开 macOS 上 `⌘P`（打印）和 `⌘D`（复制）的惯例。

页面内：Tab / ⇧Tab 在栏之间移动焦点；↑↓ 移动；←→ 折叠、展开；Space 勾选范围；Return 打开详情；Esc 关闭工作表或返回；`⌘F` 聚焦当前列表的搜索框。

## 5. 状态模型的呈现

工作区状态（TUI 设计 3.1 的 8 种，加上“无法读取”）和轮次阶段（3.2 的 8 个阶段，加上“需确认审核”标记）完全沿用 TUI 的定义和判断顺序，由 `frontend::state` 和 `frontend::rounds` 计算。“磁盘有变化时禁止新建草案”“待处理草案不算冲突”“预览通过只在会话内存有效”这几条规则也不变。GUI 只决定怎么显示：

| 状态 | 工具栏胶囊 | 概览状态卡的主按钮 | 下一步横幅 |
| --- | --- | --- | --- |
| 未初始化 | 灰 · 未初始化 | 初始化… | 初始化这个文件夹 |
| 元数据中断 | 红 · 需人工检查 | 在 Finder 中显示 .organizer | 先备份 .organizer，再人工检查 |
| 迁移未完成 | 红 · 迁移未完成 | 前往恢复 | 在恢复页检查 |
| 基线冲突 | 红 · 基线冲突 | 另存为草案… | 另存当前 structure.tree，再人工恢复基线 |
| 扫描不完整 | 橙 · 扫描不完整 | 重新扫描 | 修正出错路径的权限后重新扫描 |
| Git 待补记 | 橙 · Git 待补记 | 同步… | 重试 Git 记录（有轮次时先归档） |
| 磁盘有变化 | 橙 · 有 n 项变化 | 同步…（有轮次时为“全部归档…”） | 先归档轮次，再同步 |
| 已同步 | 绿 · 已同步 | 新建草案… | 由轮次计算，例如“papers.tree 可执行” |
| 无法读取 | 红 · 无法读取 | 重新扫描 | 显示错误原文和中文提示 |

颜色只是辅助，胶囊、徽标和阶段始终同时带文字或符号。按钮不可用时置灰，悬停或用 VoiceOver 读出不可用原因，原因文字来自共享层，与 TUI 的“受阻：磁盘有变化”等标签一致。

## 6. 页面

各页面的视觉效果见 [GUI_MOCKUP.html](GUI_MOCKUP.html)，用页面顶部的演示栏切换场景。

### 6.1 初始化

打开没有 `.organizer` 的文件夹时显示。页面列出：将要管理的绝对路径；初始化会扫描目录，创建 `.organizer/`（结构文本、SQLite 索引、只记录结构的 Git 仓库），不移动任何文件。路径是 `$HOME`、卷根目录或 `/` 时额外提示扫描可能很慢，并建议选择更具体的文件夹。

首次访问桌面、文稿、下载、iCloud 云盘或外置卷时，macOS 可能要求授权，页面提前说明（9.1）。“初始化…”打开普通确认工作表，显示等价命令；确认后显示扫描进度，完成后进入概览。

### 6.2 概览

顶部是状态卡：状态名、一句解释、扫描时间和主按钮（第 5 节）。

左栏“磁盘变化（相对基线）”有五个计数：新增、缺失、移动、仅内容变化、未知身份。点击某一类展开为表格（路径；移动显示旧路径 → 新路径；未知身份显示候选 ID 和原因），行的右键菜单有“在 Finder 中显示”。

右栏是“进行中的整理轮次”（名称和阶段，点击跳到整理页）和“扫描边界”（受保护的嵌套 Git 项目、跳过的符号链接或特殊文件、被排除的 `.git` 与嵌套 `.organizer`，图标与 TUI 的 `[P]`、`[S]`、`[E]` 对应）。

特殊状态下状态卡替换为对应内容：

- **元数据中断**：只读显示 `pending.json` 内容、SQLite 恢复文件列表和 `sync.log` 末尾，说明“先备份 `.organizer`，再人工检查”。按钮只有“在 Finder 中显示 .organizer”和“拷贝诊断信息”。
- **基线冲突**：显示冲突原文。“另存为草案…”等价于复制当前 `structure.tree` 到 `drafts/`，是这一状态下唯一的写操作。
- **扫描不完整**：列出出错路径和原因。原因是权限不足时提供“重新授权…”（9.1）。

### 6.3 结构

基线的只读大纲，三列：名称（带展开箭头）、说明、标记。目录可折叠，长列表只渲染可见行。

每个目录行前有复选框，用于选择 LLM 范围（`--scope`）。敏感名称（如 `.env`、私钥）和受保护的嵌套 Git 项目显示锁图标，复选框禁用，悬停说明原因。顶部搜索框按名称过滤，保留祖先以便定位。

选中节点时，检查器显示名称、类型、相对路径、ID（可拷贝）、子项数量，以及“LLM：可发送名称与路径”或“LLM：已排除（原因）”。

底部范围栏显示“已选 n 个目录”，并实时调用 `preview_scope`，显示“将发送 k 个节点的名称与路径，m 个已排除”。超过 512 个节点时显示错误，“以所选范围新建 LLM 草案…”按钮置灰。右键菜单有“在 Finder 中显示”和“拷贝相对路径”。

### 6.4 整理

三栏：轮次列表、轮次详情、检查器。

**轮次列表**按 TUI 的分组：轮次（名称、阶段徽标、“需确认审核”徽标、来源 LLM 或手工）、未关联计划、孤立报告、其他文件。列表底部有“＋”菜单（新建 LLM 草案…、复制当前结构为草案…）和“全部归档…”。

**轮次详情**顶部是步骤条“草案 → 审核 → 计划 → 执行 → 归档”，下面是分段控件“审核 | 编辑 | 计划 | 报告”，不可用的段置灰（例如手工草案没有报告）。

- **审核**：带标注的目标树，徽标与 TUI 相同并带文字：`+` 新目录、`~` 改名、`>` 移到新父目录、`^` 随上级移动、`#` 说明变化、`?` 待审核、`!` 阻断。目标中缺失的旧节点单独列在树下方。工具栏有“只看变化”开关和各类变化的计数。选中节点时检查器显示旧路径、新路径、LLM 的决定（`place`/`keep`/`review`）和理由；理由放在引用框里，标注“模型输出，仅供参考”。草案带“需确认审核”标记时，树上方显示黄色提示条，写明原因（有待审核项，或生成后被编辑过）。
- **编辑**：内置草案编辑器，见 6.5。
- **计划**：计划文件名、`plan sha256:…`、预览状态（“10:45 通过，执行前会重新检查”）；操作表格（序号、类型、源 → 目标）；未解决项；扫描边界。按钮：预览、导出脚本…、执行迁移…。没有配对计划时显示空状态和“生成计划…”按钮；计划有未解决项时不写文件，只在这里列出未解决项，并提供“回到编辑”。
- **报告**：仅 LLM 草案可用。显示编辑数、读取的正文字节数、全部分配（文件、决定、目标目录、理由）和警告。

### 6.5 草案编辑器

TUI 用 `$VISUAL`/`$EDITOR` 编辑草案。GUI 用户通常没有配置这两个变量；用“文本编辑”打开 `.tree` 文件还可能把直引号换成弯引号，破坏 TreeSpec 的 JSON 引号名称。所以 GUI 内置一个纯文本编辑器，只编辑 `drafts/` 下已经存在的草案。

- **编辑**：CodeMirror 6，等宽字体，TreeSpec 语法高亮（名称、目录 `/`、`@ID`、`# 说明` 分色，`@ID` 弱化显示）。Tab 插入两个空格，自动缩进与上一行对齐。关闭拼写检查、智能引号和自动替换。
- **实时检查**：停止输入 300 ms 后调用 `check_draft(name, text)`。后端对文本执行与载入时相同的 `parse_tree_with_base`、`compare_trees` 和 `llm::check_review`，不写盘。解析错误标在对应行号；侧栏显示变化计数，以及“将有 n 个文件缺失”“新增 m 个无来源文件”这类会让计划出现未解决项的问题。
- **审核标记**：LLM 草案首行的 `# organizer-review-v1: …` 设为只读，不能删除或修改。编辑器顶部提示“编辑后生成计划需要确认审核”。
- **保存**（`⌘S`）：调用新的 `workspace::save_draft`（第 11 节）。库在独占锁下检查文件仍是 `drafts/` 内的普通文件，内容摘要等于载入时的摘要，审核标记行未变，然后用临时文件加原子替换写入。摘要不一致时拒绝保存，提供“重新载入”（放弃本地修改）和“另存为新草案…”。保存不做规范化，空行和注释都保留。
- **未保存的修改**：切换轮次、生成计划、关闭窗口前询问“保存 / 不保存 / 取消”。生成计划总是基于磁盘上的文件。
- **外部编辑器**：编辑器工具栏的“用其他应用打开”调用设置中选定的应用（默认不设置）。Rust 侧用 `/usr/bin/open -a 应用 绝对路径` 启动，路径作为单独参数传入，不经过 Shell。

TUI 设计 5.6 推迟的“编辑模式”是结构化的 AST 编辑，保存时会规范化并丢失空行和注释。这里的编辑器只改原文，不涉及这个问题。结构化编辑（拖放节点、在树上新建目录）放在第三期，见第 14 节。

### 6.6 历史

左右两栏。左栏是结构提交（短哈希、提交说明，按 `init`/`reconcile`/`refactor` 显示类型徽标）。右栏是迁移运行记录（`run_id`、`plan_id`、状态、完成步数），选中运行时检查器显示错误原文。第三期增加按提交查看结构并与当前基线比较。

### 6.7 恢复

平时侧栏中置灰。存在未完成运行时，页面顶部是红色状态卡：`run_id`、状态、已确认 k/n、`plan sha256:…`、错误原文（等宽、可拷贝）和中文提示。

下面是步骤表：序号、状态（done/pending）、操作、当前磁盘（在源、在目标、两处都没有）。“当前磁盘”是 GUI 按 `recover` 返回的当前扫描与计划前置条件比对得出的，只用于人工核对，不代表身份确认。行的右键菜单有“在 Finder 中显示源/目标”。有多个未完成运行时，顶部用弹出菜单切换。

表下方固定显示：“只读检查。pending 可能已经落盘但未确认，不要重新执行计划。部分完成的文件步骤需要人工核对；GUI 不提供续跑或回滚。”

按钮：重新检查（`⌘R`）、补记 Git…、标记已人工处理…。可用性沿用 TUI 设计 5.5 的表格，是否真正允许由库判断。

### 6.8 设置（`⌘,`）

- **通用**：外部编辑器（选择应用）、窗口激活时自动重新扫描（默认关）、启动时重新打开上次的工作区（默认开）。
- **LLM**：服务地址（默认 `https://api.openai.com/v1`）、模型、API Key（存入钥匙串，只显示“已保存”，不回显）；三个开关“只允许本地服务”“禁止发送文本片段”“禁用 LLM”。进程环境中设置了 `ORGANIZER_LLM_*` 限制时，对应开关强制勾选并锁定，说明“由环境变量设置”（9.3）。
- **高级**：Git 路径与命令行工具检查结果（9.2）；“拷贝诊断信息”，内容包括应用版本、macOS 版本、Git 状态和 LLM 摘要，不含 API Key 和文件路径。

不提供“测试连接”：它会在用户没有确认发送内容时联网。配置是否有效，在打开“新建 LLM 草案”工作表时离线校验，与 TUI 顶栏相同。

## 7. 主要流程

每个写动作都在工作表里确认。工作表显示将写入或移动的内容和等价命令（可拷贝），确认后由 worker 执行，结果写入活动日志并触发刷新。

### 7.1 首次使用

欢迎窗口 →“打开文件夹…”（系统面板）或拖入文件夹 → 初始化页 →“初始化…”→ 确认 → 扫描进度 → 概览。

### 7.2 同步在 GUI 之外完成的调整

概览页“同步…”打开工作表，按类别汇总将要记录的变化，并说明“缺失只从结构快照移除，不删除磁盘文件，也无法恢复其内容”。

存在未知身份时，工作表逐项列出路径、候选 ID 和原因，并附一个默认不勾选的复选框“将这些记录为旧节点消失 + 新节点出现”。不勾选不能确认，勾选后对应 `sync --accept-unknown`。有进行中的轮次时“同步…”置灰，横幅提示先归档。

### 7.3 用 LLM 生成草案

从整理页“＋”菜单、`⌘N` 或结构页的“以所选范围新建 LLM 草案…”打开工作表：

- **请求**：多行文本框，实时显示“字节数 / 8192”。Return 换行，不会发送。
- **范围**：显示结构页选中的目录，可点击“更改…”回到结构页。
- **允许重新分类已有文件**（`--classify`）、**发送文本片段**（`--read-content`，单文件 4 KiB、整次 64 KiB；被禁止时置灰并说明原因）。
- **输出**：`drafts/` 下的新文件名，默认 `chat_XXXXXXXX.tree`。
- **将发送到**：主机、本地或远程、HTTPS。
- **发送内容**：由 `preview_scope` 离线算出，例如“范围内 41 个节点的名称、相对路径、ID、说明；不含文件正文；3 个敏感或受保护节点已排除”。超过 512 个节点时禁止发送。
- **等价命令**。

“发送”不是默认按钮，Return 不会触发它。发送后工作表切换为等待状态，显示已用时间，说明“一次请求最多调用模型 4 次，每次最长 30 秒；等待期间不能取消”。窗口其他部分仍可浏览。完成后选中新轮次的审核视图；有待审核项时（CLI 退出码 `2`）选中第一个待审核节点。

LLM 未配置或已禁用时，“新建 LLM 草案”置灰，并说明原因和“到设置中配置”的入口；“复制当前结构为草案”始终可用（受第 5 节状态限制）。

### 7.4 手工草案

“复制当前结构为草案…”打开一个只要求文件名的工作表（默认 `new.tree`，已存在时建议 `new-2.tree`），等价于 README 的 `cp -n`。创建后直接打开编辑器。

### 7.5 计划、预览与执行

**生成计划**：编辑器有未保存修改时先询问是否保存。草案带“需确认审核”标记时，工作表写明原因，要求勾选“我已检查完整草案和报告；未分类文件保持原有归属”，并说明“这一步不绕过路径、身份、源文件和冲突检查”。输出文件名默认 `plans/NAME.json`，确认审核后为 `plans/NAME.reviewed.json`，重名追加 `-2`。有未解决项时不写文件，切到计划段列出问题。

**预览**：只读，直接执行，不需要确认。结果显示在计划段，标注“执行前会重新检查”。

**执行迁移**：先自动预览一次。预览通过后打开强确认工作表，内容与 TUI 设计 5.3 的执行确认框相同（根目录、计划、操作数、预览结果、“只创建目录和移动，不删除、不覆盖”、“任一步失败立即停止，已完成的移动不会自动回滚”、等价命令），底部是输入框“输入 apply 确认”。输入完全一致前“执行”按钮不可用。

确认后工作表切换为进度视图：进度条和“第 k / n 步”，没有取消按钮，说明“执行期间请不要改动该目录”。成功后显示 `run_id`、完成的操作数和新的 Git 提交，按钮为“归档本轮…”和“完成”。执行开始后失败时显示错误原文和中文提示，唯一的按钮是“前往恢复”。

### 7.6 归档

“归档本轮…”列出将被移动的每个文件（草案、审核报告、配对的计划和导出脚本）以及目标目录 `.organizer/archive/<时间戳>-<草案名>/`。“全部归档…”同样逐项列出。归档不删除也不覆盖。

### 7.7 恢复

“补记 Git…”使用普通确认工作表。“标记已人工处理…”的工作表要求勾选“我已人工核对每个步骤的源和目标”，并说明之后要先归档这一轮，再同步，以记录磁盘的实际状态。

## 8. 确认与安全规则

### 8.1 确认分级

沿用 TUI 设计第 6 节，GUI 的对应形式如下：

| 风险 | 动作 | GUI 交互 |
| --- | --- | --- |
| 只读 | 扫描、浏览、草案实时检查、预览、历史、恢复检查 | 直接执行 |
| 只新建文件 | `plan`、复制基线为草案、导出脚本 | 只确认文件名的工作表 |
| 改写自己的草案 | 编辑器保存 | `⌘S` 直接保存；摘要不一致时拒绝 |
| 写 `.organizer` 内部 | `init`、`sync`、归档、`recover --finish-git` | 普通确认：列出内容和等价命令 |
| 关闭失败运行 | `recover --mark-resolved` | 普通确认，并要求勾选“已人工核对” |
| 向外部服务发送数据 | `chat` | 隐私确认：目标主机、本地或远程、发送内容和排除数量 |
| 移动用户文件 | `apply --yes` | 强确认：自动预览，再输入 `apply` |

所有确认工作表：

- 默认按钮是“取消”：Return 触发取消，Esc 也是取消。确认按钮必须点击，或用 Tab 移过去再按 Space。这与 macOS 人机界面指南对破坏性操作的建议一致，也满足 TUI 的“连续按 Enter 不会确认写操作”。
- 焦点在单行输入框（文件名、`apply` 确认框）中时，Return 不触发任何按钮，避免输入完 `apply` 后按 Return 误取消或误确认。多行请求框中 Return 换行。
- 中文输入法正在组合输入时（`KeyboardEvent.isComposing`），Return 和 Esc 交给输入法，不触发任何按钮。
- 等价命令用与 `export-script` 相同的单引号转义，由 `Action::command` 生成。

### 8.2 WebView 安全

文件名、说明、LLM 理由和错误原文都是不可信数据，并且都要显示在 WebView 里。防护分五层：

1. **只按文本渲染。** Svelte 的 `{…}` 插值默认转义。禁止 `{@html}`，用 ESLint 规则 `svelte/no-at-html-tags` 在 CI 中拦截。CodeMirror 的内容同样按文本处理。
2. **严格 CSP。** 基础策略为 `default-src 'self'; script-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'`。Tauri 构建时会自动加入 IPC 所需的来源和脚本哈希，最终策略以构建产物为准，在实现时核对。不加载任何远程资源；LLM 请求只由 Rust 发出，不经过 WebView。`style-src` 的 `'unsafe-inline'` 是 Svelte 过渡动画需要的；如果实现时不用这类动画，就去掉它。
3. **最小权限。** `build.rs` 用 `AppManifest` 显式声明全部自定义命令（Tauri 2.11.1 的安全修复与此相关），capability 只允许 3.4 中的命令和事件监听，不开放任何插件的前端权限，不配置 `remote` capability。
4. **前端不传路径。** 只传名称和 ID，Rust 侧校验并拼接（3.4）。`reveal` 接受 `{ kind: "node", id }` 这类目标，而不是路径。
5. **执行令牌。** `Apply` 必须带一个一次性令牌。令牌由同一会话中对同一计划文件（名称和 `plan_id` 都相同）最近一次成功的预览产生，5 分钟后或任何刷新后失效。它保证 GUI 中的执行之前一定有一次针对这份计划的预览。它防不住已被注入的脚本先调用预览再调用执行；这种情况靠前四层阻止，库对计划的全部检查仍然生效，最坏结果也只是执行了一份通过检查的、用户自己的计划。

另外：所有不可信字符串在后端先经过 `frontend::text::clean`，与 TUI 相同（控制字符和双向文本控制符显示为 `\u{…}`）；超长名称在中间省略，悬停显示全名。发布构建关闭开发者工具。

### 8.3 执行期间

- 窗口关闭（`⌘W`、红色按钮）、`⌘Q`、注销和关机在执行期间被拦截，显示警告工作表：“迁移正在进行。现在退出会留下部分完成的迁移，需要人工恢复。”默认按钮是“继续等待”，“仍然退出”标为破坏性；单独关闭窗口不提供“仍然关闭”，因为关闭不会停止迁移。这对应 TUI 的“Ctrl-C 需在 3 秒内连按两次”。`⌘Q` 使用自定义菜单项，走与窗口关闭相同的拦截逻辑。
- 执行期间阻止系统空闲睡眠。实现上以子进程运行 `/usr/bin/caffeinate -i -w <本进程 PID>`，执行结束后结束该子进程。合盖睡眠无法阻止，唤醒后执行会继续。
- 强制退出和断电无法拦截，依靠操作日志：下次打开时进入恢复页。
- 等待 LLM 时可以退出，提示“将放弃这次草案生成”。库总是先写报告再写草案，最坏情况只留下孤立报告。

## 9. macOS 集成

### 9.1 文件夹访问授权

GUI 不启用 App Sandbox（13.3），但仍受 TCC（隐私授权）约束：桌面、文稿、下载、iCloud 云盘、外置卷和网络卷都需要授权。

- 用户在系统“打开”面板中选择文件夹，或把文件夹拖到窗口上，系统视为用户明确表达了意图，直接授予访问，不弹授权框。这种授权在重启应用和系统后仍然有效，但不会出现在“系统设置 › 隐私与安全性 › 文件与文件夹”中。
- 从“最近打开”重新打开时，按路径直接访问。如果授权已失效，系统可能弹出授权框，或者库返回权限错误。
- 扫描遇到 `EPERM`/`EACCES` 时，工作区进入“扫描不完整”。概览页提供“重新授权…”（用系统面板重新选择同一文件夹）和“打开系统设置”（跳到“文件与文件夹”）。
- TCC 授权绑定到应用的签名身份。开发构建的签名每次可能变化，授权可能反复失效；正式版本必须使用固定的 Bundle ID 和同一 Team ID 签名，更新后授权才能保留。
- 不需要也不请求“完全磁盘访问权限”。
- `Info.plist` 填写 `NSDesktopFolderUsageDescription`、`NSDocumentsFolderUsageDescription`、`NSDownloadsFolderUsageDescription`、`NSRemovableVolumesUsageDescription`、`NSNetworkVolumesUsageDescription`，说明“用于扫描和整理你选择的文件夹的结构；默认只读取文件名和元数据，不读取文件内容”。

### 9.2 Git 与命令行工具

从 Finder 启动的应用只继承系统默认的 `PATH`，`git` 解析为 `/usr/bin/git`。它是一个转发程序：没有安装 Xcode 命令行工具时，调用它会弹出系统安装框，命令本身失败。

GUI 在打开工作区前检查 `xcode-select -p` 是否成功。失败时不调用 `git`，显示说明和“安装命令行工具”按钮（运行 `xcode-select --install`，由系统弹出安装框），安装完成后重试。库通过 `PATH` 调用 `git` 的方式不变。

### 9.3 LLM 配置

从 Finder 启动的应用读不到终端里 `export` 的环境变量，所以 GUI 有自己的设置（6.8）：服务地址、模型和三个开关保存在设置文件中；API Key 保存在钥匙串的通用密码项（服务名为 Bundle ID，账户为 `llm-api-key`），只在发起请求前由 Rust 读取，不进入前端、日志、诊断信息或等价命令。

进程环境中若有 `ORGANIZER_LLM_*`（例如管理员用 `launchctl setenv` 设置），三个限制开关取“环境或设置任一为 1 则生效”，界面上锁定为勾选。服务地址、模型和密钥以设置为准；环境中同时存在时在设置页提示。

目前 `DISABLED`、`NO_CONTENT` 的检查写在 `chat_with_provider` 内部，直接读取环境变量。为了让 GUI 设置中的限制同样由库执行，第 11 节增加显式的策略参数。

等价命令中，GUI 把配置写成环境变量前缀，例如 `ORGANIZER_LLM_BASE_URL='…' ORGANIZER_LLM_MODEL='…' organizer chat …`，API Key 显示为 `ORGANIZER_LLM_API_KEY=<钥匙串中的密钥>`，提示用户在终端中自行设置。

### 9.4 Finder 与系统集成

- **在 Finder 中显示**：结构树节点、概览的变化项、轮次文件、恢复步骤的源和目标。Rust 侧用基线中的相对路径拼接，确认结果仍在根目录内，再以单独参数运行 `/usr/bin/open -R 路径`。
- **拖放与“打开方式”**：窗口和 Dock 图标接受文件夹拖放。`Info.plist` 用 `CFBundleDocumentTypes` 声明 `public.folder`，`LSHandlerRank` 设为 `Alternate`，不抢占 Finder 对文件夹的默认处理。
- **最近打开**：第一期只保存在设置文件中，显示在欢迎窗口和“文件 › 最近打开”。接入系统的最近项目列表和 Dock 菜单需要调用 AppKit，放在第二期。
- **第二期**：Quick Look 预览（空格键）、后台任务完成时发送系统通知、有未完成迁移时 Dock 图标显示标记。

### 9.5 外观与无障碍

- 跟随系统的浅色/深色外观和强调色。中文界面用系统字体（`-apple-system`，即苹方），路径和 ID 用 SF Mono。
- 大纲使用 `role="tree"`/`treeitem`，带 `aria-level`、`aria-expanded`、`aria-selected`。徽标都有文字标签，VoiceOver 读作“新目录”“待审核”等，不只依赖颜色。
- 全部功能可以只用键盘完成（4.3）。遵守“减弱动态效果”和“增强对比度”。
- 界面使用中文。库的英文错误原样显示，常见错误另附一行中文提示（TUI 设计第 12 节决定 6，同一份 `hint` 表）。

## 10. 退出与生命周期

| 时机 | 行为 |
| --- | --- |
| 关闭最后一个工作区窗口 | 显示欢迎窗口，应用不退出 |
| 关闭窗口时编辑器有未保存修改 | 询问保存 / 不保存 / 取消 |
| 执行迁移期间关闭窗口、退出、注销 | 8.3 的警告工作表 |
| 等待 LLM 期间关闭窗口或退出 | 提示将放弃这次草案生成，确认后退出 |
| 启动 | 按设置重新打开上次的工作区窗口；每个窗口做一次完整刷新；检测到未完成迁移时直接进入恢复页 |

## 11. 底层接口

TUI 第一期补齐的接口（TUI 设计第 10 节第 1–7 项）GUI 全部复用。GUI 另需以下改动，都放在库里，CLI 和 TUI 也能用：

1. **共享前端层**（3.2）：把 `src/tui` 中与界面无关的部分移到 `organizer::frontend`，`next_step` 改为返回 `NextStep`。纯重构，TUI 行为和现有测试不变。
2. **计划与输出**：`main.rs` 中的 `plan_documents`、`PlanOutcome` 和只新建文件的 `output`（含 `check_directory`）移入库，例如 `migration::plan_documents` 和 `fsops::write_new`。CLI、TUI、GUI 调用同一个函数，审核检查只有一处。
3. **草案读写**（新）：`workspace::read_draft(root, name) -> (text, sha256)` 和 `workspace::save_draft(root, name, expected_sha256, text)`。保存只接受 `drafts/` 下已存在的普通 `.tree` 文件，取得独占元数据锁，要求原有的审核标记行保持不变，用临时文件加原子替换写入，替换前比较摘要（复用 `metadata::write_snapshot` 的 `expected` 机制）。不写 `.review.json`，不规范化。这是 GUI 唯一一个没有对应 `organizer` 命令的写路径；它代替 README 中“用文本编辑器修改草案”这一步，在活动日志里标为“GUI 内部操作”。
4. **LLM 策略参数**：新增 `LlmPolicy { disabled, local_only, no_content }`，由调用方传入，并与环境变量取“或”；`chat_with_provider` 和 `OpenAiCompatible::new` 都用它检查。CLI 传 `LlmPolicy::default()`，行为不变。
5. **未保存草案检查**：`frontend::check_draft(saved, path, text)`，对内存中的文本执行与 `load::draft` 相同的解析和 `check_review`，不读写磁盘，不扫描。`saved` 取自会话的最近快照。

TUI 设计第 10 节中尚未实现的两项仍放在后续：按提交读取 `structure.tree`（历史比较，第三期），结构化错误类型（第二期）。GUI 比 TUI 更需要后者，因为错误要对应到具体按钮，例如“基线过期”直接给出“归档并同步”。

## 12. 代码结构

```text
src/frontend/            # 新，库的一部分，TUI 和 GUI 共用；不依赖 ratatui
├── mod.rs
├── state.rs             # 工作区状态（TUI 设计 3.1）
├── rounds.rs            # 轮次配对与阶段（3.2）
├── review.rs            # 审核标注
├── tree_index.rs        # 子节点顺序与路径
├── load.rs              # 刷新时的全部读盘
├── action.rs            # 动作与等价命令
├── worker.rs            # 串行执行与事件
├── next.rs              # NextStep
├── llm_info.rs          # LLM 配置摘要与策略
├── check.rs             # 未保存草案检查
└── text.rs              # clean、hint
src/tui/                 # 只剩视图、按键、对话框、终端处理和外部编辑器

gui/
├── package.json         # svelte、vite、typescript、@tauri-apps/api、codemirror、vitest、playwright
├── vite.config.ts
├── index.html
├── src/
│   ├── main.ts
│   ├── App.svelte       # 欢迎窗口与工作区窗口的路由
│   ├── ipc/             # invoke 封装；types.ts 由 ts-rs 生成，不手改
│   ├── stores/          # 会话快照、选中项、活动日志
│   ├── pages/           # Welcome、Init、Overview、Structure、Organize、History、Recovery、Settings
│   ├── components/      # Sidebar、Toolbar、NextStepBanner、Outline、Inspector、Badge、Sheet 等
│   ├── sheets/          # Confirm、Sync、Chat、Plan、Apply、Progress、Archive、MarkResolved
│   ├── editor/          # CodeMirror 配置与 TreeSpec 高亮
│   └── styles/          # 颜色、字体与浅色/深色变量
├── tests/               # Vitest 组件测试；Playwright 测试（模拟 IPC）
└── src-tauri/
    ├── Cargo.toml       # 独立 [workspace]；organizer = { path = "../..", default-features = false }
    ├── build.rs         # tauri-build，AppManifest 声明全部命令
    ├── tauri.conf.json  # CSP、窗口、bundle、macOS 最低版本
    ├── Info.plist       # 用途说明、文档类型
    ├── capabilities/main.json
    └── src/
        ├── main.rs
        ├── session.rs   # 窗口 ↔ 根目录 ↔ worker
        ├── commands.rs  # 3.4 的命令；名称 → 路径解析
        ├── dto.rs       # ViewModel，派生 Serialize 与 ts_rs::TS
        ├── token.rs     # 执行令牌
        ├── settings.rs  # 设置文件与钥匙串
        └── macos.rs     # 文件夹面板、Finder、caffeinate、xcode-select 检查、退出拦截
```

## 13. 测试、构建与发布

### 13.1 测试

**Rust（任何平台都能跑）**

- `frontend` 的单元测试随 TUI 代码迁移过来，覆盖每种工作区状态和轮次阶段；新增 `NextStep` 和 `check_draft` 的测试。
- `save_draft`：摘要不一致、审核标记被改、目标是符号链接、不在 `drafts/` 内、名称不存在，都要拒绝。
- GUI 后端：`commands.rs` 的逻辑写成普通函数，Tauri 命令只是外壳。测试在临时目录上用 Mock LLM 走完“初始化 → 同步 → 草案 → 保存 → 计划 → 预览 → 执行 → 归档”，并覆盖：快照中不存在的名称被拒绝、名称中的 `..` 和 `/` 被拒绝、没有令牌或令牌过期时执行被拒绝、刷新后令牌失效。
- DTO：对固定场景的 `ViewModel` JSON 做快照比较；`ts-rs` 生成的类型与仓库中的文件一致。
- `parity` 测试继续检查每个 `Action` 生成的命令行能被真实 CLI 解析器解析为相同参数，GUI 自动受益。

**前端（不需要 macOS）**

- Vitest 组件测试：给定 `ViewModel`，检查显示内容和按钮可用性。
- 不可信文本：用含 `<img onerror>`、`</script>`、控制字符和双向文本控制符的文件名与 LLM 理由渲染页面，断言没有生成元素、没有执行脚本。
- Playwright 运行 Vite 开发服务器，用 `@tauri-apps/api/mocks` 的 `mockIPC` 模拟后端，覆盖键盘流程：确认工作表的默认按钮是取消、输入法组合时 Return 不触发、强确认必须完整输入 `apply`。

**macOS 手工检查（每次发布前）**

Tauri 的官方 WebDriver 不支持 macOS（WKWebView 没有驱动程序），第三方驱动可作补充，但不作为发布门槛。手工检查清单：

- 在桌面、文稿、下载、外置卷和网络卷上打开并初始化；撤销授权（`tccutil reset All <Bundle ID>`）后重新打开，确认出现“重新授权”。
- 在未安装命令行工具的系统上打开工作区。
- 执行迁移期间按 `⌘Q`、关闭窗口、注销、合盖；用活动监视器强制退出后重新打开，确认进入恢复页。
- 浅色、深色和增强对比度；VoiceOver 完整走一遍整理流程。
- 用简体拼音输入法在请求框、编辑器、文件名框和 `apply` 输入框中输入。
- 同时用 CLI 或 TUI 操作同一个根目录，确认出现“元数据正被其他进程使用”。

### 13.2 构建与 CI

```bash
cd gui
npm ci
npm run tauri dev                                  # 开发
npm test && npm run check && npm run lint          # Vitest、svelte-check、ESLint
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri build -- --target universal-apple-darwin
```

根目录的门禁（`cargo fmt`、两种 feature 组合的 `cargo clippy`、`cargo test --locked`）保持不变，`frontend` 的测试包含在内，可以继续在 Linux 上运行。GUI 的门禁在 macOS runner 上运行：`src-tauri` 的 fmt、clippy、test，以及前端的 test、check、lint 和 `ts-rs` 类型一致性检查。前端测试也可以在 Linux 上运行。

### 13.3 签名与分发

- 产物是通用二进制（Apple Silicon 与 Intel）的 `.app` 和 `.dmg`。`rusqlite` 的 bundled SQLite 分别编译进两个架构。
- 使用 Developer ID Application 证书签名，启用 Hardened Runtime，用 `notarytool` 公证并装订（staple）。Tauri 的打包流程读取 `APPLE_SIGNING_IDENTITY`，以及 `APPLE_API_KEY`、`APPLE_API_ISSUER`、`APPLE_API_KEY_PATH`（App Store Connect API 密钥）完成签名和公证。
- 授权文件（entitlements）从空开始，只有公证或运行时确实需要时才添加。
- `Info.plist`：9.1 的用途说明、9.4 的文档类型、`LSMinimumSystemVersion`（13.0）、`LSApplicationCategoryType` 为 `public.app-category.productivity`。
- **只用公证的 DMG 分发，不上 Mac App Store。** 上架要求启用 App Sandbox。沙盒中无法可靠调用系统 `git`，访问根目录要改用安全范围书签，还要让锁、SQLite 和临时文件都在书签范围内工作。这些改动量大，目前也没有必要。
- 自动更新（`updater` 插件）放在第二期。

## 14. 分期与实现顺序

**第一期**覆盖 TUI 第一期的全部流程，加上 GUI 必需的部分：

1. 共享前端层与第 11 节第 1、2、4、5 项接口。纯重构加小接口，根目录门禁全部通过后再开始界面。
2. `gui/` 脚手架：Tauri + Svelte + Vite，CSP、capability、`AppManifest`，菜单，欢迎窗口，打开文件夹，会话与 worker，`snapshot`/`refresh`，`ts-rs` 类型。
3. 只读页面：初始化页、概览、结构（含范围预览）、整理（审核、计划、报告）、历史、恢复。
4. 写动作：初始化、同步（含 `--accept-unknown`）、复制基线、生成计划、预览、执行（令牌、进度、阻止睡眠、退出拦截）、导出脚本、归档、补记 Git、标记已处理。活动日志。
5. 草案编辑器与第 11 节第 3 项 `save_draft`。
6. 设置、钥匙串、LLM 草案（隐私确认）。
7. macOS 集成：TCC 引导、命令行工具检查、Finder、拖放；签名、公证和 DMG；13.1 的手工检查。

**第二期**：结构化错误与对应按钮；Quick Look；系统通知、Dock 标记、系统最近项目；自动更新；超大基线树的按需加载（先测出瓶颈再做）。

**第三期**：历史版本查看与比较；结构化编辑（在树上拖放节点、新建目录、改名，规则同 TUI 设计 5.6，需单独确认）；在设置中安装命令行工具（把应用内的 `organizer` 链接到 `PATH`）。

## 15. 待确认的决定

每项给出推荐选项。

1. **技术栈**：推荐 Tauri 2 + Svelte 5 + TypeScript（第 2 节）。另一选择是 SwiftUI + UniFFI，观感更原生，但要多维护 Swift 代码和 Xcode 工程。
2. **共享层的位置**：推荐作为库内模块 `organizer::frontend`。另一选择是把仓库改成 Cargo workspace 并新建一个 crate，边界更清楚，但要调整根目录的构建和门禁。
3. **内置草案编辑器**：推荐放在第一期，并新增 `workspace::save_draft`。它是 GUI 唯一一个没有对应命令的写路径（第 11 节第 3 项）。另一选择是第一期只提供“用其他应用打开”，但默认的“文本编辑”会破坏 JSON 引号名称。
4. **分发方式**：推荐 Developer ID 签名、公证的 DMG，不启用 App Sandbox，不上 Mac App Store（13.3）。
5. **最低系统版本**：推荐 macOS 13。
6. **LLM 配置来源**：推荐 GUI 设置加钥匙串，环境变量中的限制开关仍然生效且不能被设置放宽（9.3）。
7. **窗口激活时是否扫描**：推荐默认不扫描，只提示“可能已过期”；设置中可以打开（3.5）。
8. **执行前的强确认**：推荐与 TUI 相同，输入 `apply`，不再加原生二次确认框。
9. **应用名称与 Bundle ID**：建议应用名“TreeSpec Organizer”，Bundle ID 需要一个你控制的反向域名（例如 `io.github.<用户名>.treespec-organizer`）。TCC 授权和钥匙串项都绑定 Bundle ID，发布后不宜再改。

## 16. 参考

- [TUI 设计](TUI_DESIGN.md)：状态模型（第 3 节）、确认规则（第 6 节）与已确认的决定（第 12 节）。
- [Tauri 2.11.1 发布说明](https://tauri.app/release/tauri/v2.11.1/)：自定义命令的 ACL 安全修复。
- [Tauri Core Releases](https://tauri.app/release/core/)：当前版本。
- [Tauri WebDriver](https://tauri.app/develop/tests/webdriver/)：桌面端官方驱动只支持 Windows 与 Linux。
- [Tauri Prerequisites](https://tauri.app/start/prerequisites/)：macOS 10.15 及以上。
- [What's new in Svelte: September 2026](https://svelte.dev/blog/whats-new-in-svelte-september-2026)：Svelte 5.57。
- [Apple Developer Forums：Reset access obtained through infe…](https://developer.apple.com/forums/thread/124121)、[Michael Tsai：Privacy & Security Settings Don’t Show Intent-Based Access](https://mjtsai.com/blog/2026/04/10/privacy-t-show-intent-based-access/)：通过“打开”面板获得的文件夹授权会持续有效，且不显示在系统设置中。

## 17. 实现记录（第一期，2026-11-06）

### 17.1 与上文的差异

- **GUI crate 分成两层。** `gui/src-tauri` 的库 `organizer_gui`（`src/*.rs`）是与 Tauri 无关的核心：会话、命令白名单、视图模型、设置、钥匙串、执行令牌。可执行文件 `treespec-organizer`（`src/app/`，feature `app`）只负责窗口、菜单、IPC 和生命周期。这样核心可以在任何平台上用 `cargo test --no-default-features` 测试，第 12 节的 `commands.rs`、`dto.rs`、`settings.rs` 等都在核心里，`macos.rs` 改名为 `system.rs`。
- **新增 IPC 命令。** 除 3.4 的命令外：`window_info`（欢迎窗口还是工作区窗口）、`open_recent`/`open_parent`（前端只传最近列表的序号，不传路径）、`command`（确认工作表的等价命令由 Rust 的 `Action::command` 生成，前端不拼接）、`set_guard`（编辑器报告未保存修改，供关闭和退出拦截）、`force_close`、`system`（安装命令行工具、打开“文件与文件夹”设置、重新授权、选择外部编辑器等）。全部列在 `build.rs` 的 `AppManifest` 和 `capabilities/main.json` 中。
- **事件按窗口投递。** 后端用 `emit_to(窗口标签, …)` 发送 `gui`、`menu`、`guard`、`activated`、`welcome` 事件，前端用当前窗口的 `listen` 接收，事件不会串到别的窗口。
- **插件只用 `dialog`。** 见 2.3。窗口大小和检查器开关按根目录保存在 `settings.json`。
- **“另存为新草案”。** 文件在外部被修改时，编辑器提供“重新载入”和“拷贝我的文本”。库只能从基线新建草案，不能用任意文本新建，所以没有直接的“另存为”。
- **前端测试的后端替身。** Playwright 没有使用 `@tauri-apps/api/mocks`，而是用 `vite build --mode mock` 构建出带内存后端的页面（`gui/src/mock/`），它和 Tauri 命令同名、同类型。生产构建不包含它（构建后检查）。
- **执行期间的关闭与设置。** 迁移进行中只允许“仍然退出”（之后由操作日志进入恢复），不允许单独关闭窗口：关闭不会停止迁移，只会让它失去退出拦截。外部编辑器只能通过系统“选择应用”面板设置，WebView 不能指定要启动的程序；已保存 API Key 时更改服务地址，需要在原生对话框中确认，WebView 无法代替用户点击。执行前除了令牌，还要确认计划文件中的 `plan_id` 仍是预览时那一份。
- **共享前端层的补充。** worker 线程捕获任务中的 panic 并仍然发出“完成”事件，前端不会一直显示忙碌。`organizer::frontend` 另有 `recovery`（恢复页“当前磁盘”提示，原在 TUI 视图中）和 `check`（未保存草案检查）。`frontend::text::hint` 的中文提示不再提 TUI 键位，两个前端通用。`main.rs` 的输出辅助函数移到库里的 `organizer::output`。

### 17.2 验证情况

已验证（Linux 开发环境）：根目录门禁（fmt、两种 feature 组合的 clippy、137 个测试）；`gui/src-tauri` 核心 24 个测试（在真实临时目录上走完“初始化 → 复制基线 → 编辑保存 → 计划 → 预览 → 执行 → 归档”，以及令牌失效与过期、预览后计划文件被替换、白名单拒绝路径和名称、关闭窗口后会话被释放）；整个 Tauri 应用对 `aarch64-apple-darwin` 的交叉 clippy（含 `tauri-build` 对命令、capability、配置和 Info.plist 的检查，但不链接）；前端 svelte-check、ESLint（含 `svelte/no-at-html-tags`）、Vitest 5 个测试（工作表键盘规则、不可信文本只按文本渲染）、Playwright 4 个流程。

另做了一次独立的安全审查（白名单、路径、令牌、锁顺序、设置），上面“执行期间的关闭与设置”和会话释放、任务队列顺序、草案读取时的符号链接替换都是据此修正的。

尚未验证，需要在 Mac 上完成：应用实际启动与运行（WKWebView 渲染、CSP、菜单、红绿灯位置、拖放、Dock 打开文件夹）、钥匙串读写、`caffeinate`、退出与注销拦截、TCC 授权提示与重新授权、签名与公证，以及 13.1 的全部手工检查。

