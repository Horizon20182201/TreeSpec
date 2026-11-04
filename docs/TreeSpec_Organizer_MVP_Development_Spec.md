# TreeSpec Organizer — MVP 开发规范（供 Coding Agent 实现）

> 版本：v0.1 · 状态：可实施草案  
> 项目定位：**用 Git 版本化文件和目录的组织结构，用 LLM 根据用户自然语言提出新的结构，再安全地把旧结构迁移到新结构。**  
> 重点：**只管理文件路径和结构元数据，不使用 Git 跟踪被管理文件的内容。**

## 0. 给 Coding Agent 的执行指令

你正在实现一个本地文件整理 CLI 工具，暂定名称 `organizer`。以本文档为需求基准。请：

1. **先完成可运行的非 LLM 闭环**：TreeSpec 解析/序列化 → 扫描与同步 → 结构 Diff → 迁移计划 → dry-run → 安全执行 → Git 结构快照。
2. 再实现可插拔的 LLM 接口：自然语言编辑树、语义分类文件；LLM **只能返回结构化建议**，不能直接运行命令、删除文件或编辑真实目录。
3. 采用 Rust workspace 或单 crate 模块化项目，优先实现 Linux 与 macOS；核心逻辑保持跨平台，不做 GUI、后台 daemon 或文件监听。
4. 所有真实文件操作必须具有冲突检查、路径安全校验、禁止覆盖、执行日志。**永不因 Tree 差异自动删除非空文件/目录。**
5. 先写单元/集成测试；每个阶段完成后能通过 `cargo test`，并用 `cargo fmt`、`cargo clippy` 检查。
6. 有歧义时采取保守行为：报告 `unresolved` 或停止操作，**不能猜测文件的去向，更不能静默丢弃文件**。

> 交付标准：一个可以在临时测试目录内完成「初始化 → 手工移动文件 → 同步 → 生成新版树 → 规划 → 预览 → 应用 → Git 记录」的 CLI；有 API 配置时可以通过自然语言生成新版树；无 API 时所有确定性功能仍可使用。

---

## 1. 背景、目标与非目标

### 1.1 用户痛点

用户习惯先设计目录框架，但文件积累后，旧结构可能不合适。日常出现：文件放错位置、忘记位置、不知新文件该放哪里，以及整体重构成本过高。

### 1.2 MVP 核心用户流程

```text
真实文件系统 ──扫描──> structure.tree（当前已应用结构）
                                   │
                            自然语言 / 手工编辑
                                   ▼
                              new.tree（草案）
                                   │
                 Tree Diff + 文件状态 + 可选 LLM 语义映射
                                   ▼
                              migration.json
                                   │
                        预览 → 显式确认 → 执行
                                   ▼
                  更新 structure.tree + Git commit
```

### 1.3 范围内

- 单个指定根目录的本地管理；CLI 调用时扫描，没有后台监控。
- **完整 Tree 快照**：列出受管理范围内的目录和普通文件的相对路径、稳定 ID；可为目录添加语义说明。
- `old.tree` 与 `new.tree` 比较，生成明确的目录/文件迁移计划。
- LLM 基于用户自然语言编辑 Tree；需要时读取有限文件上下文并给文件选目标目录。
- 默认只执行创建目录、移动/重命名文件或目录、清理已经确认为空的目录；不修改文件内容。
- 元数据存入独立的 Git 仓库；支持结构历史查看。
- 冲突检测、dry-run、持久化操作日志、失败后恢复指引。

### 1.4 暂不实现

- 后台 daemon / inotify / FSEvents / ReadDirectoryChangesW 监听。
- 图形界面、云同步、跨设备结构同步。
- 文件正文版本控制、备份或被删除内容恢复。
- 向量数据库、自然语言全量检索、复杂规则语言。
- 默认分析所有 PDF 全文、OCR、多模态视频/音频内容。
- 跨文件系统移动、自动去重、自动覆盖、批量删除。
- 复杂的忽略规则语言；MVP 只支持少量内建忽略项。

---

## 2. 关键原则与系统不变量

