# TreeSpec Organizer

用自然语言整理文件，先看方案，再决定是否执行。

TreeSpec Organizer 是一个基于 Rust 的文件结构管理命令行工具。它把目录结构保存为可编辑的文本，你可以让 LLM 提议新的分类，也可以自己调整目录和文件的位置。确认迁移计划后，工具完成移动，并通过本地 Git 记录结构历史。

- **自然语言整理**：按主题归类资料、创建目录、调整目录名称和层级。
- **先预览后执行**：查看完整目标结构和迁移计划，明确确认后才移动文件；目标已存在时拒绝覆盖。
- **手工也能使用**：编辑结构文本即可规划迁移，无需模型服务或 API Key。
- **保留结构历史**：记录整理前后的目录结构；文件正文不会提交到工具的 Git 仓库。

## 安装

目前支持 **Linux** 和 **macOS**。Linux 需要内核 5.6+，macOS 需要 10.12+；两个平台都要求支持原子防覆盖移动的文件系统，以及 Rust 1.95+、Git 和 C 编译器。已在 macOS 26.6（Apple Silicon）与 Rust 1.99 上验证构建，SQLite 无需单独安装。

下载或克隆本仓库，进入项目目录后安装：

```bash
cargo install --path . --locked
organizer --help
```

macOS 首次构建如果缺少 C 编译器，先运行 `xcode-select --install`。首次整理桌面、文稿或下载目录时，系统可能要求为终端授予文件访问权限。首次构建需要下载依赖。若提示找不到 `organizer`，将 Cargo 的安装目录加入 `PATH`；默认目录为 `$HOME/.cargo/bin`。

## 开始整理

以下步骤在同一个终端中执行。整个流程是：**初始化 → 草案 → 预览 → 确认执行**。

### 1. 选择要管理的目录

把下面的路径替换为你已有的资料目录的绝对路径：

```bash
workspace="/path/to/Workspace"
organizer init --root "$workspace"
```

初始化会扫描目录，并创建 `.organizer/` 保存结构和历史，不移动原有文件。一个目录只需初始化一次；之后可用 `organizer status --root "$workspace"` 查看变化。

### 2. 生成一份整理草案

**使用 LLM**：配置支持 OpenAI-compatible Chat Completions 的服务，替换下面的地址、模型名和密钥：

```bash
export ORGANIZER_LLM_BASE_URL="https://your-provider.example/v1"
export ORGANIZER_LLM_MODEL="your-model-name"
export ORGANIZER_LLM_API_KEY="your-api-key"

organizer chat --root "$workspace" --classify \
  --out "$workspace/.organizer/drafts/new.tree" \
  "按主题创建子目录，将现有资料归类；不确定的文件保留原位"
```

