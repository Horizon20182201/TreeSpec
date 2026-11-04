# TreeSpec Organizer — M2

基于 Rust 的本地文件结构管理 CLI。已完成 **M0：TreeSpec v1 / ID Diff**、**M1：扫描 / 身份索引 / Git 结构同步**、**M2：安全迁移规划与执行**，无需 LLM Key。

需求见 [开发规范](docs/TreeSpec_Organizer_MVP_Development_Spec.md)。工具只管理结构元数据，不把用户文件正文写进 Git。`sync` 记录已经发生的磁盘变化；`plan` 根据稳定 ID 规划路径变更，`apply --yes` 才执行真实移动。

## 完整 M2 流程

需要 Linux、稳定版 Rust、本机 Git 和 C 编译器。M2 要求内核支持 `openat2`（Linux 5.6+）且文件系统支持 `RENAME_NOREPLACE`；能力不足时报错停止，无覆盖式 rename 降级。SQLite 随 `rusqlite` 编译。保留 `Cargo.lock`；本次验证使用 Rust 1.95、Clippy 1.97。

以下演示保留全部 ID，将 `Inbox` 整体改名为 `Archive`：

```bash
cargo build --locked
export PATH="$PWD/target/debug:$PATH"
demo="$(mktemp -d)"
mkdir "$demo/Inbox"
printf 'hello\n' > "$demo/Inbox/note.txt"
organizer init --root "$demo"

# 只编辑草案，保留现有 @ID；也可以用文本编辑器修改名称/缩进
sed 's/^Inbox\//Archive\//' "$demo/.organizer/structure.tree" \
  > "$demo/.organizer/drafts/new.tree"
organizer plan --root "$demo" \
  --from "$demo/.organizer/structure.tree" \
  --to "$demo/.organizer/drafts/new.tree" \
  --out "$demo/.organizer/plans/migration.json"
organizer apply "$demo/.organizer/plans/migration.json" --root "$demo" --dry-run
organizer apply "$demo/.organizer/plans/migration.json" --root "$demo" --yes
organizer history --root "$demo"
cat "$demo/Archive/note.txt"
```

也可以添加目录声明并把已有文件节点缩进到新目录下；保留文件 ID。规范中的论文分类案例由 [M2 集成测试](tests/migration_integration.rs) 自动验证：两个 `mkdir` + 两个 `move_file`，文件内容和 ID 均保留，Git 只增加结构提交。

`examples/old.tree`、`examples/new.tree` 用于独立文档比较，里面的示例 ID 不是任意新初始化根目录的真实 ID。实际迁移从该根的 `.organizer/structure.tree` 复制草案。

## 构建与完整 M1 流程

M1 的扫描与同步功能继续可用：

```bash
cargo build --locked
cargo run -- --help

# 使用新的临时目录演示，下面的 mv 是用户手工操作
demo="$(mktemp -d)"
mkdir "$demo/Inbox"
printf 'hello\n' > "$demo/Inbox/note.txt"
cargo run -- init --root "$demo"

mkdir "$demo/Archive"
mv "$demo/Inbox/note.txt" "$demo/Archive/note.txt"
cargo run -- status --root "$demo"
cargo run -- sync --root "$demo"
cargo run -- history --root "$demo"
```

构建后的程序为 `target/debug/organizer`。以上流程会保留 `note.txt` 的 ID，更新 `structure.tree` 和 SQLite，并增加一次 `reconcile:` 结构提交。原地修改内容后再次 `sync` 只更新扫描缓存，不产生结构提交。

独立比较示例：

```bash
cargo run -- validate examples/old.tree
cargo run -- normalize examples/new.tree
cargo run -- plan --structural-only --from examples/old.tree --to examples/new.tree
```

示例报告包含 6 条变化：新增 2 个目录、迁移 2 个文件、改名 1 个目录、更新 1 条说明；`unresolved` 为空。

## 当前命令