1. **磁盘是实际状态的事实来源**：扫描结果不能因配置期望而假装文件存在。
2. **Tree 是结构快照，不是内容快照**：仅存名称、相对路径、ID 和少量注释；不把用户文件内容写入 Git。
3. **已应用结构与草案分离**：`.organizer/structure.tree` 是已同步/已应用的基线；LLM 只生成草案（`drafts/` 或用户指定的 `new.tree`）。
4. **完整性约束**：同一文件在 Tree 中只有一个节点；目标 Tree 不允许无解释地遗漏旧文件。遗漏代表待解决，而非删除指令。
5. **路径/身份分离**：ID 用来跨路径变化识别同一个节点；路径不是身份。
6. **LLM 只能建议**：绝不直接调用 shell、执行文件操作或绕过验证。
7. **执行前二次验证**：对计划涉及的文件/目录检查身份、状态和目标冲突；计划过期则停止。
8. **默认无破坏性操作**：不覆盖文件、不自动删除文件、不擅自进入符号链接或受保护子树。
9. **Git 与执行日志职责不同**：Git 记录已确认结构版本；操作日志记录实际迁移进度。Git 无法恢复文件内容。
10. **可无 LLM 使用**：手动编辑 `new.tree` 后可完全确定性地生成和执行迁移。

---

## 3. 项目文件布局

被管理根目录（例 `~/Workspace`）：

```text
Workspace/
├── Research/
├── Courses/
├── Inbox/
└── .organizer/
    ├── .git/                 # 仅以 .organizer 为 Git 工作树
    ├── .gitignore
    ├── structure.tree        # 已应用的完整结构快照（Git 跟踪）
    ├── drafts/               # 草案，不提交
    ├── plans/                # 迁移计划，不提交
    ├── state/
    │   ├── index.sqlite     # 文件身份与扫描缓存，不提交
    │   └── operations.sqlite# 执行日志，不提交
    └── cache/                # 文件摘要/LLM 缓存，不提交
```

`.organizer/.gitignore` 默认包含：

```gitignore
drafts/
plans/
state/
cache/
*.log
.env
```

Git 在 `.organizer` 内初始化，**绝不在 Workspace 根目录初始化 Git**。显式将 `.organizer/structure.tree` 加入 Git；初始化/同步/成功执行后提交结构快照。验证仓库中不存在用户真实文件正文、数据库或密钥。

> 注：`structure.tree` 包含文件名与路径；这些仍可能是敏感元数据，因此仓库默认仅保存在本地，不自动创建远端。

---

## 4. TreeSpec v1：极简语言

### 4.1 示例

```text
!tree 1

Research/ @d_research # 科研资料
  Papers/ @d_papers # 学术论文
    CRUSH.pdf @f_crush
    Ownership.pdf @f_ownership
  Projects/ @d_projects # 研究项目
    CRUSH/ @d_crush_project
      README.md @f_readme
      main.rs @f_main

Courses/ @d_courses # 课程
  OS/ @d_os # 操作系统
    lab1.pdf @f_lab1

Inbox/ @d_inbox # 暂未分类
```

- 行尾 `/`：目录；无 `/`：普通文件。
- 缩进：每一层 **两个空格**；禁止 Tab。
- `@ID`：可选的稳定节点 ID；持久化后的规范化 Tree 中必须为所有节点自动补齐 ID。
- `# 说明`：可选；主要用于目录语义。允许为文件添加注释，但不强制。
- 空行、独立注释行：允许。
- 头部首个有效声明：`!tree 1`。

### 4.2 词法与语法约定

**每条节点声明**：

```text
<2*N 个空格><名称><可选的目录结尾 /> [@ID] [# 描述]
```

- 普通名称支持中文、空格、点号、连字符等；若包含 ` #`、` @`、引号等可能与语法冲突的内容，使用双引号包住名称，例如 `"C#"/`、`"2026 @ Research"/`。
- 双引号内使用 JSON 字符串转义语法。名称必须是**单个路径段**，不得包含 `/`、`\\`、NUL、`.` 或 `..`（Windows 额外合法性由平台检查）。
- `@ID` 在**整个 Tree 内唯一**，使用 `[A-Za-z0-9_-]+`，推荐工具生成 `d_<uuid>` / `f_<uuid>`；用户可用短 ID，但不应因修改路径而改变 ID。
- 一个节点只能在目录之下；文件节点不能有子节点。
- 同一父目录下不能有同名节点；重复 ID、跳级缩进、非法路径、缺失父目录均解析失败。
- 同目录下节点排序采用**目录优先、文件其次、各组名称稳定排序**，以降低 Git diff 噪声（不会修改文件系统的实际顺序）。
- 编码使用 UTF-8，行结束统一为 LF。

### 4.3 兼容性边界

