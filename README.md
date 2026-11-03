# TreeSpec Organizer — M0

基于 Rust 的文件结构管理 CLI。当前实现开发规范中的 **M0：TreeSpec v1 + 稳定 ID 结构 Diff**，可以离线运行，无需 LLM Key。

规范见 [TreeSpec_Organizer_MVP_Development_Spec.md](docs/TreeSpec_Organizer_MVP_Development_Spec.md)。M0 的 `plan` 生成纯结构报告，**不是可执行的迁移计划**；不会移动、重命名、删除或创建被管理的普通文件。

## 构建与快速运行

使用稳定版 Rust 工具链（Rust 2024 edition；本次验证使用 Rust 1.95 和 Clippy 1.97）。保留 `Cargo.lock` 以复现依赖版本。

```bash
cargo build --locked
cargo run -- --help
cargo run -- validate examples/old.tree
cargo run -- normalize examples/new.tree
cargo run -- plan --from examples/old.tree --to examples/new.tree

# 可选：写入尚不存在的报告文件
cargo run -- plan --from examples/old.tree --to examples/new.tree --out diff.json
```

构建后的程序是 `target/debug/organizer`。示例报告有 6 条变化：2 个 `add_node`、2 个 `reparent_node`、1 个 `rename_node`、1 个 `update_description`，`unresolved` 为空。文件路径仅从 Tree 计算，无需真实的 PDF 文件或对应目录。

## 当前命令

| 命令 | M0 行为 |
| --- | --- |
| `organizer init [--root PATH]` | 在已有目录内创建 `.organizer` 元数据目录和空的初始草案；默认当前目录 |
| `organizer validate TREE` | 只读检查语法和结构；允许缺失 ID，不回写 |
| `organizer normalize TREE [--base OLD] [--out PATH]` | 补齐 ID、规范排序并输出 Tree；默认 stdout |
| `organizer plan --from OLD --to NEW [--out PATH]` | 以 OLD 为基线规范化 NEW，输出结构 Diff JSON；默认 stdout |

退出码：`0` 表示命令完成且没有未解决文件；`1` 表示解析、校验、身份类型冲突或 I/O 错误；`2` 表示 `plan` 已输出报告但存在 `unresolved`。Clap 的命令行参数错误也使用 `2`，会输出用法错误而非 JSON 报告。`0` **不代表**已验证磁盘条件或可以执行迁移。

输出文件使用 `create_new`，已存在的文件、目录和符号链接均不覆盖，包括输入 Tree 本身。输出父目录必须已存在；元数据写入拒绝包含 `..` 的路径和已有符号链接祖先。用户应使用 `--out` 保存文件，避免 shell 重定向覆盖输入。

### 初始化的阶段边界

```bash
mkdir -p /tmp/organizer-demo
cargo run -- init --root /tmp/organizer-demo
```

生成：

```text
.organizer/
├── .gitignore
├── init.log
├── drafts/
│   └── initial.tree  # 仅包含 !tree 1
├── plans/
├── state/
└── cache/
```

`init` 不扫描磁盘，因此不创建 `structure.tree` 基线，不初始化 SQLite 或 Git，也不表示现有文件已纳入管理。完整初始化留给 M1，避免把空 Tree 误当成实际状态。已有 `.organizer` 时直接失败；创建过程中失败会保留已创建的元数据并提示检查 `init.log`，不会删除目录或盲目重试。Unix 下新建元数据目录权限为 `0700`。

### 正确保留身份

先为旧树补齐并保存 ID，再复制它编辑草案。移动或改名时保留整棵子树已有的 ID。

```bash
# 三个输出文件都必须尚不存在
cargo run -- normalize handwritten.tree --out old.normalized.tree
cp old.normalized.tree draft.tree
# 编辑 draft.tree：修改名称、缩进、说明，保留原有 @ID
cargo run -- normalize draft.tree --base old.normalized.tree --out new.normalized.tree
cargo run -- plan --from old.normalized.tree --to new.normalized.tree
```

`plan --from` 和 `normalize --base` 要求旧树所有节点都有显式 ID。新树中缺少 ID 的节点只在**同路径、同类型**时继承旧 ID，其余分配新 ID。即使父目录显式保留 ID 并改名，无 ID 子文件也不会凭名称推断身份；应保留子文件原 ID，否则会出现缺失旧文件和新增文件两个未解决项。