| 命令 | 行为 |
| --- | --- |
| `organizer init [--root PATH]` | 扫描已有根目录，初始化 Tree、SQLite 和专用 Git 仓库；默认当前目录 |
| `organizer status [--root PATH]` | 只读扫描并输出 JSON：added / missing / moved / changed / unknown / conflicts / warnings |
| `organizer sync [--root PATH] [--accept-unknown]` | 更新已确认的结构与索引，仅在结构变化时提交 Git |
| `organizer history [--root PATH]` | 查看本地结构提交历史 |
| `organizer validate TREE` | 只读校验 Tree；允许缺失 ID，不回写 |
| `organizer normalize TREE [--base OLD] [--out PATH]` | 补齐 ID、规范排序；默认 stdout |
| `organizer plan --from OLD --to NEW [--root PATH] [--out PATH]` | 检查基线与磁盘，生成含前置条件的迁移计划；无管理根时保留结构 Diff |
| `organizer plan --structural-only --from OLD --to NEW [--out PATH]` | 显式仅比较文档，不查找管理根或扫描磁盘 |
| `organizer apply PLAN [--root PATH] [--dry-run \| --yes]` | 默认只读预览；`--yes` 才执行，两个选项互斥 |
| `organizer export-script PLAN --out SCRIPT` | 生成正确 shell 转义的 CLI 包装脚本；不嵌入文件操作 |
| `organizer recover RUN_ID [--root PATH] [--finish-git]` | 默认只读检查；可重试已完成迁移的 Git 记录，绝不重放文件移动 |

未指定根目录时，从当前目录向上查找最近的 `.organizer`。`plan` 找不到管理根时保留 M0 的独立文档比较能力。`apply` 的根由当前目录或显式 `--root` 选择，必须与计划完全一致；不会从计划路径或内容自动切换根。`--root` 与 `--structural-only` 互斥。

退出码：正常完成为 `0`；I/O、解析、验证、执行失败或未完成迁移阻断命令时为 `1`；`status` 能生成报告且有不完整扫描、冲突、未知身份或待提交 Git 时为 `2`；`plan` 有未解决项也为 `2`。Clap 参数错误使用 `2` 并输出用法。普通磁盘变化本身不会使 `status` 失败。计划生成成功后，执行时仍须重新验证。

`normalize` / `plan --out` / `export-script --out` 只创建新输出，不覆盖已有文件或输入 Tree。输出父目录必须已存在；根目录与元数据输出路径拒绝 `..` 和已有符号链接祖先。不要用 shell 重定向覆盖输入文件。将草案、计划放在 `.organizer` 内；写到受管理用户目录会被扫描为新增文件，使旧计划过期。

## 计划、执行与恢复

迁移计划 JSON v1 包含真实 `scan_id`、规范化目标 Tree 全文、基线/目标的 SHA-256、根身份、索引代次、Git HEAD、全部节点的身份/属性前置条件、扫描边界、确定性操作序列及 `unresolved`。`plan_id` 是整个规范化计划（该字段置空）的 SHA-256；相同输入和同一快照得到相同结果，重新扫描有新的扫描 ID。摘要用于一致性检查，不是数字签名。执行时根据当前可信基线、磁盘和内嵌目标重新生成并比较计划，不信任外部编辑的操作列表，也不重新读取草案。

支持 `mkdir`、`move_file`、`move_directory`。优先整体移动目录，并在模拟目录空间中更新后代路径，避免重复搬移；混合迁入/迁出按依赖安排。目标槽位必须为空或已由先前步骤让出。缺失旧节点、新增无来源文件、受保护/跳过子树、受影响硬链接、循环路径交换均阻断，未解决计划不包含可执行操作。M2 保守要求保留所有旧目录，不实现可选的空目录删除。