- Tree 只支持目录和普通文件；符号链接、socket、设备节点、FIFO 等特殊文件不写入 MVP Tree，扫描时显示警告并跳过。
- `.organizer/` 永不进入快照。
- 遇到嵌套的 Git 项目目录，可将其作为**不深入扫描的独立目录节点**，避免移动源码文件破坏仓库结构；该行为应明确在 `status` 中提示。以后可增设 opt-in 深入管理。
- 不在 Tree 内编码文件大小、mtime、文件内容哈希；这些是运行时扫描索引信息。

### 4.4 AST 与解析 API（概念接口）

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
enum NodeKind { Directory, File }

struct TreeNode {
    id: String,
    parent_id: Option<String>,
    name: String,
    kind: NodeKind,
    description: Option<String>,
}

struct TreeSpec {
    version: u32,
    nodes: Vec<TreeNode>,
}

fn parse_tree(source: &str) -> Result<TreeSpec, TreeError>;
fn render_tree(tree: &TreeSpec) -> Result<String, TreeError>;
fn validate_tree(tree: &TreeSpec) -> Result<(), TreeError>;
```

验证器同时检查 parent 引用、层级无环、重复 ID、重名、非法路径。`render_tree(parse_tree(s))` 结果应稳定可重复（幂等）。在规范化导出之前分配缺失 ID。

---

## 5. 文件扫描与 `sync` 机制（无后台监听）

### 5.1 命令行为

- `organizer status`：**只读**扫描当前磁盘并与已记录快照比较，不修改结构树、不提交 Git。
- `organizer sync`：扫描并把已确认的实际变化写入 `structure.tree` 和 SQLite，产生一次结构 Git commit（若存在结构变化）。
- `organizer plan`：先只读扫描并检测基线是否过期；若与 `old.tree` 不一致，停止并建议 `sync`（或明确解决冲突）。
- `organizer apply`：执行前重新扫描受影响路径并核验前置条件；成功后更新 `structure.tree`、索引并提交 Git。

### 5.2 身份识别优先级

`index.sqlite` 至少保存：`logical_id`、`last_path`、`kind`、`platform_file_key`（可用时）、`size`、`mtime`、`last_seen_scan`。

匹配流程：

1. 路径相同、类型兼容且身份信息未冲突：延续旧 ID。
2. 路径改变，但同卷稳定文件 ID（如 Unix device+inode）匹配唯一旧节点：识别移动/重命名，延续旧 ID。
3. 不足以确定时，**不要**仅凭同名或同哈希强行认作同一个文件；标记疑似匹配并让用户确认。MVP 可以暂记“旧节点消失 + 新节点出现”。
4. 复制文件必须得到**新 ID**；同内容不等于同身份。

注意：inode 可能复用；跨卷移动和离线批量操作存在歧义。扫描后需结合类型、大小、时间、路径历史等信息降低误判，但不能保证全平台 100% 精确。

### 5.3 `sync` 的冲突原则

- 外部新增文件：自动分配 ID，写入 Tree。
- 外部删除文件：从**当前结构快照**移除，并记录提示；不执行真实删除、不承诺恢复其内容。
- 外部移动/重命名：确认身份后保留 ID，更新路径。
- `structure.tree` 存在未应用的手工修改或有未解决的草案/状态冲突：`sync` 不应静默覆盖。建议先存草案或解决冲突。
- 文件内容变化但路径/组织未变化：可以更新缓存，**不应因此产生 Git 结构提交**。

### 5.4 扫描边界

- 不递归跟随符号链接，跳过 `.organizer/`。
- 不跨越管理根目录；根目录本身不能成为迁移目标。
- 支持大目录的流式/逐批扫描，不把文件正文载入内存。
- 发生权限错误、不可达目录或扫描中断时，不能把这些文件错误地判为删除；报告**扫描不完整**并停止自动同步。

---

## 6. Tree Diff：纯确定性比较

### 6.1 输入与输出

输入：规范化的 `old.tree` 和 `new.tree`（所有节点都有唯一 ID）。

输出：`TreeChange[]`，每个变化包含节点 ID、旧路径、新路径、变化类型。

变化类型：

- `add_node`
- `remove_node`（只表示目标树缺少该节点，**不是删除文件命令**）
- `rename_node`（ID 不变，name 变）
- `reparent_node`（ID 不变，parent_id 变）
- `update_description`

同一节点可同时发生 rename 和 reparent。父目录改名导致后代路径随之改变，不应重复生成一堆无意义的子节点 rename。

### 6.2 匹配规则

1. **ID 相同 → 同一逻辑节点**（类型不得从文件变目录或反之；若发生，作为冲突处理）。
2. 无 ID 的输入先规范化：与旧树**同路径、同类型**者继承旧 ID；其他分配新 ID。
3. 无 ID 且只有名称/路径相似，**不可自动推断重命名**；等待用户确认对应关系。
4. 新树中旧文件 ID 消失：生成 `unresolved_missing_file`，不得生成 `rm`。
5. 新树增加一个不存在于当前磁盘的普通文件：生成 `unresolved_new_file`，工具不会凭 Tree 创建空文件。
6. 旧目录在新树中消失：仅在其下文件已明确迁移完且磁盘目录为空时，才可选移除空目录；否则阻断清理。

### 6.3 基本算法

- 为两棵树构建 `Map<ID, Node>` 和 `Map<RelativePath, Node>`。
- 交并集找新增/缺失 ID。
- 公共 ID 比较类型、name、parent_id、description。
- 单独计算由祖先迁移带来的有效路径变化。
- 验证最终目标树无路径冲突、无自嵌套移动、无 ID 重复。

核心比较时间复杂度可为 `O(N+M)`（不含排序、路径计算和文件系统扫描）。

---

## 7. LLM：两个明确分开的结构化接口

LLM 使用统一的 Provider 抽象；第一版实现至少一个可配置的 **OpenAI-compatible HTTP API**，通过环境变量读取端点、模型和密钥（例如 `ORGANIZER_LLM_BASE_URL`、`ORGANIZER_LLM_MODEL`、`ORGANIZER_LLM_API_KEY`）。测试中使用 mock provider，网络失败不会影响基础扫描/规划功能。

### 7.1 TreeEditor：自然语言 → 树编辑建议

输入：当前相关 Tree 子树、用户需求、可操作节点 ID；大树只传用户指定的子树与必要的祖先上下文。

**只接受白名单操作**：`add_directory`、`rename_node`、`move_node`、`set_description`、`remove_empty_directory_declaration`。对文件的迁移不直接由 TreeEditor 猜测，交给 FileMapper。

示例输出：

```json
{
  "schema_version": 1,
  "edits": [
    {
      "op": "add_directory",
      "parent_id": "d_papers",
      "name": "C2Rust",
      "description": "C-to-Rust 翻译研究论文"
    },
    {
      "op": "add_directory",
      "parent_id": "d_papers",
      "name": "Security",
      "description": "系统安全研究论文"
    }
  ],
  "migration_intent": {
    "kind": "redistribute_existing_files",
    "scope_dir_ids": ["d_papers"]
  }
}
```

`migration_intent.kind` 最少支持：

- `structure_only`：仅改变目录结构，不迁移旧文件。
- `redistribute_existing_files`：用户明确要求对指定子树重新分类。

工具验证、分配新 ID、应用到 AST 并输出草案 `new.tree`，LLM 不直接写原始 `structure.tree`。模型输出解析失败时可重试一次，仍失败则报错。

### 7.2 FileMapper：有限文件上下文 → 目标目录 ID

**只有需要语义判断时才调用**：拆分、合并、按主题重新分类，以及用户明确要求按内容整理。仅做目录确定性重命名时不调用。

输入至少包含：

```json
{
  "schema_version": 1,
  "user_intent": "把已有论文按主题分类",
  "candidate_directories": [
    {"id": "d_c2rust", "path": "Research/Papers/C2Rust", "description": "C-to-Rust 翻译"},
    {"id": "d_security", "path": "Research/Papers/Security", "description": "系统安全"},
    {"id": "d_papers", "path": "Research/Papers", "description": "研究论文"}
  ],
  "files": [
    {"id": "f_crush", "path": "Research/Papers/CRUSH.pdf", "context": "Semantics-guided C-to-Rust translation"},
    {"id": "f_aslr", "path": "Research/Papers/ASLR.pdf", "context": "Address space layout randomization"}
  ]
}
```

输出：

```json
{
  "schema_version": 1,
  "assignments": [
    {"file_id": "f_crush", "decision": "place", "target_dir_id": "d_c2rust", "reason": "C-to-Rust 翻译"},
    {"file_id": "f_aslr", "decision": "place", "target_dir_id": "d_security", "reason": "ASLR 属于系统安全"}
  ]
}
```

`decision` 枚举：`place`、`keep`、`review`。候选目录之外的 ID 一律拒绝；重复/未知文件 ID 一律拒绝；漏掉文件按 `review` 处理；模型的理由只用于 UI 展示，不能替代验证。未能分类的文件保留原位置并列入 `unresolved` 或需要审核项。

文件内容读取策略：MVP 默认文件名+目录上下文；按需读取文本文件的有限字节或 PDF 元数据/标题（提取不成功则退回文件名），严禁无限制全文上传。可配置禁用内容读取或云端发送；跳过私钥、`.env` 等敏感候选。文件中的提示词是**不可信数据**，不构成新的系统命令。

### 7.3 TreeEditor 与 FileMapper 的串联

1. TreeEditor 生成结构编辑和 `migration_intent`。
2. 工具构造目标目录 AST 和初步草案。
3. 若意图是重新分类，FileMapper 仅针对相关文件返回归属。
4. 工具按 `file_id` 把文件移动到目标目录的 AST 节点下；没有明确选择的文件保留原位置。
5. 校验所有旧文件均有去向，导出完整的 `new.tree`；未解决项要显示出来，不能假装已迁移成功。
6. **用户审核 `new.tree` 和详细计划后**，才执行真实操作。

---

## 8. Migration Planner：完整目标树 → 操作计划

### 8.1 输入

- 规范化的 old/new Tree。
- 当前文件系统扫描快照及稳定 ID 对照。
- 结构 Diff。
- 可选 LLM 文件映射/用户批准的重新分类意图。

### 8.2 输出格式（JSON v1）

```json
{
  "schema_version": 1,
  "plan_id": "plan_example",
  "root": "/absolute/path/to/Workspace",
  "base_tree_digest": "sha256:...",
  "target_tree_digest": "sha256:...",
  "scan_id": "scan_example",
  "operations": [
    {"op": "mkdir", "path": "Research/Papers/C2Rust"},
    {
      "op": "move_file",
      "file_id": "f_crush",
      "from": "Research/Papers/CRUSH.pdf",
      "to": "Research/Papers/C2Rust/CRUSH.pdf"
    }
  ],
  "unresolved": [],
  "warnings": []
}
```

真实计划还必须包含受影响文件的预期身份/元数据前置条件（可单列 `preconditions`），以及与审核草案关联的目标 Tree 内容或安全引用，防止执行时草案被替换。`scan_id`、digest 应是**真实计算值**，上例占位符只用于展示格式。

### 8.3 操作白名单

- `mkdir`
- `move_file`
- `move_directory`
- `rmdir_if_empty`（可选，且只清理经确认的空目录）

**没有** `delete_file`、`overwrite`、任意 shell 命令执行。

### 8.4 规划原则

1. 先做 ID 对应的确定性 rename/reparent；需要文件内容分类时再调用 LLM。
2. 单个文件改路径生成 `move_file`；整个同 ID 目录变路径优先生成 `move_directory`，避免对子项重复搬移。
3. 目录内已有手工操作、部分文件需要从目录迁出或迁入时，规划器必须正确排列前后依赖，必要时拆分目录级操作或使用私有暂存路径。
4. 对源/目标路径进行归一化、安全校验；所有目标必须在根目录内部且不穿过符号链接。
5. 目标存在并且不是计划中已安全让出的槽位时，报冲突；**禁止覆盖**。
6. 循环路径交换（A→B、B→A）需要临时路径与操作依赖；若暂未实现，必须标记不支持而非冒险执行。
7. 对没有映射的旧文件、没有来源的目标文件、不可达或扫描失败的节点，产生 `unresolved`；未解决项不得被当作删除。
8. `plan` 输出必须稳定；相同输入与快照得到等价操作序列。

### 8.5 可导出迁移脚本，但脚本不是可信的执行器

用户希望从 `old.tree`、`new.tree` 自动生成脚本。提供：

```bash
organizer plan --from old.tree --to new.tree --out migration.json
organizer export-script migration.json --out migrate.sh
```

脚本原则上只调用安全 CLI：

```bash
#!/usr/bin/env bash
set -euo pipefail
organizer apply "/absolute/path/to/migration.json"
```

避免把 LLM 输出拼接成未经校验的 `mv` 命令。脚本路径由程序正确转义；执行器依然需要校验前置条件和冲突。也允许不导出脚本、直接 `organizer apply migration.json`。

---

## 9. Executor：安全执行、日志与 Git

### 9.1 执行过程

1. `apply` 默认先显示 dry-run 摘要；真正执行要求显式命令确认（CLI 交互确认或 `--yes`）。
2. 对计划结构、根目录、目标 Tree digest、受影响节点身份、源文件属性及目标路径进行重新验证；过期则停止。
3. 在 SQLite/日志中持久化 `plan_id`、操作列表和 `pending` 状态，开始执行。
4. 按依赖有序执行，并在每步成功后标记 `done`；发生错误停止并报告已经完成的步骤。
5. 完成后重新扫描，验证实际文件结构符合目标 Tree；不符合则不能标记成功。
6. 更新 `structure.tree` 和 `index.sqlite`，Git 提交一次 `refactor: ...`。
7. 中断后重新启动时报告未完成任务；提供 `organizer recover <run_id>` 检查并继续/人工处理。MVP 至少要确保不会盲目重放已完成操作。

### 9.2 强制安全检查

- 操作只能在根目录内；不允许路径穿越、绝对路径逃逸、符号链接目标绕行。
- 不能把目录移动进自身后代。
- 同卷迁移优先使用 rename。**注意 Unix `rename` 可能覆盖已有目标：仅“先检查目标不存在”不足以消除并发竞态。Linux/macOS MVP 应使用平台提供的 no-replace 原子重命名能力（`RENAME_NOREPLACE` / `RENAME_EXCL`）；做不到时 fail closed，不允许潜在覆盖。**
- 不触碰 `.organizer/`、嵌套 Git 项目的内部内容、系统特殊文件。
- 不跨文件系统迁移，不做隐式 copy+delete。
- 冲突、权限失败、文件已变化、日志状态不一致时停止，不强行修复。

### 9.3 Undo 的实际含义

`organizer undo <run_id>` 可作为下一里程碑实现：基于已执行操作记录生成**反向移动计划**，重新检查当前磁盘条件后再应用。若文件已被修改、迁移目标被占用或原文件已丢失，则报告冲突。**不是内容备份，也不保证所有操作均可回滚。**

### 9.4 Git 策略

- `init`：提交首次 `structure.tree`。
- `sync`：仅当结构确有变化时提交 `reconcile: ...`。
- `apply`：所有文件操作成功且最终扫描通过后提交 `refactor: ...`。
- `chat` / `plan` / `dry-run`：**不提交**草案和计划。
- 仅在 `.organizer` 仓库进行 Git 操作，不递归 add 用户文件。提交失败需提示元数据未提交，但不能假装文件迁移失败或悄悄重复执行。

---

## 10. CLI 合约

| 命令 | 最低行为 |
|---|---|
| `organizer init [--root PATH]` | 初始化管理目录、扫描、生成 Tree/SQLite、创建元数据 Git 仓库 |
| `organizer status [--root PATH]` | 只读扫描，展示 added/missing/moved/changed/unknown/conflicts |
| `organizer sync [--root PATH]` | 将可确定的手动变化同步到 Tree 与 Git；不做文件移动 |
| `organizer chat [--root PATH]` | 自然语言生成草案 Tree；无 API 时明确提示可手工编辑 |
| `organizer plan --from OLD --to NEW --out PLAN [--root PATH]` | 读取两树和当前磁盘，生成操作计划；语义映射可通过 chat 或显式 `--semantic` 接入 |
| `organizer apply PLAN [--dry-run] [--yes]` | 预览或执行已审核运行计划 |
| `organizer export-script PLAN --out SCRIPT` | 生成调用 `organizer apply` 的安全包装脚本 |
| `organizer history [--root PATH]` | 查看元数据仓库的 Git 结构历史 |
| `organizer recover RUN_ID` | 检查中断操作并辅助恢复（可在第一阶段实现最低检查版本） |

`--root` 默认当前工作目录或向上定位最近 `.organizer/` 的受管理根目录；调用 `plan`/`apply` 时不得从不可信路径意外切换管理根目录。

典型流程：

```bash
organizer init --root ./Workspace
organizer status --root ./Workspace
organizer sync --root ./Workspace

