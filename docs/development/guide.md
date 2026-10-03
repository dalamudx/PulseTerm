# 开发指南

## 1. 仓库结构

```
.
├─ Cargo.toml               # workspace；依赖版本统一在 [workspace.dependencies] 中管理
├─ .mise.toml               # 固定 Rust / Node.js / pnpm 和辅助工具版本，定义开发任务
├─ package.json / pnpm-workspace.yaml
├─ crates/                  # 见 architecture/backend.md §1
├─ apps/
│  ├─ desktop/src-tauri/    # Tauri 应用；tauri.conf.json、capabilities/、icons/
│  └─ cli/
├─ ui/                      # 前端；见 architecture/frontend.md §2
├─ xtask/                   # cargo xtask <子命令>
├─ tests/
│  ├─ interop/              # docker compose：各种协议的服务端及互通测试用例
│  ├─ fixtures/             # VT 录制样例、二进制帧样例、密钥样例
│  └─ e2e/                  # Playwright
├─ assets/                  # 子集化字体、内置主题、Shell 集成脚本
├─ docs/
├─ deny.toml                # cargo-deny：许可证白名单与安全公告检查
├─ NOTICE / THIRD_PARTY.md  # 第三方署名
└─ .github/workflows/
```

## 2. 环境准备

| 工具 | 版本 |
|---|---|
| mise | 工具链统一入口；先安装 mise，再信任仓库配置 |
| Rust | 1.95.0（由 `.mise.toml` 固定），组件：rustfmt、clippy |
| Node.js | 22.23.3（由 `.mise.toml` 固定） |
| pnpm | 10.34.6（由 `.mise.toml` 固定，不再使用 corepack 管理版本） |
| Tauri CLI | 2.x（`pnpm tauri`） |
| 辅助工具 | `cargo-nextest`、`cargo-deny`、`cargo-fuzz`（可选）、Docker（运行互通测试） |

各平台依赖：
- **Windows**：Visual Studio Build Tools（MSVC + Windows SDK）、WebView2 Runtime。如果要从源码编译音频相关依赖，还需要 NASM。
- **macOS**：Xcode Command Line Tools。
- **Linux**（Debian/Ubuntu）：`libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev librsvg2-dev libayatana-appindicator3-dev libasound2-dev libudev-dev build-essential`。要测试 H.264，还需安装 `gstreamer1.0-libav` 或 VA-API 插件。

## 3. 常用命令

工具版本只修改 `.mise.toml`，不另建 `rust-toolchain.toml`、`.nvmrc` 或其他版本文件。库依赖仍在 Cargo / package manifests 中声明，提交 `Cargo.lock` 和 `pnpm-lock.yaml`；mise 管理执行这些依赖的工具。配置尚未信任前不要执行任务。

**当前可执行：**

```bash
mise trust .mise.toml              # 先审阅配置，再信任
mise install                      # 安装固定版本的工具
mise run bootstrap                # 依据 pnpm-lock.yaml 安装依赖
mise run check                    # Rust 格式/clippy/依赖分层 + TS 类型/Biome
mise run test                     # Rust 单元/属性测试 + Vitest
mise run probe                    # Rust 与 xterm headless 共享样例比较
mise run probe-pty                # Unix 真实 PTY、查询回写、尺寸与背压
mise run probe-pty-portable       # 三平台finite Rust helper：规范输入/Unicode/native尺寸/EOF
mise run probe-local-session      # 真实PTY→actor/registry/wire、授权ACK背压/End/重新捕获、resize/关闭join
mise run probe-software           # Unix vim/less/htop/tmux真实输出及input/resize恢复；Linux CI执行
mise run unicode-table            # 从锁定 Rust 表生成前端 Unicode 宽度
mise run unicode-table-check      # 只读漂移检查
mise run probe-unicode            # 16 组文本、多提供器与分块对比
mise run probe-snapshot           # 完整/屏幕恢复（Rust/xterm）、分歧矩阵与体积
mise run probe-attachment         # binary事务→actor/driver/历史/续流/Fence/End→headless callback ACK
mise run terminal-api-check       # 独立默认feature构建拒绝Echo及旧视口replay
mise run terminal-vendor-check    # 检查211个源文件仅有批准的只读getter差异
mise run terminal-vendor-test     # 独立锁文件运行上游178项测试
mise run bindings                 # 从 Rust 命令表生成 IPC 类型、命令与事件
mise run bindings-check           # 检查生成产物漂移，不覆盖源文件
mise run audit                    # Rust 许可证/安全公告/来源检查
mise run fmt                      # 统一格式
mise run desktop-check            # 打包前端并检查真实 Tauri 适配层
mise run desktop                  # 打开原生开发验证界面（不是完整终端）
mise run probe-channel            # native PTY/窗口授权/EOF ACK/Commit/Data idle Marker及精确屏幕修复，两轮headless+两轮可见DOM及7组测量
```