`apply` 默认与 `--dry-run` 都只读：不创建操作数据库、不写日志、不提交 Git。`--yes` 取得独占元数据锁，再次检查基线和完整扫描，将全部步骤以 `pending` 写入 SQLite 后逐步执行。每步前后校验扫描及身份；成功后同步父目录并持久化 `done`。任何失败立即停止，保留已发生的文件移动，不自动回滚。完成后再次验证目标，才发布 Tree/索引和一次 `refactor:` Git 提交；纯无变化计划不增加 Git 提交。

真实操作使用根目录描述符、`openat2(BENEATH | NO_SYMLINKS | NO_XDEV)` 和叶名称，检查父目录身份/保护标记；禁止绝对路径、`..`、`.git`、`.organizer`、自身后代和挂载点穿越。移动使用原子 `RENAME_NOREPLACE`，即使目标在检查后被创建也不会覆盖；没有 copy+delete 或 shell 文件操作降级。新目录权限为 `0700`（受 umask 约束）。

```bash
# 在 apply --yes 之前，可导出并运行预览脚本；不隐含 --yes
organizer export-script "$demo/.organizer/plans/migration.json" \
  --out "$demo/.organizer/plans/migrate.sh"
(cd "$demo" && bash .organizer/plans/migrate.sh)

# 使用 apply 输出/报错中的实际 run_id
organizer recover run_UUID --root "$demo"
organizer recover run_UUID --root "$demo" --finish-git
```

恢复行为：

- **文件操作部分完成或崩溃**：后续 `status`/`sync`/`plan`/`apply` 报告未完成 run 并停止。`recover` 返回原计划、逐步状态、完成计数、错误、当前完整扫描和元数据发布标记，辅助人工核对；`pending` 可能代表操作已经落盘但尚未确认，绝不能盲目重试。M2 不提供文件步骤的自动续跑/回滚/清除未完成状态。
- **文件和元数据已完成，仅 Git 失败**：报错明确区分“迁移完成”和“Git 待记录”。解决 Git 错误后使用 `recover RUN_ID --finish-git`，重新检查目标、身份和 Git 前置条件，只补齐提交及确认。包括 HEAD 已更新、索引更新失败的情况，不重复提交或移动。
- **Tree/索引发布中断或 SQLite 恢复文件存在**：保留数据库、恢复文件及 `pending.json`，先备份元数据并人工检查；不自动删除标记或修复数据库。此时 `--finish-git` 拒绝继续。SQLite 日志本身无法安全只读打开时，恢复命令会明确报错要求人工检查。

执行成功后草案和计划仍保留供审核；它们会继续触发 M1 的待处理草案提示，归档后再 `sync`。完成的日志永久保留，索引代次和日志共同防止重复执行旧计划。

## 元数据、扫描和同步边界

```text
.organizer/
├── .git/                 # 仅跟踪 structure.tree；无远端
├── .gitignore
├── structure.tree        # 已确认的完整受管理结构
├── sync.log              # 元数据更新记录，不提交
├── drafts/               # 不提交
├── plans/                # 不提交
├── state/
│   ├── index.sqlite      # 身份、扫描缓存、基线与 Git 状态
│   ├── operations.sqlite # M2 首次真实执行时创建；计划、逐步状态与错误
│   ├── lock              # 进程共享/独占锁，崩溃后由操作系统释放
│   └── pending.json      # 仅跨 Tree/数据库更新期间存在
└── cache/                # 不提交
```

新建元数据目录权限为 `0700`。SQLite 保存 `logical_id`、`last_path`、类型、device/inode、大小、mtime、ctime、可用时的 birthtime、硬链接数、保护标记和 `last_seen_scan`；Tree 仍只保存名称、ID、父子关系和说明。

扫描逐项枚举目录，只读取元数据，不读文件正文。Linux 使用目录描述符与 `O_NOFOLLOW`，不递归跟随符号链接。内建跳过任何层级的 `.organizer`、根目录的 `.git`、符号链接及特殊文件；嵌套目录只要有 `.git` 标记，就作为受保护的独立目录节点，不进入内部，并在报告中提示。