# 目标树可由 LLM chat 生成，也可由用户手工编辑
organizer chat --root ./Workspace
organizer plan --root ./Workspace \
  --from ./Workspace/.organizer/structure.tree \
  --to ./Workspace/.organizer/drafts/new.tree \
  --out ./Workspace/.organizer/plans/migration.json

organizer apply ./Workspace/.organizer/plans/migration.json --dry-run
organizer apply ./Workspace/.organizer/plans/migration.json --yes
organizer history --root ./Workspace
```

---

## 11. Rust 代码组织与依赖建议

建议先做单 crate，保持模块边界（避免过早拆 workspace）：

```text
organizer/
├── Cargo.toml
├── README.md
├── src/
│   ├── main.rs                 # clap CLI
│   ├── lib.rs
│   ├── tree/
│   │   ├── mod.rs
│   │   ├── parser.rs
│   │   ├── render.rs
│   │   ├── model.rs
│   │   └── diff.rs
│   ├── scan/
│   │   ├── mod.rs
│   │   ├── inventory.rs
│   │   └── identity.rs
│   ├── sync.rs
│   ├── llm/
│   │   ├── mod.rs
│   │   ├── provider.rs
│   │   ├── tree_editor.rs
│   │   └── file_mapper.rs
│   ├── migration/
│   │   ├── plan.rs
│   │   ├── validate.rs
│   │   ├── execute.rs
│   │   └── journal.rs
│   ├── storage.rs
│   ├── git.rs
│   └── error.rs
├── tests/
│   ├── treespec.rs
│   ├── sync_integration.rs
│   ├── diff_integration.rs
│   ├── migration_integration.rs
│   └── llm_mock.rs
└── examples/
    ├── old.tree
    └── new.tree