这会生成 `new.tree` 和 `new.tree.review.json`：前者是完整目标结构，后者说明分类理由和待审核项。模型只提出建议，`chat` 不移动文件。服务需要支持 JSON 输出模式；具体协议要求见 [开发指南](docs/DEVELOPMENT.md#m3自然语言生成草案)。

`--classify` 允许对已有文件重新分类。如果只想修改目录名称或层级，可省略它，例如将请求改为“把 Inbox 目录改名为 Archive”。

**不使用 LLM**：复制当前结构作为草案，然后用文本编辑器修改：

```bash
cp -n "$workspace/.organizer/structure.tree" "$workspace/.organizer/drafts/new.tree"
```

草案大致如下；这里的 ID 仅作示意，实际编辑时保留文件中的原有 ID：

```text
!tree 1
Inbox/ @d_inbox
  notes.txt @f_notes
```

将 `Inbox` 改成 `Archive`，表示整体改名；改变节点缩进，表示调整归属。每层使用两个空格，目录名称后有 `/`。**保留所有已有节点和 `@ID`，只编辑草案**；添加新目录时可以不写 ID，由工具补齐。

两种方式任选一种。草案和计划使用新文件名，已有输出不会被工具覆盖；再次整理时换一个名字。

### 3. 审核草案和迁移计划

先阅读 `new.tree`；如果由 LLM 生成，同时阅读 `new.tree.review.json`。确认目标结构后生成计划并预览：

```bash
organizer plan --root "$workspace" \
  --from "$workspace/.organizer/structure.tree" \
  --to "$workspace/.organizer/drafts/new.tree" \
  --out "$workspace/.organizer/plans/migration.json"

organizer apply "$workspace/.organizer/plans/migration.json" \
  --root "$workspace" --dry-run
```

`plan` 列出需要创建的目录和需要移动的文件，`--dry-run` 检查计划是否仍可执行。这两步都不移动文件。

如果 LLM 草案有待审核项，或者你编辑了生成的草案，`plan` 会要求确认。检查文件归属、保留草案的审核注释及配套报告后，可用以下命令重新规划：

```bash
organizer plan --root "$workspace" \
  --from "$workspace/.organizer/structure.tree" \
  --to "$workspace/.organizer/drafts/new.tree" \
  --acknowledge-review \
  --out "$workspace/.organizer/plans/reviewed.json"
```

确认审核不会绕过同名冲突、源文件变化等检查。使用此命令后，后续预览和执行都应改用 `reviewed.json`。如果计划仍有 `unresolved`，先解决问题并生成新计划。

### 4. 确认执行

计划预览通过后，明确加上 `--yes` 执行：

```bash
organizer apply "$workspace/.organizer/plans/migration.json" \
  --root "$workspace" --yes

organizer history --root "$workspace"
```

完成后，文件会出现在目标位置，结构历史也会更新。执行期间请保持该目录稳定，避免同时移动或编辑其中的文件。

## 日常使用

| 想做什么 | 命令 |
| --- | --- |
| 查看新增、移动、缺失等变化 | `organizer status --root "$workspace"` |
| 记录你在文件管理器中完成的调整 | `organizer sync --root "$workspace"` |
| 查看本地结构历史 | `organizer history --root "$workspace"` |
| 检查草案格式 | `organizer validate "$workspace/.organizer/drafts/new.tree"` |
| 查看某个命令的选项 | `organizer chat --help`、`organizer plan --help` |

`sync` 记录已经发生的变化；要让工具按草案移动文件，使用 `plan` 和 `apply`。在受管理目录内运行时，大多数命令可以省略 `--root`。

完成或放弃一轮整理后，把 `.organizer/drafts/`、`.organizer/plans/` 内的草案、配套审核报告和计划归档到 `.organizer/archive/` 等其他子目录，保留 `drafts/` 和 `plans/` 目录本身。有待处理草案或计划时，`sync` 会停止，避免与未完成的整理混淆。外部文件发生变化后，先同步，再基于最新结构生成草案和计划。

## LLM 会读取哪些信息？

默认发送你的整理请求、候选文件和目录的名称、相对路径、ID 及说明，**不读取文件正文**。常见私钥、`.env` 等敏感候选和受保护的 Git 项目会被排除。名称和说明本身仍会发送给你配置的服务。

- **限定范围**：添加 `--scope DIRECTORY_ID`，只处理该目录子树；从结构文本中获取目录 `@` 后的 ID，可重复指定。每次最多 512 个上下文节点、256 个待分类文件，超限时需缩小范围。
- **按正文辅助分类**：同时添加 `--classify --read-content`，允许发送有限文本片段，单文件最多 4 KiB、整次最多 64 KiB。目前不提取 PDF 正文或标题；名称过滤也不能识别所有敏感文本。
- **使用本地模型**：把服务地址配置为 loopback 地址，例如 `http://127.0.0.1:8000/v1`；本地服务可不设置 API Key。远程服务必须使用 HTTPS。

还可通过环境变量限制调用：

| 配置 | 效果 |
| --- | --- |
| `ORGANIZER_LLM_NO_CONTENT=1` | 禁止读取和发送正文片段 |
| `ORGANIZER_LLM_LOCAL_ONLY=1` | 只允许连接 loopback IP 或 `localhost` |
| `ORGANIZER_LLM_DISABLED=1` | 禁用 LLM，继续使用手工整理流程 |

这些开关省略或设为 `0` 时关闭。没有 API Key 或模型服务不可用时，手工规划与执行仍可使用。

## 使用限制与问题处理

- 当前为 Linux/macOS CLI 版本，不支持跨文件系统迁移、删除文件或旧目录、自动撤销。符号链接会被跳过，嵌套 Git 项目受到保护；涉及保护边界或硬链接的移动可能被拒绝。
- 执行器在两个平台都使用原子防覆盖移动：Linux 使用 `RENAME_NOREPLACE`，macOS 使用 `renameatx_np(RENAME_EXCL)`。文件系统不支持时报错停止，不会退化为可能覆盖的 `rename` 或 copy+delete。
- macOS 会解析并记录 `/var`、`/tmp`、`/etc` 等系统路径别名后的真实根目录；根目录本身是符号链接时仍会拒绝。把输出写到系统别名之下时同样会解析真实目录。
- Git 只记录结构，不备份文件正文，也不会因切换 Git 提交自动恢复真实目录。当前也不支持直接搬迁已初始化的管理根目录。
- 执行失败时可能已有部分移动完成，工具会停止并保留记录。根据错误中给出的 `run_id` 使用 `organizer recover RUN_ID --root "$workspace"` 检查，不要直接重跑或删除 `.organizer/`。具体处理见 [恢复说明](docs/DEVELOPMENT.md#计划执行与恢复)。
- 命令主要输出 JSON；`chat` 或 `plan` 返回退出码 `2` 表示报告中仍有待审核或未解决项。模型拒答、网络错误、路径冲突等会明确报错，不会自动绕过检查。

## 更多文档

- [开发指南](docs/DEVELOPMENT.md)：完整命令、TreeSpec 语法、设计决策、安全边界与测试。
- [开发规范](docs/TreeSpec_Organizer_MVP_Development_Spec.md)：项目目标与阶段规划。
- [示例旧树](examples/old.tree) / [示例新树](examples/new.tree)：用于独立比较的结构文档；实际整理应从自己目录的结构生成草案。
