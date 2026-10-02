# 架构总览

> 本文定义系统的整体结构和各部分之间的约定，细节见 [backend.md](./backend.md) 和 [frontend.md](./frontend.md)。
> 本文中的决策编号（ADR-xxx）见 [decisions.md](./decisions.md)。

## 1. 设计原则

1. **后端是唯一数据源，前端只是视图**。会话、终端状态、桌面帧缓冲、传输任务都保存在 Rust 侧。WebView 刷新、崩溃、标签在窗口之间移动，都不应该丢失会话。
2. **控制和数据分开传输**。控制类操作走 Tauri 命令（JSON）。终端字节、画面、码流这类数据，每个会话各用一个独立的二进制通道，**不经过全局事件广播，不做 base64 编码**。
3. **传输过程绝不丢数据**。数据来不及处理时，压力一路传回 PTY 或 SSH 窗口（背压）；后端和前端之间按额度（credit）控制发送。前端跟不上时，可以跳过中间画面，直接恢复到最新状态（用快照或合并的变化区域）。后端保存的数据始终完整。
4. **所有入口共用一套能力定义**。界面、快捷键、命令面板、CLI、MCP、插件都调用同一个 Capability 注册表，每个操作只定义一次。
5. **协议和连接路径解耦**。协议实现只接收一个已经建立好的字节流；这个字节流怎么连到目标（直连 / 代理 / SSH 跳板）由 Route 负责，可以任意组合。
6. **凭据不离开后端**。前端只拿到凭据的引用（`SecretRef`），密码和私钥只在后端解密，用完尽快从内存清除。
7. **平台差异集中处理**。各平台特有的代码（ConPTY、窗口材质、全局键盘钩子、提权、钥匙串等）只放在 `pulse-platform` 中，其他 crate 不写 `#[cfg(windows)]` 这类业务分支。
8. **协议和状态逻辑不直接做 I/O（sans-I/O）**。协议解析和状态机只处理传入的数据，不自己读写网络或文件，这样可以脱离真实服务器做单元测试和 fuzz。

## 2. 进程模型

```
┌────────────────────────────────── 主进程 pulseterm-desktop ─────────────────────────────────┐
│  Tauri 主线程（系统事件循环、窗口）                                                          │
│  tokio 多线程运行时：会话 actor、协议、Route、文件传输、CLI 通信服务、存储                       │
│  专用线程：PTY 读写、桌面解码、音频播放（cpal）、全局键盘钩子                                    │
└────────▲────────────────────────────────────────────▲────────────────────────────────▲───────┘
         │ Tauri IPC（命令 / 通道 / 事件）                │ 子进程                      │ 本地 socket
┌────────┴─────────┐                    ┌───────────┴─────────────────┐   ┌──────┴────────┐
│ WebView 进程（每窗口） │                    │ shell / OpenConsole.exe        │   │ pulseterm CLI  │
│ React 界面 + xterm.js │                    │ 外部 RDP 客户端 / 外部编辑器    │   │ 短生命周期进程  │
│ + noVNC + WebGL     │                    │                                │   │ JSON-RPC       │
└────────────────────┘                    └────────────────────────────────┘   └────────────────┘
```

- **单实例**：同一用户同时只运行一个主进程（用 Tauri `single-instance` 插件实现）。多个窗口属于同一个进程，所以标签可以在窗口间移动，会话不受影响。
- **提权实例**（Windows）：是另外一个以管理员身份运行的主进程，使用独立的单实例锁和 socket，和普通实例彼此隔离（ADR-006）。
- **CLI**：是一个独立的小二进制，不加载 WebView。主进程在运行时走 JSON-RPC；主进程没有运行时，就先拉起主进程再把请求交给它。
- **后续演进**：可以把会话从主进程拆到独立的 `pulse-mux` 守护进程，实现关闭界面后会话仍然保留。会话层从一开始就不依赖 Tauri 类型，为此留出空间。

## 3. 分层