```

可考虑的 crates：`clap`、`serde`、`serde_json`、`rusqlite`、`walkdir` 或 `ignore`、`thiserror`、`uuid`、`sha2`、`tracing`、`reqwest`；Git MVP 可以调用本机 Git CLI（参数数组、禁用 shell 插值），以后换 `git2`。选稳定兼容版本即可，不要求绑定指定版本。

公共模块接口建议：

```rust
fn scan(root: &Path) -> Result<ScanSnapshot>;
fn reconcile(snapshot: &ScanSnapshot, saved: &SavedState) -> Result<ReconcileReport>;
fn diff(old: &TreeSpec, new: &TreeSpec) -> Result<Vec<TreeChange>>;
fn build_plan(ctx: &PlanningContext) -> Result<MigrationPlan>;
fn validate_plan(plan: &MigrationPlan, root: &Path) -> Result<ValidationReport>;
fn apply_plan(plan: &MigrationPlan, root: &Path) -> Result<ExecutionReport>;
```

LLM 接口应方便 Mock：

```rust
trait LlmProvider {
    async fn complete_json(&self, request: LlmRequest) -> Result<serde_json::Value>;
}
```

建议使用明确的版本化 JSON Schema 校验 LLM 输出，不要把不合法字段悄悄忽略。

---

## 12. 验收测试（必须自动化）

### 12.1 TreeSpec 解析

- [ ] 解析多级目录与普通文件，支持中文、空格、`#`、`@` 的引号名称。
- [ ] 忽略空行/独立注释，保存描述。
- [ ] 拒绝非法缩进、重复 ID、同层重名、文件下挂子节点、路径分隔符与 `..`。
- [ ] 无 ID 节点生成唯一 ID；解析→序列化→解析保持语义一致。