权限错误、不可达目录、不可表达为 TreeSpec 的名称、非 UTF-8 名称、遍历中检测到的目录变化都会使扫描不完整。此时不推断 missing，不发布新快照。完整清单保存在内存，内存开销与节点数和路径长度有关，不随文件正文大小增长。

身份匹配：

1. 同路径、同类型且文件键/birthtime 不冲突时继承 ID。
2. 跨路径仅在同卷文件键一对一匹配时关联移动；文件还要求大小、mtime 一致且不是多重硬链接。不会用同名或相同内容猜测移动。
3. 复制或已确认的替换得到新 ID。每个管理根使用独立随机命名空间；新 ID 由命名空间、索引代次、路径和身份信息生成，因此只读 status 的临时新增 ID 可重复，并避免复用旧身份。
4. 文件键相同但身份关联不确定时显示 `unknown`，默认阻止 `sync`。审核后可用 `sync --accept-unknown` 明确接受“旧节点消失 + 新节点出现”；该选项不会确认猜测的移动，也不能绕过其他冲突。

外部删除仅从快照中移除并显示 missing，不执行真实删除，不承诺恢复正文。目录整体移动会延续已确认的目录和子文件 ID。说明按已有 ID 保留。硬链接按目录项分配逻辑 ID，存在歧义的改名不会强行关联。

`sync` 会检查两次扫描是否一致，再发布元数据。以下情况都会阻止同步：手工编辑已应用基线、数据库/根目录/HEAD 不一致、人工暂存修改、额外 Git 暂存路径、不完整扫描、已有受保护子树的范围变化、旧文件被符号链接或特殊文件遮蔽、未完成更新标记。

为避免静默覆盖草案，`drafts/` 或 `plans/` 有内容时也阻止同步。完成审核后，把这些文件归档到这两个目录之外再同步；建议仍保存在 `.organizer` 下，避免把归档当作新用户文件纳入扫描。未修改的 M0 `drafts/initial.tree` 不算待处理草案。`plan` 允许草案存在，但要求 `--from` 等于已记录基线且磁盘未变化。

## Git 隔离、失败与 M0 升级

Git 只在 `.organizer` 中初始化，且每个提交只包含一个普通文件 `structure.tree`。使用 Git plumbing 构造单文件快照，不递归 add。禁用 hooks、签名、内容过滤和全局配置，并清理继承的 `GIT_*` 环境，避免切换工作树或运行用户过滤器。结构提交使用本地身份 `TreeSpec Organizer <organizer@localhost>`，不修改用户全局 Git 设置，不连接远端。

SQLite 更新使用事务和 FULL 同步，Tree 使用临时文件及原子替换；两者之间由 `state/pending.json` 标记覆盖。M1 不宣称跨文件事务：

- **仅 Git 记录失败**：Tree 与 SQLite 已保存，数据库标记 `pending_git`。`status` 只读报告该状态；排除 Git 错误后运行 `sync` 重试。若提交已发布但后续步骤失败，会核对父提交和 Tree 内容后补齐状态，不重复产生提交，也不重复任何用户文件操作。人工暂存的冲突仍会阻断重试。
- **Tree/数据库发布中断**：保留 `pending.json` 或 SQLite 恢复文件并停止。先备份 `.organizer`，检查日志、标记里的前后 Tree、数据库和 Git；不能未经核对就删除标记或盲目重放。M2 的 `recover` 针对迁移日志，不会自动修复 M1 的发布中断。
- **人工修改基线**：先保存为草案，再恢复索引记录的基线。仅当没有待提交 Git 状态时，Git HEAD 才是可直接参照的已提交基线。

已有 M0 目录可重新运行 `init --root PATH` 升级，但只接受可识别的已完成 M0 布局、原始忽略文件、空状态目录和未修改初始草案。会保留旧日志与初始草案；已有 M1 数据、陌生文件、未完成初始化或用户草案均不会被覆盖。