```
┌─ 前端 ui/ ─────────────────────────────────────────────────────────────────────────┐
│ 窗口框架  │ 工作区（布局树） │ 功能模块（terminal/desktop/files/vault…）│ ViewHost │
│ stores（zustand）│ ipc/（生成的类型绑定 + 帧解码）│ design/（主题、组件）                 │
└───────────────────────────────────────────────────────────────────────────────┘
                 │ 命令（JSON）│ 通道（二进制帧）│ 事件（低频）
┌─ apps/desktop/src-tauri ───────────────────────────────────────────────────────────────────┐
│ 命令入口（只做参数转换和转发）、窗口管理、托盘、更新、deep link、单实例、启动参数 │
└─────────────────────────────────────────────────────────────────────────────────┘
┌─ pulse-app（应用服务层，不依赖 Tauri）───────────────────────────────────────────────────────┐
│ SessionRegistry │ Capability 注册表 │ Settings 解析 │ Vault 服务 │ 传输队列 │ 隧道管理 │ CLI 通信服务  │
└──────────────────────────────────────────────────────────────────────────────────┘
┌─ 会话与协议 crate ───────────────────────────────────────────────────────────────────┐
│ pulse-term │ pulse-desktop │ pulse-fs │ pulse-tunnel │ pulse-route                            │
│ pulse-ssh │ pulse-rdp │ pulse-vnc │ pulse-telnet │ pulse-serial │ pulse-mosh(P2)             │
└─────────────────────────────────────────────────────────────────────────────────┘
┌─ 基础 crate ──────────────────────────────────────────────────────────────────────────┐
│ pulse-core（领域模型、ID、HLC、错误码）│ pulse-store（SQLite、迁移、加密）│ pulse-ipc（协议类型）│
│ pulse-platform（各平台差异）                                                              │
└─────────────────────────────────────────────────────────────────────────────────┘
```

**依赖规则**（由 CI 中的 `cargo xtask check-deps` 检查）：
- 只能上层依赖下层，同层的协议 crate 之间不能互相依赖。例外是 `pulse-route` 依赖 `pulse-ssh`，因为跳板功能需要 SSH。
- 只有 `apps/desktop/src-tauri` 可以依赖 `tauri`。`pulse-app` 及其下层都不依赖 Tauri，这样可以被 mux 守护进程和测试代码直接使用。
- 只有 `pulse-platform` 可以写 `unsafe` 代码和平台条件编译；其他 crate 开启 `#![forbid(unsafe_code)]`。

## 4. 核心概念

| 概念 | 归属 | 说明 |
|---|---|---|
| **Window** | 前端 + src-tauri | 一个原生窗口，类型有 `main`、`drop`（下拉终端）、`popout`（不带 WebView 的桌面窗口） |
| **Tab / Pane** | 前端工作区 | 标签内是一棵分屏树，每个叶子节点是一个窗格（Pane），窗格内放一个视图（View）。布局会保存到后端，用于会话恢复 |
| **View** | 前端 | 会话的显示层，包括 TermView、RemoteCanvas、VncView、FilePane，以及 Vault、设置等普通页面。**视图可以随时销毁，会话不受影响** |
| **Session** | 后端 | 一个活动的连接实例，分四类：`Terminal`、`Desktop`、`File`、`Tunnel`。每个会话由一个独立的 actor 管理，有自己的状态机 |
| **Attachment** | 后端 | 把一个视图和一个会话关联起来，包含通道和发送额度。一个会话可以没有视图（在后台），也可以同时有多个视图（P2：镜像显示） |
| **Transport** | 后端 | 终端会话使用的字节流接口：写入、调整尺寸、关闭、读取输出、查看状态。实现有 LocalPty、SshShell、Telnet、Serial、Mosh、Echo（测试用） |
| **Route** | 后端 | 连到目标的路径，由若干跳组成（`Vec<Hop>`），每跳可以是直连、HTTP 代理、SOCKS5 或 SSH 跳板。结果是一个可读写的字节流 |
| **Host / Endpoint / Identity** | Vault | Host 是一台服务器；Endpoint 是它的一个协议入口；Identity 是可以复用的登录身份（用户名 + 凭据引用） |
| **Capability / Action** | 后端 | 系统中可以被调用的操作，包含 id、参数定义、作用范围和风险等级；各个入口都从这里调用 |