### 12.2 扫描/同步

- [ ] 初次 init 生成完整 Tree，Git 仓库只跟踪元数据。
- [ ] 手动新增文件后 `status` 报新增，`sync` 增加节点。
- [ ] 同卷移动/重命名文件后 `sync` 尽量保留同 ID。
- [ ] 复制一个相同内容的文件获得新 ID。
- [ ] 仅修改文件正文不改变 Tree/Git 结构历史。
- [ ] 权限不足/扫描失败不能误判所有文件已删除。
- [ ] 不进入 `.organizer/`、符号链接和受保护项目内部。

### 12.3 Diff/规划

- [ ] 同 ID 的目录改名生成 rename，不重复重命名后代节点。
- [ ] 同 ID 的文件改变父目录生成文件迁移。
- [ ] 新增空目录生成 `mkdir`，不强迫搬动现有文件。
- [ ] 缺失旧文件 ID 不生成删除，明确进入 `unresolved`。
- [ ] 目标 Tree 中无来源的新文件报错或待解决。
- [ ] 路径重名、移动进自身后代、类型冲突均阻断计划。
- [ ] 语义分类只允许使用给定的文件/目录 ID，mock LLM 能覆盖 `place/keep/review`。

### 12.4 执行与安全

- [ ] dry-run 完全不修改真实文件系统。
- [ ] apply 在临时目录中完成新增目录与文件移动，最终 Tree 与实际状态一致。
- [ ] 目标已有同名文件时不可覆盖；源文件变动后旧计划不可执行。
- [ ] 计划路径不能逃逸根目录或穿过 symlink。
- [ ] 执行中故障后有日志，重新执行不会盲目重复已完成移动。
- [ ] 成功迁移后 Git 生成结构提交，仓库中无真实文件内容。
- [ ] 没有 LLM Key 时上述非 LLM 测试依然全部通过。