`probe-pty` 当前在 Unix 运行，依赖系统 `/bin/sh` 和 `stty`，不需要显示服务；输出 `target/probes/pty.json`。监督器最长 25 秒，交互 helper 自带 10 秒截止时间。三平台 CI 新增 `probe-pty-portable` 和 `probe-local-session`，不依赖Windows的stty；仅有Linux运行证据，不能把注册成功当作ConPTY验收。Windows helper只测canonical输入和native尺寸回读，不测raw/Win32键盘、child尺寸query或随附conpty.dll。

`probe-channel` 报告schema 4要求两轮headless及两轮DOM的native session运行、隐藏peer窗口跨owner拒绝、真实Destroyed撤销且会话存活、EOF同边界digest以及原有7组传输测量。每次约3.1MB Unicode输出含截断UTF-8尾部；客户端一次延迟ACK 250ms，额度最多1帧/8212字节（不含6字节outer header），callback完成才ACK。相同label重建与旧nonce/generation ACK也在真实WebView验证。复用同一sink协议运行`@xterm/headless`和`@xterm/xterm` 6.0.0 DOM renderer；DOM每帧在parser callback及两次animation机会后ACK（不是compositor回执），最终12行DOM文本逐行与buffer核对。Tauri动态页nonce通过公开documentOverride授予可信renderer style，无nonce拒绝，不放开unsafe-inline；helper固定且40秒截止，不允许用户指定shell或命令。新增有限`probe_session_input`端点（只接受固定字符/≤1KiB UTF8）：两轮DOM由textarea合成ASCII键盘、中文/emoji composition和Enter，通过onData/串行有界队列/generation+lease授权后在PTY显示INPUT_OK；headless走同队列但不是键盘。报告要求inputChecked、keyboardChecked、compositionChecked及pasteBlocked；合成事件不等于OS键盘/IME验收。前端InputQueue含活动IPC最多32项/8KiB、一秒截止，超时不自动重发；paste始终阻止，host撤销/错误按generation detach。每轮DOM连续3次ResizeObserver自动适配（初始→104×12→60×10→104×12，尺寸来自cell/content box，50ms去抖），native ACK后按Reconfigured逐次重捕获；随后显式pause票据detach，容器隐藏、后台新增HIDDEN_SEED，100ms旧通道无帧，再resume并恢复seed。报告要求fitChecked/visibilityChecked、resizeCount=3/pauseCount=1及最终columns/rows，并核对native syscall成功次数；headless两项计数仍0。旧ACK/input/resize/detach/resume、旧帧与旧Promise结果不能影响新绑定。当前不是系统窗口最小化、真实持续拖动、跨屏DPI或全平台fit验收。权限/错误仅使用稳定标识，不把会话内容或底层主机信息写入报告。

无显示服务的 Linux 运行 `probe-channel` 需要 `xvfb`、`dbus-x11` 和 coreutils 的 `timeout`；探针监督整个 Xvfb/DBus/WebView 进程组，最长运行 180 秒。Linux 原生构建还需 GTK3/WebKitGTK 开发包，Windows/macOS 使用对应系统开发环境。原生依赖只在 `pulseterm-desktop` 的 `desktop` feature 启用，默认 headless 检查无需 GUI 库。

IPC 绑定提交在 `ui/src/ipc/gen/commands.ts`，禁止手改；Biome 排除此目录，tsc 仍检查它。变更命令或 DTO 后运行 `mise run bindings`。生成与实际 invoke/event 注册共用一个命令表，`bindings-check` 已接入三平台 CI；这些任务需对应平台的原生开发依赖，但无需启动显示服务。

输出在 `target/probes/`，详见 [验证记录](./validation.md)。使用未激活 mise 的 shell 时，单条工具命令加 `mise exec --` 前缀。CI 同样读取此配置，不在工作流里重复声明 Rust/Node/pnpm 版本。

**下列是目标接口，相关模块尚未实现，不应视为可执行命令：**