显式 ID 优先。如果同路径的隐式继承 ID 已被草案其他节点显式占用，停止并要求为该节点明确指定 ID。显式更换文件 ID 即使路径不变，也表示旧文件缺失和一个无来源新文件。

## TreeSpec v1

```text
!tree 1

Research/ @d_research # 科研资料
  "2026 @ Research"/ @d_year
    "C# notes.txt" @f_notes # 文件也可以带说明
Inbox/ @d_inbox
```

- 首个有效声明必须是顶格的 `!tree 1`，此前可以有空行或独立 `#` 注释。其他版本和重复头部拒绝。
- 每层两个 ASCII 空格，不能跳级。字面 Tab 禁止，文件不能有子节点。
- 名称后的 `/` 表示目录；引号名的目录标记写在引号外，例如 `"C#"/`。
- 名称是单个非空路径段，拒绝 `/`、反斜杠、NUL、`.`、`..`；任何层级的 `.organizer` 名称保留。
- 普通名称可以含中文、空格、点和连字符。含 ` #`、` @`、引号，或以 `#`、`@`、`!` 开头，或需保留首尾空白时，使用 JSON 双引号字符串。JSON Unicode 和转义序列由 `serde_json` 解析，不手工解码；Unix 文件名中的换行/Tab 可通过转义表达。
- `@ID` 在全树唯一，字符为 `[A-Za-z0-9_-]+`。`d_` / `f_` 是生成约定，不强制用户短 ID 使用前缀。
- 同一父目录不能有同名节点，即使类型不同。不同目录可有同名文件。
- `#` 后的内容为说明，去掉首尾空白，保留内部文本；允许空说明。说明为不含控制字符的单行文本。
- 输入接受 UTF-8 和 LF/CRLF，输出统一 LF，不接受 BOM。Windows 编译时额外拒绝保留设备名、非法字符和尾部点/空格；目前运行验证以 Linux 为准。

规范化输出为每个节点补齐 ID，采用目录优先、文件其次、组内按名称的 Unicode 顺序区分大小写排序。保留名称、ID、父子关系和说明；不保留原文空行、独立注释或原排列。`render_tree(parse_tree(rendered)) == rendered`。

## Diff 报告与安全语义

报告含 `schema_version: 1`、`report_type: "tree_diff"`、`changes`、`effective_path_changes`、`unresolved` 和 `warnings`。没有 `operations`、磁盘扫描 ID、迁移前置条件或虚假的 digest。

`changes` 中每条记录包含 `id`、`node_kind`、`change`、`old_path`、`new_path` 以及前后说明。变化类型：

| 类型 | 含义 |
| --- | --- |
| `add_node` | 新增节点声明 |
| `remove_node` | 目标树遗漏节点声明，**不是删除命令** |
| `rename_node` | 同 ID 的本地名称改变 |
| `reparent_node` | 同 ID 的 `parent_id` 改变 |
| `update_description` | 说明改变，包括增加或清空 |

同一节点可同时产生多类变化。同 ID 的类型改变会报错。有效路径另行计算：祖先移动导致的子路径变化放入 `effective_path_changes`，其中 `inherited: true` 表示节点本身没有 rename/reparent，避免重复生成子节点改名。

旧文件 ID 消失时产生 `unresolved_missing_file`；目标新增文件 ID 产生 `unresolved_new_file`。M0 无磁盘扫描，所有新增文件都视为来源尚未验证，即使磁盘可能存在同路径文件也不能自动认可。删除目录声明会给出警告，不能据此清理真实目录。

报告按 ID、固定变化类型顺序输出。结构验证拒绝悬空父引用、文件父节点、环、重复路径/ID；Diff 还保守拒绝目录的新路径位于自身旧路径之下的情况。有效的路径交换可以展示为结构差异，但 M0 不决定其可执行性或操作顺序。

## 模块与设计决策

采用单 crate，核心库不依赖 CLI 或真实磁盘：