### 12.5 端到端场景

初始磁盘：

```text
Research/
└── Papers/
    ├── CRUSH.pdf
    └── ASLR.pdf
```

`old.tree`（两个文件均有稳定 ID）。用户要求按 C2Rust / Security 分类，得到 `new.tree`：

```text
!tree 1
Research/ @d_research # 科研资料
  Papers/ @d_papers # 研究论文
    C2Rust/ @d_c2rust # C-to-Rust 翻译
      CRUSH.pdf @f_crush
    Security/ @d_security # 系统安全
      ASLR.pdf @f_aslr
```

期望：`plan` 生成两个 `mkdir` + 两个 `move_file`；`dry-run` 无副作用；`apply` 后文件在正确位置，内容字节保持不变；`structure.tree` 与磁盘一致；Git 仅记录新结构。

---

## 13. 开发里程碑与完成定义

### M0 — CLI 骨架 + TreeSpec（首先交付）

- Rust 项目初始化；`init` 基础命令。
- TreeSpec Parser/Validator/Renderer，规范化 ID。
- AST 比较 + Diff 输出；示例 old/new Tree。
- 单元测试、README。

**完成定义**：`cargo test` 通过；`organizer plan` 能打印纯结构差异（此时可暂未生成可执行计划）。

### M1 — 扫描与 Git 结构同步