## Tree 文档中的 ID

独立文档先规范化旧树再编辑：

```bash
cargo run -- normalize handwritten.tree --out old.normalized.tree
cp old.normalized.tree draft.tree
# 修改名称、缩进或说明，保留整棵子树原有 @ID
cargo run -- normalize draft.tree --base old.normalized.tree --out new.normalized.tree
cargo run -- plan --structural-only --from old.normalized.tree --to new.normalized.tree
```

对于已初始化目录，直接复制 `.organizer/structure.tree` 到 `drafts/` 编辑即可。不要直接编辑已应用的基线。

`plan --from` 和 `normalize --base` 要求旧树所有节点都有显式 ID。新树缺失 ID 的节点只按同路径、同类型继承旧 ID，其他节点分配新 ID。父目录改名后，无 ID 子文件不会凭名称继承身份，所以编辑草案时应保留所有已有 ID。显式 ID 优先；隐式继承与其他显式 ID 冲突时停止并要求明确指定。

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

独立文档结构报告含 `schema_version: 1`、`report_type: "tree_diff"`、`changes`、`effective_path_changes`、`unresolved` 和 `warnings`，不能交给 `apply`。它与上面的 M2 迁移计划是不同格式。

`changes` 中每条记录包含 `id`、`node_kind`、`change`、`old_path`、`new_path` 以及前后说明。变化类型：

| 类型 | 含义 |
| --- | --- |
| `add_node` | 新增节点声明 |
| `remove_node` | 目标树遗漏节点声明，**不是删除命令** |
| `rename_node` | 同 ID 的本地名称改变 |
| `reparent_node` | 同 ID 的 `parent_id` 改变 |
| `update_description` | 说明改变，包括增加或清空 |

同一节点可同时产生多类变化。同 ID 的类型改变会报错。有效路径另行计算：祖先移动导致的子路径变化放入 `effective_path_changes`，其中 `inherited: true` 表示节点本身没有 rename/reparent，避免重复生成子节点改名。

旧文件 ID 消失时产生 `unresolved_missing_file`；目标新增文件 ID 产生 `unresolved_new_file`。结构 Diff 不承担文件来源验证；M2 也不会凭同路径或同内容认定新旧文件属于同一身份。删除目录声明会给出警告，不能据此清理真实目录。

报告按 ID、固定变化类型顺序输出。结构验证拒绝悬空父引用、文件父节点、环、重复路径/ID；Diff 还保守拒绝目录的新路径位于自身旧路径之下的情况。路径交换可以展示为结构差异，M2 规划器将无法安排的循环标为未解决。

## 模块与设计决策

单 crate，按规范保持模块边界：

```text
src/
├── main.rs          # clap、命令编排、JSON 与文本输出
├── error.rs         # Tree / I/O / SQLite / 状态错误
├── tree/            # AST、Parser、Validator、Renderer、Diff
├── scan/
│   ├── inventory.rs # 只读枚举、扫描完整性、受保护路径
│   └── identity.rs  # 纯身份关联、变更与歧义报告
├── storage.rs       # SQLite 身份/缓存事务与保存状态
├── sync.rs          # init、status、sync、基线预检与重试
├── git.rs           # 专用仓库、单文件快照、历史
├── migration/
│   ├── plan.rs      # 类型、摘要、确定性命名空间模拟与排序
│   ├── validate.rs  # 严格 JSON/Tree、基线和扫描前置条件验证
│   ├── filesystem.rs # Linux 描述符与原子 no-replace 操作
│   ├── execute.rs   # 执行、最终验证、元数据发布与 Git 重试
│   └── journal.rs   # SQLite 操作日志与恢复检查
└── metadata.rs      # 元数据路径校验、锁、发布标记、日志
```