```bash
pnpm install                       # 安装前端依赖
cargo xtask codegen                # 生成 TS 类型绑定、设置的 JSON Schema 和 Capability 清单（改动 IPC 类型后必须执行）
cargo xtask fetch-conpty           # Windows：下载随附的 conpty.dll / OpenConsole.exe（会校验哈希）

pnpm tauri dev                     # 桌面应用开发模式（前端热更新，Rust 改动后重新编译）
pnpm --filter ui dev:web           # 只启动前端，使用模拟后端，在浏览器中开发界面
cargo run -p pulseterm-cli -- ssh user@host   # 调试 CLI

cargo nextest run --workspace      # Rust 测试
pnpm --filter ui test              # Vitest
cargo xtask interop up && cargo nextest run --features interop   # 协议互通测试（需要 Docker）
pnpm e2e                           # Playwright + tauri-driver（Windows / Linux）
cargo xtask bench                  # 端到端性能基准（启动时间、内存、cat 1GB）

mise run check && mise run audit   # 提交前运行：格式、clippy、依赖分层、TS 与许可证检查
pnpm tauri build                   # 本地打包
```

## 4. 代码规范

### Rust
- edition 2024；代码格式必须符合 `rustfmt`；clippy 开启 `-D warnings`，并额外启用一部分 pedantic 规则（在 `[workspace.lints]` 中统一配置）。
- 除 `pulse-platform` 外，所有 crate 都开启 `#![forbid(unsafe_code)]`。写 `unsafe` 代码时必须附上 `// SAFETY:` 注释说明理由。
- 库 crate 用 `thiserror` 定义错误；`anyhow` 只能用在可执行程序和 xtask 中。除测试外禁止 `unwrap` / `expect`（`clippy::unwrap_used`）。
- 异步代码中禁止执行阻塞操作。PTY、解码等阻塞任务放到专用线程或 `spawn_blocking` 中。长期运行的任务必须持有 `CancellationToken`。
- 日志使用 `tracing`，禁止用 `println!`。会话相关的代码必须在会话的 span 内执行。**禁止记录密码、密钥和终端内容**；敏感字段使用 `Redacted<T>` 类型包装。
- 对外公开的 API 必须有文档注释。协议和状态机代码不直接做 I/O（sans-I/O），并配有单元测试。

### TypeScript / React
- 开启 `strict` 和 `noUncheckedIndexedAccess`；禁止 `any`（确有必要时用 `unknown` 再做类型收窄）。
- 使用 ESLint（包括 react-hooks 和 import 路径限制规则）和 Prettier；也可以换成 Biome，二选一。
- 只允许在 `ipc/` 中调用后端；终端和桌面数据不得进入 React state（见 frontend.md §3）。
- 只写函数组件。视图控制器和布局树操作写成不依赖 React 的普通 TS 代码。
- 界面文字必须使用 i18n key；提交前的检查会标出未翻译的文案。

### 通用
- 提交信息遵循 Conventional Commits（`feat(ssh): …`、`fix(term): …`）。scope 使用 crate 名去掉 `pulse-` 前缀，或前端的功能模块名。**提交信息只描述技术或产品变更，禁止排期、日期、Sprint、周数或里程碑安排等描述。**
- 发现代码、分析符号引用/调用关系及依赖影响优先使用 Astrolabe；写入前读取当前文件，写入后请求诊断并运行针对性编译与测试。语言服务器不可用时不得把空诊断视为通过，也不得未经授权自动安装。
- 采用主干开发：功能分支保持短小，通过 PR 合并到 `main`，采用 squash 合并。PR 要引用对应的功能 ID（例如 `SSH-04`）。
- 改动影响架构时，先在 `docs/architecture/decisions.md` 登记 ADR；修改 IPC 协议、数据库 schema 或 action id 时，要在 PR 中单独说明兼容性影响。

## 5. 测试要求

| 改动类型 | 最低要求 |
|---|---|
| 协议解析、状态机 | 单元测试 + proptest；自己写的解析器要加 fuzz 目标 |
| 终端状态和快照 | 快照重放一致性测试必须通过；新增支持的终端模式要加上对应的测试样例 |
| IPC 帧格式 | 更新 `tests/fixtures/frames/`，并确认 Rust 和 TS 两端的测试都通过 |
| 新协议或新认证方式 | 在 `tests/interop` 中增加对应的服务端和测试用例 |
| 界面交互 | 组件测试；关键流程要有端到端测试 |
| 性能敏感路径 | 运行 `cargo xtask bench` 并把结果贴在 PR 中。不允许超出 features.md §18 的预算 |

## 6. CI

