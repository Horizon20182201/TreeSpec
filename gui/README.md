# TreeSpec Organizer for macOS（GUI）

`organizer` 的 macOS 图形界面，Tauri 2 + Svelte 5 + TypeScript。设计与决定见 [GUI 设计](../docs/GUI_DESIGN.md)，第 17 节记录实现差异和验证情况。

## 准备

需要 macOS 13+、Xcode 命令行工具（`xcode-select --install`）、Rust 1.95+ 和 Node.js 20+。

```bash
cd gui
npm ci
```

## 开发与构建

```bash
npm run tauri dev                                          # 开发：Vite 热更新 + Tauri 窗口
npm run tauri build -- --target universal-apple-darwin     # 发布：.app 与 .dmg
```

签名与公证由 Tauri 读取环境变量完成：`APPLE_SIGNING_IDENTITY`（Developer ID Application 证书），以及 `APPLE_API_KEY`、`APPLE_API_ISSUER`、`APPLE_API_KEY_PATH`（App Store Connect API 密钥）。没有设置时生成未签名的构建，只适合本机测试；TCC 授权绑定签名身份，未签名构建的授权可能反复失效。

Bundle ID 为 `io.github.horizon20182201.treespec-organizer`。TCC 授权和钥匙串项都绑定它，发布后不要再改。

## 测试

```bash
npm run check && npm run lint && npm test                  # svelte-check、ESLint、Vitest
npx playwright install chromium-headless-shell && npm run e2e
cd src-tauri && cargo test --no-default-features           # Rust 核心；也会重新生成 src/ipc/bindings
cargo clippy --all-targets -- -D warnings                  # 含 Tauri 外壳（需要先 npm run build）
```

`cargo test` 用 ts-rs 把视图模型导出到 `src/ipc/bindings/`，不要手改这些文件；改了 Rust 类型后提交重新生成的文件。

`npm run e2e` 和 `npm run build:mock` 使用 `src/mock/` 中的内存后端，在普通浏览器里就能运行页面。用 `npm run build:mock && npx vite preview` 打开，加 `?scenario=clean|changed|unfinished|uninit|welcome` 切换示例场景。生产构建不包含它。

Tauri 的官方 WebDriver 不支持 macOS，真实窗口中的行为按 [GUI 设计 13.1](../docs/GUI_DESIGN.md#131-测试) 的清单手工检查。

## 目录

```text
src/                 # Svelte 前端：只显示 Rust 给出的视图模型，只传名称和 ID
├── ipc/             # 命令封装；bindings/ 由 ts-rs 生成
├── stores/          # 窗口状态与动作（按钮只打开确认工作表）
├── pages/           # 欢迎、概览、结构、整理（审核/编辑/计划/报告）、历史、恢复
├── sheets/          # 确认工作表：普通、隐私、强确认、守卫、设置
├── editor/          # CodeMirror 6：TreeSpec 高亮、只读审核标记
└── mock/            # 浏览器预览与 Playwright 用的内存后端
src-tauri/
├── src/*.rs         # 库 organizer_gui：会话、白名单、视图模型、设置、钥匙串（可在任何平台测试）
└── src/app/         # Tauri 外壳：窗口、菜单、IPC 命令、退出拦截（feature `app`）
```