## 5. 关键流程

### 5.1 打开一个 SSH 标签

```
UI: 用户在 Vault 双击主机
 → cap_invoke("session.open", { endpoint_id, target: { window, pane: new } })
 → pulse-app 解析设置：按主机 → 分组 → 全局的顺序，确定身份、Route 和终端参数
 → SessionRegistry.spawn(Terminal, SshShell)：马上返回 session_id，连接在后台建立
     └ Route.dial()：逐跳建立连接 → SSH 握手 → 校验主机密钥
       （遇到新指纹、需要输入 MFA 码或密码时，发出 Prompt 事件并等待用户回应）
 → UI 创建窗格和 TermView，调用 term_attach(session_id, cols, rows, channel)
 → 后端先发一帧 Snapshot（刚开始时只有“连接中”的提示），之后持续发 Data 帧
```

### 5.2 终端输出与背压

```
PTY/SSH ──(有界队列，满了就阻塞读取 → 压力传回源头)──► 解析器 + Term 状态(完整) ──► 历史归档 / 录制
                                                          │
                                    有关联的视图？ ──否──► 不发送（后台会话）
                                                          │是
                                    还有发送额度？ ──是──► 把不超过一帧（约 8ms）的数据合并成 Data 帧发出
                                                          │否（前端跟不上）
                                    暂停发送，标记视图为“落后”。等前端确认收完已发数据后，
                                    发一帧 Snapshot 替代积压的中间数据，然后恢复发送 Data 帧
```

- 前端每处理完一帧（在 xterm 的 `write` 回调里），就调用 `term_ack(bytes)` 告诉后端，后端据此恢复发送额度。默认额度窗口是 512KB。
- **输入走单独的命令，不经过输出队列**，所以大量输出时按下 Ctrl+C 也不会被堵住。

### 5.3 视图释放与恢复

- 标签进入后台后超过一定时间，或缓存的视图数量超过上限，前端就销毁该视图并调用 `term_detach`。后端会话不受影响。
- 切回该标签时，前端重新创建视图并调用 `term_attach`。后端先发 Snapshot，内容包括屏幕、最近 N 行历史、光标和各种终端模式，然后继续发 Data 帧。
- 前端刷新和标签跨窗口移动，都走同一套恢复流程。

### 5.4 RDP 画面

```
Route 字节流 → TLS / CredSSP → IronRDP 状态机 → 解码线程 → 帧缓冲（完整画面）+ 变化区域累积
  → 按帧发送：变化区域的 RGBA 数据；或者未解码的 H.264 数据（P2）
  → 前端处理完后确认；待确认的帧超过 2 个时，把新的变化区域合并后只发最新画面
     （跳过中间帧，但帧缓冲始终是完整的）
输入：UI → desktop_input（批量发送扫描码和鼠标事件）→ 作为快速通道输入事件发给服务器
```

### 5.5 CLI：`pulse ssh prod`

```
pulse（CLI）→ 读取运行时目录中的 token → 连接本地 socket
  ├ 连接成功 → 发送 JSON-RPC {method:"session.open", params:{target:"prod"}, token}
  │            → 由 Capability 注册表调度 → 窗口获得焦点并打开标签 → 返回 {session_id}
  └ 连接失败 → 启动 pulseterm-desktop --startup-actions '<json>'，并等待 socket 就绪
从 PulseTerm 窗格内运行时：环境变量 PULSE_PANE_ID 指明当前窗格，所以 `pulse split -v` 知道该在哪里分屏
```

## 6. IPC 规范（前端 ↔ 后端）