| 工作流 | 触发时机 | 内容 |
|---|---|---|
| `ci.yml` | PR / 推送到 main | 在 Win / mac / Linux 三个平台上运行：fmt、clippy、nextest、依赖分层检查、cargo-deny、TS 类型检查、lint、Vitest、前端构建与体积检查、检查 codegen 产物是否最新 |
| `interop.yml` | 每晚 / 手动 | 协议互通测试（Linux 上的 Docker）；在自托管的 Windows 机器上测试 RDP |
| `bench.yml` | 每晚 | 启动时间、内存、吞吐、回显延迟的基准，和历史数据对比；退化超过 10% 时自动开 issue |
| `e2e.yml` | 每晚 / 发布前 | Playwright + tauri-driver（Windows / Linux） |
| `release.yml` | 推送 tag | 三平台打包、签名（Windows Authenticode；macOS Developer ID + 公证）、更新包签名（Tauri updater 密钥）、生成发布说明 |

## 7. 发布

- 版本号遵循 semver。v1 之前的版本为 `0.x`，小版本号的变化可以包含不兼容改动。
- 分 stable 和 beta 两个渠道，各自有签名的更新清单。更新包的签名私钥只保存在 CI 的密钥配置中。
- 安装包形式：Windows 用 NSIS（安装到当前用户），并把 CLI 放在 `bin\` 目录下、加入用户 PATH；macOS 用 dmg 并提交 Homebrew cask（不上架 Mac App Store），CLI 通过菜单中的“安装命令行工具”在 `/usr/local/bin` 创建链接；Linux 用 AppImage / deb / rpm。
- 安装时创建 `pulse` 别名前，先检查 PATH 中是否已有同名命令，有就提示用户，不覆盖。
- 发布产物和更新清单（Tauri updater 的 `latest.json`）托管在 GitHub Releases；更新地址可在设置中改为内网地址。
- 代码签名：Windows 使用 OV 证书或 Azure Trusted Signing（新证书需要积累 SmartScreen 信誉）；macOS 需要 Apple Developer Program 账号做签名和公证。证书和账号的申请见 decisions.md 待决事项。

## 8. 合规红线

1. **Netcatty 是 GPL-3.0 项目**：只能参考它的功能设计和公开的问题描述，**不得复制、改写或逐行参照它的代码**。实现时不要同时打开它的源码。
2. **MIT / Apache 项目**（NovaTerm、Windows Terminal 等）：借用代码时保留版权声明，并登记到 `THIRD_PARTY.md`。
3. **MPL-2.0 项目**（noVNC、serialport）：不修改它们的源文件。如果修改了，必须公开被修改的文件。
4. **禁止引入 GPL / AGPL 依赖**，由 `cargo-deny` 和前端许可证检查拦截。新增的许可证类型需要在 PR 中审批。
5. **不内置专利相关的编解码器**（例如 OpenH264）。
6. **microsoft/terminal** 仓库禁止提交 AI 生成的公开贡献，不要向该仓库提 PR 或 issue。
7. **字体**：内置字体大多是 SIL OFL-1.1。子集化属于修改；如果字体声明了保留字体名（Reserved Font Name），修改后必须改名。选型时优先选择没有保留字体名的字体，随包附带 OFL 许可证全文。
8. **主题和配色库**：内置的第三方主题和 iTerm2-Color-Schemes 都要保留原作者署名，并登记到 `THIRD_PARTY.md`。

## 9. 安全开发要求

- 密码、密钥等敏感数据使用 `Zeroizing` / `SecretBox` 保存，用完尽快释放。新增传输敏感数据的 IPC 命令，必须经过安全评审。
- Tauri 的 capability 配置采用最小权限原则，只开放必要的接口；CSP 禁止执行内联脚本和加载远程脚本。
- 所有解析外部输入的代码（协议数据、导入文件、设置文件）都要设置长度和深度上限，并进行 fuzz 或边界测试。
- 依赖升级时，关注 `cargo-deny` 和公开安全公告（advisories）中的安全问题。
- 数据目录权限设为仅当前用户可访问（Unix 0700；Windows 只给当前用户授权）。
- 来自远端的终端内容一律视为不可信：OSC 52 剪贴板、OSC 8 链接、窗口标题、通知类序列都按 SEC-08 / SEC-09 处理；后端不应答可能泄露信息的查询（例如窗口标题回报）。

## 10. 开源协作

- 仓库根目录提供 `CONTRIBUTING.md`（开发流程、提交规范、如何运行测试）、`CODE_OF_CONDUCT.md`、`SECURITY.md`（通过 GitHub Security Advisories 私下报告漏洞，不在公开 issue 中披露），以及 issue 和 PR 模板。
- 贡献采用 DCO（提交时加 `Signed-off-by`），不要求签 CLA；依据 Apache-2.0 的约定，贡献沿用项目的许可证。
- 用户文档站点（REL-05）和代码放在同一仓库的 `site/` 目录，随版本发布。