Tree 公共 API 从 `organizer::tree` 导出：`parse_tree`、`parse_normalized_tree`、`parse_tree_with_base`、`validate_tree`、`render_tree`、`relative_paths`、`diff`、`compare_trees`。`scan::scan` 返回带 `complete`、warnings、errors 的清单，`scan::identity::reconcile` 是可独立测试的纯匹配逻辑；`storage::load` 只读加载保存状态。

迁移 API：`migration::build_plan(&PlanningContext)` 是纯规划逻辑；`migration::plan(root, old, target)` 负责只读磁盘上下文；`validate_plan` 只读验证；`apply_plan` 执行（库调用方负责取得用户确认）；`journal::recover` 检查或仅补齐 Git。JSON 使用白名单枚举及 `deny_unknown_fields`，拒绝陌生操作和字段。

独立 Tree 文档的首次 ID 分配仍使用 UUID v5（类型、路径、版本、碰撞盐），只保证单文档规范化的确定性；不用于跨管理根关联真实文件。M1 实际扫描采用根隔离的身份命名空间和持续索引。持久化 ID 不随改名重算。

Tree 使用带 `parent_id` 的平面数组，存储顺序没有语义；验证和渲染采用迭代遍历。ID 比较和身份键索引预期为线性复杂度，另加排序、路径计算和扫描开销。所有路径都是数据，不拼接 shell 命令。

## 测试

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
```

Linux 上覆盖 TreeSpec/Diff、M1 扫描与同步，以及 M2 的真实迁移、全目录 dry-run 无写入、原子防覆盖、源变化、计划篡改、保护边界、循环阻断、故障日志、防重放、Git 失败重试和脚本转义。故障注入还覆盖 rename 已落盘而日志未确认的窗口。测试需要 Git，不需要 LLM 或网络服务；首次下载依赖需要 registry 访问或缓存。

## 已知限制与 M3 建议

- 扫描/迁移目前仅提供 Linux 实现；其他平台明确报不支持。M2 保守拒绝清单中任何节点跨挂载点（包括同设备 bind mount），即使该节点本次不移动。TreeSpec 和纯比较/关联逻辑保持平台无关。
- device/inode 仍可能复用；birthtime 可用时用于排除冲突，不可用时不能保证百分之百识别。原路径被编辑器以新 inode 替换时按新对象处理，可能产生身份结构提交。内容变化检测依赖大小和时间，不计算正文哈希。
- 扫描不是操作系统级原子快照。锁协调 Organizer 进程，不锁定外部编辑器；最后检查后仍可能发生外部修改。执行期间应保持管理根、祖先目录及元数据稳定，不支持恶意进程并发搬走已打开父目录的场景。描述符解析、防覆盖和前后检查不会把多步迁移变成文件系统事务；元数据发布仍假定 `.organizer` 由可信用户控制。
- 管理根改名或搬迁会被根路径/身份检查拒绝。没有大小写不敏感文件系统的完备冲突判定、管理范围变更工具、自动元数据恢复或内容备份。
- 规划采用简单的命名空间模拟，多轮依赖搜索和后代更新未做大型树优化；执行每步前后重新扫描全部受管理节点，适合 MVP，暂不针对百万级文件或长操作链优化。
- 不删除文件/旧目录，不跨卷复制，不实现路径循环的暂存交换、自动续跑、undo 或内容备份。受保护项目、跳过项和硬链接所在的移动子树会被保守阻断。复杂目录重排可能需拆成多份审核计划。
- 没有 LLM、GUI、监听或复杂 Agent 框架。

M3 建议按规范新增 `LlmProvider`、TreeEditor 和 FileMapper，先做 mock 测试，再接入可配置 OpenAI-compatible Provider。LLM 只产出受限 Tree 草案与已有 ID 分配；缺失/未知 ID 和 `review` 保留为未解决项，继续走当前计划校验、用户审核和执行器。文件正文默认不发送，按需上下文需有大小上限；API/网络失败不得影响手工规划。