| 通道 | 用途 | 格式 | 频率 |
|---|---|---|---|
| **命令**（`invoke`） | 请求/响应类操作，包括用户输入 | JSON。Rust 和 TS 的类型定义由工具生成（tauri-specta，备选 ts-rs） | 中低频；输入按微任务合并后发送 |
| **通道**（`Channel`） | 单个会话的数据，只发给关联的视图 | **二进制帧**（`InvokeResponseBody::Raw`，前端收到 `ArrayBuffer`） | 高频 |
| **事件**（`emit`） | 全局低频通知：设置变化、Vault 数据变化、会话状态变化、需要用户处理的提示 | JSON | 低频。**严禁用事件传输会话数据** |

**二进制帧格式**（小端序）：

```
┌────────┬────────┬──────────┬─────────────────────┐
│ kind u8│ flags u8 │ seq u32  │ payload（按 kind 定义） │
└────────┴────────┴──────────┴─────────────────────┘
终端类：0x01 Data(VT 字节) 0x02 Snapshot(VT 字节 + 终端模式) 0x03 Title 0x04 Bell 0x05 Mark(OSC133) 0x06 Cwd 0x07 Exit
桌面类：0x20 Rect{surface,x,y,w,h,stride,RGBA} 0x21 H264{surface,ts,nal} 0x22 Cursor 0x23 Resize 0x24 Audio(保留)
文件类：0x40 ListChunk 0x41 Progress（按 10Hz 合并后发送）
VNC：  0x60 RfbBytes（服务器发来的原始字节，交给 noVNC）
```

- 帧格式在 `pulse-ipc` 中定义，前端的解码器在 `ui/src/ipc/frames.ts`。两端共用一组二进制样例文件做测试，确保编解码一致。
- 命令名的前缀表示所属模块：`cap_*`、`term_*`、`desktop_*`、`vnc_*`、`fs_*`、`vault_*`、`settings_*`、`window_*`。
- 统一错误结构：`{ code: "ssh.auth_failed", message, details?, retryable }`。前端按 `code` 显示对应语言的提示。
- Tauri 的权限配置只开放我们自己定义的命令；不启用 shell/fs 等通用插件的通配权限。

## 7. 数据与目录

| 类别 | 内容 | 位置（以 Linux 为例；Windows 为 `%APPDATA%\PulseTerm`，macOS 为 `~/Library/Application Support/PulseTerm`） |
|---|---|---|
| 用户设置 | `settings.toml`（只写用户改过的项）、`keymap.toml`、`themes/*.toml` | `~/.config/pulseterm/` |
| 数据 | `pulse.db`（Vault、应用状态、传输任务、连接历史） | `~/.local/share/pulseterm/` |
| 缓存 | 远程编辑时的临时副本、历史记录的落盘部分 | `~/.cache/pulseterm/` |
| 日志 | 按天轮转，默认保留 7 天 | `~/.local/state/pulseterm/logs/` |
| 运行时文件 | CLI socket、token（权限 0600） | `$XDG_RUNTIME_DIR/pulseterm/`；Windows 使用命名管道 `\\.\pipe\pulseterm-<用户 SID 的哈希>` |
| 凭据加密密钥 | 用来加密凭据的数据密钥 | 系统钥匙串；开启主密码时由主密码派生的密钥包裹保存 |

便携模式（REL-04）：程序目录下存在 `portable.flag` 文件时，以上所有目录都放到程序目录的 `data/` 下。

## 8. 横切关注点

- **日志**：使用 `tracing`，每个会话一个 span（`session{id,kind,host}`）；在输出日志前统一过滤敏感字段；前端的日志通过 `log_write` 命令写入同一个日志文件。
- **遥测**：默认不收集任何数据。崩溃报告需要用户主动开启（REL-03）。
- **多语言**：后端只返回错误码和参数，由前端翻译成提示文字。CLI 在本地内置英文和中文的提示文字。
- **版本兼容**：CLI 与主进程建立连接时先交换协议版本号；数据库 schema 和设置文件都带版本号，只向前迁移，迁移前自动备份。