```text
src/
├── main.rs          # clap、输入/输出、M0 元数据初始化
├── lib.rs
├── error.rs         # 可区分的解析、验证、冲突错误
└── tree/
    ├── model.rs     # NodeKind / TreeNode / TreeSpec
    ├── parser.rs    # 逐行语法解析和 ID 规范化
    ├── validate.rs  # AST 不变量与相对路径计算
    ├── render.rs    # 稳定排序、JSON 名称转义、LF 输出
    ├── diff.rs      # ID 比较、继承路径变化、未解决项
    └── tests.rs     # 核心单元测试
tests/               # TreeSpec、Diff、CLI 集成测试
examples/            # old.tree / new.tree
```

公共 API 从 `organizer::tree` 导出：`parse_tree`、`parse_normalized_tree`、`parse_tree_with_base`、`validate_tree`、`render_tree`、`relative_paths`、`diff` 和 `compare_trees`。前三个规范化入口返回所有节点都具有非空 ID 的 AST。AST 可以由库调用方构造，因此渲染、路径计算和 Diff 都重新验证，不依赖解析器曾运行过。

缺失 ID 使用 UUID v5，由固定命名空间、TreeSpec 版本、类型、首次声明路径及碰撞盐确定性分配；预留所有显式 ID 和基线 ID，避免占用其他节点身份。这仅是**首次分配策略**，不是从路径推断文件身份的规则。持久化之后 ID 不随改名重算；不同管理根中首次生成的同路径节点可能得到相同 ID，不能拿它跨根关联文件。M1 的新磁盘对象应使用管理根隔离的随机/命名空间身份，并存入索引。

树结构存为带 `parent_id` 的平面节点数组，数组顺序没有语义。验证和渲染采用迭代遍历，避免深层树递归溢出。ID 比较预期为 `O(N+M)`，另加排序和完整路径物化开销；路径内存与所有路径总长度成正比。相对路径使用 `/` 文本分隔，不作为 shell 命令。

依赖限于 CLI、序列化、错误和 ID 生成；测试使用临时目录。未预建扫描、同步、LLM 或迁移执行的空壳模块。

## 验证

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
```

目前 Linux 上 42 项测试通过：7 项核心单元测试、12 项 TreeSpec、12 项 Diff、11 项 CLI 集成测试。覆盖中文/引号/转义、非法语法和路径、AST 环和悬空引用、规范化幂等、ID 冲突/继承、祖先路径变化、缺失/新增文件、示例 Diff、只读报告、拒绝覆盖和符号链接、10,000 节点宽树与 1,000 层深树。测试不需要网络或 LLM；首次下载构建依赖需要 Cargo registry 访问或本地缓存。

## 已知限制与下一步

M0 不包含扫描、SQLite、Git 提交、`status`/`sync`、可执行计划、`apply`、恢复、脚本导出或 LLM；也不包含 GUI、后台监听和 Agent 框架。`plan` 不接受 `--root`，因为它只读取明确指定的 Tree。当前没有大小写不敏感文件系统的冲突判定、符号链接磁盘扫描或跨平台执行保证。Tree 文件一次性读入内存。

元数据写入检查已有符号链接并原子拒绝覆盖叶子文件，但尚不提供抵抗并发替换父目录的描述符级保证；初始化也不是事务，写入失败可能留下部分元数据文件。应在用户控制且没有并发修改的目录内使用。M2 执行器必须独立实现根目录约束、无符号链接路径解析、no-replace 原子移动及持久化操作日志，不能直接把当前元数据 I/O 代码当作迁移执行器。

M1 建议按以下顺序推进：

1. 实现 `scan` 的完整性标记与文件清单：跳过 `.organizer`、符号链接和特殊文件，嵌套 Git 项目作为受保护目录；权限/读取失败必须阻止自动同步。
2. 引入 SQLite 身份索引，先匹配同路径同类型，再结合设备号/inode、元数据和历史确认唯一移动；复制分配新 ID，歧义标记 unknown/unresolved。
3. 完整化 `init` 并实现只读 `status`、可审计的 `sync`。已有基线被手工编辑、扫描不完整或有状态冲突时拒绝覆盖；仅内容改变不产生结构提交。
4. Git 仓库只建在 `.organizer` 内，只显式提交 `structure.tree`。覆盖新增、删除、移动、复制、不完整扫描和提交失败测试；定义现有 M0 初始化目录的升级流程。