- 文件枚举、SQLite 身份索引、`status`、`sync`。
- 外部移动/重命名的 ID 关联；Git 专用元数据仓库。
- 检测未完全扫描、受保护路径和冲突。

**完成定义**：手动在文件管理器里改动文件后调用 `sync`，Tree 和 Git 精确反映已确认的结构变化。

### M2 — 安全 Migration Planner / Executor（MVP 核心）

- `old.tree/new.tree` → 计划 JSON。
- `dry-run`、安全 `apply`、操作日志、执行后校验与 Git 提交。
- `export-script` 安全包装脚本。
- 所有冲突/不确定情形 fail closed。

**完成定义**：端到端迁移测试通过；无数据覆盖；没有 LLM 也能手工规划迁移。

### M3 — LLM 语义辅助（MVP 完成）

- 可配置 OpenAI-compatible Provider；mock provider 测试。
- `chat` 通过 TreeEditor 更新草案；FileMapper 对需要语义迁移的文件分类。
- 输出全量 `new.tree`，保留全部已有文件，展示未解决项。
- 文件上下文按需读取、调用边界与隐私保护。

**完成定义**：用户在 CLI 输入自然语言整理目标，得到可审核的目标 Tree，随后 `plan` + `apply` 完成迁移。

### 后续阶段（本轮不做）

- 交互式 TUI / GUI、跨平台完备支持、内容搜索与摘要缓存优化。
- 可靠 undo、跨卷迁移、复杂批量目录重构。
- 文件监听、持续提醒、同步多设备。

---

## 14. 推荐实现顺序与约束冲突的解决优先级

1. **安全性高于自动化程度**：不确定就停止/请求确认。
2. **文件真实状态高于草案**：不为迁就 `new.tree` 而假装源文件存在。
3. **已应用快照高于临时草案**：草案不能覆盖 `structure.tree`。
4. **确定性算法高于 LLM 推断**：已知同 ID rename/move 不请求模型。
5. **可测试的清晰接口高于过度抽象**：先把非 LLM 端到端跑通。

建议 Coding Agent 从 **M0** 开始，先生成项目目录、核心类型和 TreeSpec parser/diff 测试；不要一开始加入后台服务、复杂配置规则或全部功能的空壳实现。之后按 M1 → M2 → M3 逐步完成，并在 README 中记录每个阶段可用的命令、已知限制与测试方式。
