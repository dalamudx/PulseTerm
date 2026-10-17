# 后端架构（Rust）

> 前置阅读：[overview.md](./overview.md)。本文描述 crate 划分、运行时模型、各类会话的实现、存储与安全，以及 Capability 层。

## 1. Cargo workspace

```
crates/
  pulse-core       领域类型：Id(UUIDv7)、Hlc、ErrorCode、Host/Endpoint/Identity/Route 等模型、设置的 schema 类型。不做任何 I/O；当前 SshTarget/SshEndpoint 只验证有界元数据，SSH 与存储信任目标复用相同规范化，构造不表示拨号、指纹批准或执行权限
  pulse-ipc        前端和 CLI 共用的协议类型、二进制帧编解码、JSON-RPC 消息体
  pulse-platform   各平台差异：ConPTY 加载、窗口材质、全局键盘捕获、提权、钥匙串、系统设置读取、生物识别
  pulse-store      SQLite 访问、schema 迁移、凭据加密、应用状态、设置文件的读写和热加载
  pulse-route      Route 建立：直连 / HTTP CONNECT / SOCKS5 / SSH 跳板；SSH 连接池
  pulse-ssh        russh 封装：认证流程、known_hosts、Agent、各类通道（shell/exec/sftp/端口转发/x11）、PPK 解析
  pulse-term       终端会话：Transport trait、alacritty_terminal 状态、快照生成、历史归档、OSC 处理、录制
  pulse-pty        本地 PTY（portable-pty）、Shell 自动探测、集成脚本注入
  pulse-telnet     Telnet 协议（不做 I/O）+ Transport 实现
  pulse-serial     串口 Transport（tokio-serial）
  pulse-desktop    桌面会话的通用部分：帧缓冲、变化区域累积、帧发送控制、输入事件模型
  pulse-rdp        IronRDP 集成：连接、会话循环、各虚拟通道、外部客户端启动器
  pulse-vnc        为 noVNC 提供 TCP/Route 字节流（P1）；以后可接入 vnc-rs
  pulse-fs         文件系统接口（Vfs trait）及 SFTP/本地/FTP/opendal 实现；传输队列
  pulse-tunnel     端口转发规则的运行、SOCKS5 服务端
  pulse-cap        Capability 注册表、参数 schema、风险等级与确认流程
  pulse-app        应用服务层：组装以上各部分；SessionRegistry、设置解析、Vault 服务、CLI 通信服务
  pulse-mosh       (P2) Mosh SSP 客户端
  pulse-plugin     (P2) wasmtime 宿主
  pulse-mcp        (P2) rmcp 服务端
apps/
  desktop/src-tauri   bin: pulseterm-desktop（只做命令入口、窗口和插件配置）
  cli/                bin: pulseterm（安装时可创建别名 pulse）
xtask/                代码生成、依赖检查、下载 ConPTY、性能测试、打包辅助
```


Unix 工作区主机元数据产品入口复用 `ProfileReader` 的同一私有 SQLite worker，不另开 `pulse-store → pulse-ssh` 依赖。`workspace_hosts` 与 `workspace_host_save` 只接当前本地 document nonce／WindowToken 下的名称、host、port、username；保存使用原始 `PageLaunchScope`、单次5分钟捕获票据和实例绑定的目录版本 CAS。页面退休清票据，执行中 SQL 信用归实际工作直到退出；视图／编辑器关闭不会销毁共享库或本地 PTY，也不能承诺撤回已提交 SQL。前端仅单选择器／单四字段表单，所有元数据作为字面文本；选择器换表单还撤销同捕获旧 DOM，晚回执不能绘制或抢焦点。此入口不包含身份引用、凭据明文、指纹批准、网络连接、任意路径或 Shell 执行权限，Windows ACL 未验证的平台入口仍关闭。
**主要依赖**（版本统一在 `[workspace.dependencies]` 中管理）：

| 用途 | 依赖 |
|---|---|
| 异步 | tokio、futures、bytes、tokio-util |
| 终端 | portable-pty、alacritty_terminal、vte（解析 OSC）、encoding_rs（非 UTF-8 编码转换）、unicode-width |
| SSH | russh、russh-sftp、ssh-key、ssh2-config、pageant |
| 远程桌面 | ironrdp（按 feature 启用子模块）、sspi、cpal |
| 其他协议 | tokio-serial、suppaftp、opendal、tokio-socks |
| 加密 | rustls、argon2、chacha20poly1305、zeroize、rand |
| 存储 | rusqlite（bundled）、rusqlite_migration、keyring、toml_edit、notify |
| 序列化与类型 | serde、serde_json、schemars、specta / tauri-specta |
| 错误与日志 | thiserror（库）/ anyhow（仅用于可执行程序）、tracing、tracing-subscriber、tracing-appender |
| CLI | clap、interprocess（本地 socket / 命名管道） |
| 压缩 | zstd（历史归档） |

## 2. 运行时模型

- 使用 Tauri 自带的 tokio 多线程运行时（`tauri::async_runtime`）。`pulse-app` 只依赖 tokio 接口，不关心运行时由谁创建。
- **每个会话一个 actor**：一个独立的 tokio 任务，独占该会话的全部状态。外部只能通过消息和它交互，**不使用共享的 `Mutex<Term>`**。

```rust
pub struct SessionHandle {
    pub id: SessionId,
    pub kind: SessionKind,                     // Terminal | Desktop | File | Tunnel
    cmd: mpsc::Sender<SessionCmd>,             // 写入、调整尺寸、关联/解除视图、确认、关闭…
    pub state: watch::Receiver<SessionState>,  // Connecting | Prompt(..) | Connected | Reconnecting | Closed(reason) | Failed(err)
}
```

- **阻塞操作放到专用线程**：
  - PTY 的读写是同步接口，各用一个线程，用有界的 `mpsc` 和 actor 通信。初版已在 `pulse-app::spawn_local` 组装：setup放blocking pool，常驻read/write/control三worker由组合task监督join。actor带外cancel，read在renderer取消后继续discard/drain到EOF以支持ConPTY关闭；native resize回执后才更新parser，5秒无回执明确失败。只监督direct child，尚非process-tree/跨窗口生产attach。
  - RDP 解码是 CPU 密集型，每个桌面会话一个解码线程。
  - 音频和全局键盘钩子各用自己的专用线程。
- **关闭顺序**：应用退出时，先通过 `CancellationToken` 通知所有会话关闭并最多等待 2 秒；然后保存工作区布局和传输队列；最后关闭数据库。

## 3. SessionRegistry 与关联（Attachment）

```rust
impl SessionRegistry {
    async fn open(&self, req: OpenRequest) -> Result<SessionId>;          // 解析 Endpoint、身份和 Route，创建 actor
    fn get(&self, id: SessionId) -> Option<SessionHandle>;
    async fn attach(&self, id: SessionId, view: ViewId, sink: FrameSink, opts: AttachOpts) -> Result<()>;
    async fn detach(&self, id: SessionId, view: ViewId);
    fn list(&self) -> Vec<SessionInfo>;                                    // 供界面、CLI 和 MCP 使用
}
```

- 上述API示意尚非完整生产registry。当前 `pulse-app::registry::SessionRegistry` 已实现有界metadata、可信WindowToken的single-owner授权、backend generation、capture串行及晚完成隔离；state/input/resize/ACK/detach/close均检查owner。window关闭不关闭session，application shutdown才关闭所有handles；直接使用 `spawn_local` 的 native receipt 仍由 registrar 监督；可选的内部 `local_launch` 作业改由其后台监督者 join。Tauri runtime窗口token、Destroyed/Exit及local-origin命令授权已在finite helper的真实WebView组合探针中接入。跨窗口迁移/重新授权、多视图、任意用户会话的生产命令/权限和事件合并仍待实现。后端 `begin_local_launch` 仅对可信窗口预留生成子进程前的名额；窗口关闭时撤销提交权限，但在途容量直到持票者完成原生任务清理才释放。在 native setup 前的短锁 `LocalLaunchTicket::require_current` 可以拒绝已撤销票据，但关窗仍可发生在检查之后和 spawn 之间，必须保留提交时的再次校验与失败后的 join。失败的 `commit_local_launch` 保留票据以便先回收子进程；`local_launch::start_local_registered` 在活跃 runtime 中保留票据直至 setup/失败回收，并在一次性接受确认前发现丢失回执时关闭、join 并 reap 已登记会话；`start_profile_local` 只从当前 backend Profile 目录取程序和 shell 分类，再走同一预留与回执路径；`PageLaunchScope` 与 `start_page_profile_local` 可把可信原生 adapter 确认后的文档代际加入同一预留与提交锁，换页使旧票据失效而保留已登记会话，旧 scope 析构不得撤销新代际。固定 `login-probe` peer admission 在 Tauri 本地来源/WindowToken 与 PageTracker 当前文档 nonce 一起确认后激活并在导航中废止 scope，并通过 `start_page_profile_local` 启动后端固定短命 Bash Profile，试预留立即释放、真正启动仅持票至 supervisor 回收，异步等待不持 scope 锁；仅绑定 scope 与新建固定 PTY 的真实窗口四轮回执均已通过（8次固定 Profile 子进程 EOF、4次导航撤销；两轮 session + 两轮 DOM、7项测量）。后续完整 `check/test/probe/bindings-check` 通过；产品工作区命令尚未建立。生产页面尚未向用户暴露 Profile 身份选择或打开 shell 的命令，这些内部入口本身不认证真实页面或 Profile 授权，也不保证 runtime shutdown 后的阻塞工作与子进程树回收。详情见 [终端关联协议](./terminal-attachment.md)。
- `FrameSink` 是对发送渠道的抽象。桌面端的实现是 Tauri `Channel`；测试中是内存队列；将来 mux 守护进程中是本地 socket。
- 会话的生命周期只由 `open` 和 `close` 决定，**和视图无关**。没有视图关联时，会话照常运行。
- 会话元数据（标题、当前目录、状态、未读标记）改变时，以低频事件 `session.updated` 广播，按 100ms 合并。

## 4. 终端会话（pulse-term）

### 4.1 Transport

```rust
#[async_trait]
pub trait Transport: Send + 'static {
    async fn start(&mut self, size: TermSize) -> Result<()>;
    async fn write(&mut self, data: Bytes) -> Result<()>;
    async fn resize(&mut self, size: TermSize) -> Result<()>;
    async fn close(&mut self) -> Result<()>;
    fn reader(&mut self) -> &mut (dyn AsyncRead + Unpin + Send);   // 由使用方主动读取，因此天然有背压
    fn state(&self) -> watch::Receiver<TransportState>;
    fn features(&self) -> TransportFeatures;                       // 例如是否支持 win32-input-mode、是否可以重连
}
```

实现：`LocalPty`（pulse-pty）、`SshShell`（pulse-ssh）、`TelnetTransport`、`SerialTransport`、`MoshTransport`（P2）、`DockerExec`（P3）、`EchoTransport`（测试用）。

### 4.2 数据处理

```
reader ─► 有界队列(64 块 × 最大 64KB) ─► actor:
   0. Decoder（仅非 UTF-8 会话，TERM-17）：用 `encoding_rs` 把字节流转换为 UTF-8。解码器有状态，可以处理被切断的多字节字符。之后各步骤和前端只处理 UTF-8
   1. OscTap：从字节流中提取 OSC 7 / 9;9 / 133 / 52，生成当前目录、命令标记、剪贴板请求等事件
   2. alacritty_terminal::Term::advance(bytes)   ← 后端保存的完整终端状态
   3. History：把滚出热区的行压缩归档
   4. Recorder（可选）：写入 asciicast
   5. 对每个关联的视图：按发送额度决定是发 Data 帧、先积累，还是切换到“落后”状态
```

- **合并发送**：同一个视图的数据最多累积 8ms 或 64KB 后发一帧，以先到者为准。交互时的小输出（例如回显）要立即发送：前一帧发出已经超过 8ms，就立即发。
- **发送额度**：初始 512KB。前端处理完数据后调用 `term_ack`，归还对应的字节数。额度用完后视图进入“落后”状态，此时不再为它缓存数据；前端确认完已发送的数据后，后端发一帧 Snapshot，然后恢复正常发送。
- **数据不丢失**：第 1–3 步对每个字节都会处理。只有前端视图会跳过中间状态。

### 4.3 快照

- 快照**是一段 VT 序列**（类似 `tmux capture-pane -e`），前端直接交给 xterm 的 `write` 处理。这样快照不依赖前端用哪种渲染器。
- 内容包括：
  - 重置序列；
  - 最近 N 行历史（默认 2000 行）和当前屏幕，带完整的 SGR 样式和超链接；
  - 当前是否在备用屏及其内容；
  - 光标的位置、样式和是否可见；
  - 各类终端模式：DECCKM、鼠标上报、bracketed paste、焦点事件、自动换行、滚动区域、字符集；
  - 窗口标题。
- **正确性测试**：对任意输入 X，先让 `Term_A` 处理 X，生成快照 S；再让全新的 `Term_B` 处理 S，两者的屏幕格子和终端模式必须完全相同（用 proptest 生成输入，并用 vttest 等真实输出作为样例）。这是 S2 的核心验收项。

### 4.4 历史记录（scrollback）

- **热区**：`alacritty_terminal` 自己保存的 scrollback，默认 10k 行，供快照和最近的搜索使用。
- **冷区**：更早的行归档成“文本 + 样式段”的格式，每 256 行用 zstd 压成一块放在内存中，超过上限后写入磁盘缓存。前端可以通过 `term_history_page` 分页读取，全量搜索也在冷区上进行。
- **待验证（S2）**：alacritty 的每个格子占内存较多（估计每格 20 字节以上）；另外，它没有提供“某行即将被丢弃”的回调，要把行从热区搬到冷区，可能需要在每批数据处理前检查历史长度，或者维护一个补丁版。达不到“10 万行不超过 20MB”时的备选方案：缩小热区，或改用 `vt100`。

### 4.5 其他

- **终端内文件传输**（Zmodem / trzsz）：在 Decoder 和 OscTap 之前检测传输的起始标志（传输内容是二进制，不能做编码转换）。检测到后，把字节流交给传输状态机处理，前端显示进度（P2）。
- **输入**：`term_input(session, bytes)` 直接写入 Transport，不经过输出处理流程。非 UTF-8 会话先用同一种编码把输入转换回去。Backspace、Delete 的编码按 TERM-18 的设置转换。广播输入（WS-05）由 `pulse-app` 把同一份输入分发给多个会话。
- **win32-input-mode**：解析到终端发出 `CSI ?9001h` 时，将会话标记为使用该模式，并通过事件通知前端切换按键编码方式。
- **查询应答只由后端负责**：前后端各有一份解析器，如果两边都回答程序的查询（DA1/DA2、DSR 光标位置、DECRQM、XTVERSION、DECRQSS、OSC 4/10/11/12 颜色查询、CSI t 窗口报告），shell 会收到两份应答，导致乱码或程序误判。规定由后端的 `alacritty_terminal` 统一应答（经 `Event::PtyWrite` 写回 Transport），前端屏蔽 xterm 的应答（见 frontend.md §6）。为了正确回答颜色查询（vim 等程序靠它判断深浅背景），前端切换终端配色时，要把当前配色同步给后端。
- **字符宽度一致**：后端（`unicode-width`）和前端（使用同一锁定表生成的 SharedUnicodeProvider）对 CJK、emoji、ZWJ 序列的宽度判断必须一致，否则搜索命中位置、命令标记和快照会错位。两端固定使用同一 Unicode 版本，并用同一份宽度测试样例做一致性测试（S2）。
- **远端剪贴板请求**（OSC 52，SEC-08）：写入请求按用户设置处理（允许 / 询问 / 禁止）；读取请求一律忽略，不应答。
- **会话日志**（TERM-16）：和录制一样挂在处理流程的第 4 步。纯文本模式输出的是“已滚出屏幕的行 + 会话结束时的屏幕内容”，没有控制序列；原始模式直接写入字节流。

## 5. 本地 PTY（pulse-pty）

- 使用 `portable-pty`。Windows 上通过 `pulse-platform::conpty` 加载随应用分发的 `conpty.dll` 和 `OpenConsole.exe`（由 `cargo xtask fetch-conpty` 从官方 NuGet 包下载），失败时回退到系统自带版本。portable-pty 是否能加载指定的 dll 要在 S10 验证，不支持的话就在 platform 中自己封装一层。
- **Shell 自动探测**：扫描 PATH 和常见安装目录，用 `wsl.exe --list --quiet` 获取 WSL 发行版列表（输出是 UTF-16LE，需要解码）。探测结果作为动态 profile，已生成过的 profile 记录在应用状态中，用户删除后不会再自动出现。
- **已实现基础**：`pulse-pty::profiles`提供Unix只读元数据发现（bash/zsh/fish/sh/pwsh）、每kind一个稳定自动profile、隐藏状态刷新保留与可导出/恢复的删除tombstone。程序沿用发现路径，不将`sh`替换成symlink目标。PATH最多64个组件/64KiB、程序路径最多4KiB，忽略相对/空组件及无执行位文件。此处只判元数据候选，不保证当前用户ACL/挂载exec策略或程序内容合法，native launch仍可失败；需在阻塞worker上执行。`AutoProfileCatalog::prepare_launch`从当前目录确定程序和可信分类，`AutoProfileLaunch`只允许字面args/cwd/env/remove/mode，应用身份由后端context提供；隐藏只影响自动显示，可显式请求，删除/缺失/失效均拒绝，不回退。纯准备不执行程序、不创建能力、不保证metadata到spawn期间程序不变；原生启动仍可失败。设置持久化、用户自定义profile/默认选择、Windows/WSL发现与macOS登录启动尚未接入。
- **WSL列表解析基础**：`pulse-pty::wsl::decode_wsl_list`仅对成功原生命令的UTF-16LE stdout做有界严格解码，64KiB/64发行版/256UTF16单元名称，保留大小写、空格和顺序；非法surrogate/BOM/控制字符/重复记录拒整个snapshot。`WslDistribution::command`仅构造字面`--distribution`参数并走通用启动校验，不执行wsl.exe。原生命令定位、成功回执/超时、WSL argv解析互通与基于registry GUID的稳定profile持久化仍未接入，不把名字当GUID。
- **启动字段边界**：`PtyCommand::validate`在任何native PTY分配前拒绝NUL、非法env键/空cwd及128参数/128环境覆盖与删除键合计/128KiB显式字段预算；空参数和最后env覆盖保持字面语义，Debug只显示数量/是否设置，不显示程序、argv、cwd、环境键值。继承环境与OS-specific argv限制未计入该预算；这不是启动授权或完整敏感数据存储。
- **启动时的环境变量**：
  - `TERM=xterm-256color`、`COLORTERM=truecolor`、`TERM_PROGRAM=PulseTerm`、`TERM_PROGRAM_VERSION`；
  - `PULSE_PANE_ID`、`PULSE_SOCKET`（供 CLI 定位当前窗格）；
  - pwsh 加上 `-NoLogo`，并设置 `POWERSHELL_UPDATECHECK=Off`。
- **本地启动环境准备基础**：`pulse-pty::launch::prepare_local_command`纯构造defaults+profile overrides，profile可以改TERM/COLORTERM及普通env；TERM_PROGRAM/TERM_PROGRAM_VERSION/PULSE_PANE_ID/PULSE_SOCKET的任何ASCII大小写别名覆盖或删除均拒。`PtyCommand::env_remove`通过公开builder API在显式override前删除继承键，清掉父进程旧PULSE pair，无CLI绑定则保持不存在；当前pair仅由backend context注入。它不创建CLI endpoint或授予binding权限。Pwsh按可信分类默认前置-NoLogo/POWERSHELL_UPDATECHECK=Off，profile仍可覆盖更新检查；macOSlogin、Windows环境行为实测与production Profile入口未接入。
- **登录 shell（ADR-030）**：`LocalShellMode`/`prepare_local_profile_command`提供平台默认、显式普通和显式登录三种模式；macOS 平台默认只为可信 Bash/Zsh/Fish 请求登录，其余分类及其他平台保持普通模式，不推断未知程序的登录支持；profile 可覆盖。Bash 通过 `--login`，Zsh/Fish 通过 `-l` 请求登录，不改 argv0，不修改 portable-pty，不使用默认程序选择或回退。选定程序、cwd、环境及用户参数保持字面值；未知 shell、generic sh 与 pwsh 的自动登录请求明确拒绝。参数登录与 `-basename` argv0 不完全等价，特别是 `$0` 与检查其前缀的脚本；用户参数也可能改变启动模式，不把“插入参数”当任意 profile 下的登录状态证明。系统 `/etc/paths`、用户启动文件、macOS 实际启动与生产 profile adapter 仍待独立验证；普通模式只是不添加登录参数，不剥离用户明示的登录选项。
- **参数登录会话验证**：`mise run probe-local-profile-session`用禁用启动文件的已知Bash登录 bootstrap，只 `exec` 自有有限Rust helper。实际PTY→actor→注册层输入/ACK/有序EOF已在Linux验证，暂停ACK期间解析保持不变，续流精确接收2MiB固定burst；consumer仍是Rust验证器，不能据此认定生产Profile、真实DOM、启动文件或跨平台验收通过。另有显式`login-probe`特性与`mise run probe-login-window`：仅包装当前exe的固定helper，Bash实际检查登录状态后才exec，helper缺收据明确失败；Linux已通过真实Channel/DOM输入、resize、刷新、迁移与EOF组合。普通desktop不启用此特性，报告严格区分普通/登录模式，不开放用户Profile选择或任意shell权限；启动文件仍禁用，Zsh/Fish/macOS未据此实测。
- **登录公开API实验**：`mise run probe-login`已在Linux真实PTY验证`CommandBuilder::new_default_prog()+env(SHELL,选定程序)`能保持选定可执行路径并使用`-basename` argv0；fixture是自有有限Rust binary，不是读取用户启动文件的shell。默认builder禁止追加参数，SHELL不存在/无效可回退到passwd shell；因此不能将其视为完整自定义profile登录或fail-closed程序选择。用户已选择 ADR-030 的登录参数方案，不 patch portable-pty；此实验保留作 API 限制证据，不用于生产启动或将登录参数等同于 argv0 语义。
- **Windows 环境变量刷新**：新会话启动前，从注册表重新读取用户和系统的环境变量，再合并 profile 中的设置，使得安装新软件后不用重启 PulseTerm 就能用上新的 PATH。
- **Shell 集成脚本注入**（SHELL-03）：bash 用 `--rcfile`；zsh 用临时 `ZDOTDIR`；fish 使用 `XDG_DATA_DIRS` 下的 vendor_conf.d；pwsh 使用 `-NoExit -Command`。每种方式都要确保用户原有的配置文件仍然会被加载。`mise run probe-shell-startup`已用临时HOME和自有用户rcfile在Linux Bash5真实交互PTY验证配置/标量与数组prompt hook保留、幂等注入与退出码；脚本仅在xtask fixture，普通profile不自动注入。系统bashrc可能仍由平台Bash加载，此测试不证明系统配置隔离、实际用户配置、登录模式、其他shell或macOS兼容；OSC133只作不可信提示，不授予启动/输入权限。低层`TerminalProbe::take_shell_markers`提供有界A/B/C/D提示（128条/显式丢弃计数），复用ground pending而非新增无界解析器；只接收BEL或完整ST、D状态0..255可缺省，不收命令内容。提示不带revision/行坐标，可能早于同步输出绘制，也不在恢复快照中保留；actor现提供同turn lease/cancel校验的有界读取，注册层验证owner/generation并设独立500ms准入/250ms截止，额度跨换绑与移窗保留；失败交付还原提示，成功交付后撤销的迟到结果仍可能丢弃，因此不是exactly-once日志。sampled_at只是采样时actor状态，不是各提示的revision/行坐标。读取不写VT、不释放ACK信用、不清风险；`SessionTask`/`LocalSessionTask::join_with_receipt`保留结束后最后一批有界提示，旧join接口兼容。残留提示不补造EOF/关闭D，D状态与实际原生exit_code分开；固定peer页面现通过`probe_peer_shell_hints`只读typed IPC采样：同一run/精确窗口实例/当前page nonce/当前generation，重复页在采样前拒绝，await后重新授权；首次登录样例必须A/B/C三提示，普通样例与刷新页必须空，独立mandatory收据贯穿native/JS/xtask。DTO只含枚举、drop数与采样时revision/source/UTF8十进制坐标，不含命令文本；该检查不等于命令标记的DOM坐标渲染或生产脚本注入，不推导授权。

## 6. SSH 与 Route（pulse-ssh / pulse-route）

```rust
pub enum Hop {
    Direct,
    HttpProxy { addr, auth: Option<SecretRef> },
    Socks5   { addr, auth: Option<SecretRef> },
    SshJump  { endpoint: EndpointId },        // 使用连接池中的连接，打开 direct-tcpip 通道
    Command  { argv: Vec<String> },           // ProxyCommand：启动本地进程，用它的 stdin/stdout 作为字节流；支持 %h %p %r 占位符
}
pub struct Route { pub hops: Vec<Hop> }

#[async_trait]
pub trait Dialer { async fn dial(&self, route: &Route, target: (Host, u16)) -> Result<BoxStream>; } // AsyncRead + AsyncWrite
```

- **当前Route库**：`pulse-route::{Destination, Credentials, Hop, Route, Dialer, NetworkDialer, BoxStream}`已提供Direct/HTTP CONNECT/SOCKS5组合，返回协议无关`AsyncRead+AsyncWrite`。trusted backend配置才可创建；host/port先验证防CRLF，secret无Debug且拥有的认证暂存零化。hops≤8、HTTP头≤8KiB、整次dial≤30秒总截止，取消drop partial stream；认证拒绝/降级不直连fallback。HTTP支持全部2xx并严格状态/字段，SOCKS远端DNS及IPv4/IPv6地址格式；没有HTTPS代理TLS、SSHJump/ProxyCommand、凭据发现或UI/IPC入口。上述enum/async trait为完整目标架构示意，不代表所有Hop已经实现。
- **SSH 连接池**：以 `(endpoint, identity, route)` 区分连接，并做引用计数。同一主机的 shell、SFTP、端口转发和以它为跳板的连接共用一条 SSH 连接；最后一个使用者离开后延迟 30 秒再断开（SSH-06）。
- **当前复用验收阻塞**：锁定russh0.63.3的client CHANNEL_DATA/EXTENDED_DATA在连接循环里await有界channel.send；单通道收件队列满会阻塞同连接其它通道/控制处理。probe-ssh-pressure及HTTP→SOCKS后的probe-route-ssh-pressure都实测暂停ACK可停止远端生产者、但同连接control被阻塞，独立连接仍响应，ACK/撤视图/超时后control恢复。不能把连接池示意当生产策略已验收；解决单channel window-credit/分发隔离前，不得宣称SSH-06/SFTP/转发同连接互不阻塞。没有为绕过背压改用无限queue，也未变更russh或默默采纳默认多连接的新架构。

- **认证流程**：按顺序尝试 Agent 中的密钥、配置的密钥、密码、keyboard-interactive。需要用户输入时（MFA 码、密码、私钥口令），会话状态变为 `Prompt { id, kind, text }`，前端弹窗后通过 `session_prompt_reply` 回传。用户输入的内容用 `Zeroizing` 包装，用完即清除。
- **主机密钥校验**：先查自己的 `known_hosts` 表，再查 `~/.ssh/known_hosts`（只读）。密钥变化时会话直接失败，错误码 `ssh.hostkey_changed`，需要用户在界面中明确确认后才能更新。当前 `pulse-ssh` 完成纯拒绝策略、有界文本解析及 Unix 私有目录下已验证的只读文件打开，路径由可信后端显式提供。`pulse-store/ssh-trust` 是可选的后端-only SQLite `app_state` 文本记录：最多128个规范目标、64KiB 行，独立打开句柄与单调修订控制写入。存储层不依赖 `pulse-ssh`/russh；可选上层 `pulse-ssh/managed-store` 在连接前对全体记录解析公钥，有坏键时整源拒绝、不回退系统文件。真实 loopback 证明重开后应用 pin 优先于系统文件、变更 pin 不会因系统文件正确而降级。它仍不是计划中的正式 `known_hosts` 表；已补实际候选／原页 guard 与同事务完整源 CAS 的首次批准后端（见 `ssh-peer-evidence.md`），已有或变更 pin 不得首次覆盖，但真实 WebView 审批／页面写权限仍未接；尚未对父目录整个链条或并发修改提供完整 TOCTOU 保护，真实用户目录、正式表迁移、审批及生产连接均未接入。不得直接使用 russh 的上游 `check_known_hosts_path` 作为信任读取器：文件打开失败被视为无记录且 debug 日志可能包含完整行。
- **算法**：默认使用 russh 的安全算法列表，按主机可以开启旧算法（SSH-09）。
- **睡眠唤醒和网络变化**：`pulse-platform::power_net` 订阅系统的唤醒和网络变化事件。收到事件后，立即对所有远程会话发一次 keepalive 探测，失败的进入重连流程，不用等 keepalive 超时。
- **重连**（SSH-07）：连接断开后状态变为 `Reconnecting`，按指数退避重试。重连成功后开启新的 shell，在屏幕上插入一条“已重新连接”的分隔线，原有屏幕内容保留。

## 7. 桌面会话（pulse-desktop / pulse-rdp / pulse-vnc）

### 7.1 通用部分

```rust
pub struct Surface { id: u16, w: u32, h: u32, fb: Vec<u8> /* RGBA */, dirty: RegionAccumulator }
pub enum DesktopFrame { Rect{..}, H264{..}, Cursor{..}, Resize{..} }
pub enum DesktopInput { Key{scancode, extended, down}, Unicode{ch, down}, Mouse{x, y, buttons}, Wheel{dx, dy}, Sync{locks} }
```

- 发送控制按帧进行。待前端确认的帧超过 2 个时，只继续累积变化区域。前端确认后，把累积的区域合并成少量矩形，取当前帧缓冲的数据一次发出。
- 画面停止变化时不发送任何帧；视图不可见时也不发送，但会话继续解码，保持帧缓冲是最新的。
- 重新关联视图时，先发一帧整屏画面。

### 7.2 RDP

- **建立连接**：`Dialer` 建立字节流 → `ironrdp-connector` 处理协商、TLS（rustls，由 ironrdp-tls 适配）和 CredSSP（sspi，支持 NTLM/Kerberos）→ 进入活动会话阶段，由 `ironrdp-session` 驱动。
- **服务器证书**：第一次连接时让用户确认证书指纹，确认后存入 `trusted_certs` 表（SEC-03）。
- **画面解码**：在会话的解码线程中处理 bitmap、RLE、RemoteFX，结果写入 `Surface`。
  - 开启 EGFX（P2）后，实现一个只转发不解码的 `H264Decoder`（`PassthroughH264`），把码流作为 `H264` 帧发给前端。前端在连接时上报自己是否支持 WebCodecs，不支持就不声明 AVC 能力。
- **虚拟通道**：
  - 剪贴板（cliprdr）和系统剪贴板双向同步，在后端直接完成，不经过 WebView；
  - 分辨率动态调整使用 displaycontrol，尺寸变化做 200ms 防抖；
  - 音频（rdpsnd）由 cpal 播放（P2）；
  - 磁盘重定向（rdpdr）在 P2。
- **系统快捷键捕获**：前端调用 `desktop_capture_keyboard(session, on)`，`pulse-platform` 安装系统级键盘钩子，捕获到的按键直接转换为 `DesktopInput` 送给会话。窗口失去焦点时自动卸载钩子，并发送所有修饰键的抬起事件，避免远端出现“按键卡住”。
- **外部客户端启动**（RDP-08）：生成临时 `.rdp` 文件。密码不写入文件：Windows 上可以用 `cmdkey` 写入一个临时凭据，用完即删；其他平台由用户在客户端中输入。

### 7.3 VNC（P1：由 noVNC 解码）

- `vnc_open(endpoint)`：后端用 Route 建立 TCP 连接，之后只转发字节。
  - 服务器 → 前端：经通道发送 `RfbBytes` 帧；
  - 前端 → 服务器：`vnc_send(bytes)`。
  - 发送控制和终端一样按额度进行。
- VNC 密码的处理：noVNC 需要密码时会发出 `credentialsrequired` 事件，前端收到后调用 `vnc_credentials(session)`，后端从 Vault 取出密码返回。这是**凭据不进前端的唯一例外**，因为由 noVNC 做解码时无法避免。密码只存在于前端内存，用完即弃，并在安全文档中登记。以后切换到 vnc-rs 在后端解码时消除这个例外。

## 8. 文件会话（pulse-fs）

```rust
#[async_trait]
pub trait Vfs: Send + Sync {
    async fn list(&self, path: &VPath) -> Result<BoxStream<Vec<Entry>>>;   // 分批返回，大目录也能流式显示
    async fn stat(&self, path: &VPath) -> Result<Entry>;
    async fn open_read(&self, path: &VPath, offset: u64) -> Result<BoxAsyncRead>;
    async fn open_write(&self, path: &VPath, mode: WriteMode /* Create|Truncate|Append{offset} */) -> Result<BoxAsyncWrite>;
    async fn rename(&self, from: &VPath, to: &VPath) -> Result<()>;
    async fn remove(&self, path: &VPath, recursive: bool) -> Result<()>;
    async fn mkdir(&self, path: &VPath) -> Result<()>;
    async fn set_attr(&self, path: &VPath, attr: AttrChange) -> Result<()>;
    async fn realpath(&self, path: &VPath) -> Result<VPath>;
    fn caps(&self) -> VfsCaps;                                             // 是否支持修改权限、符号链接、断点续传…
}
```

- 实现：`SftpFs`、`LocalFs`、`FtpFs`（P2）、`OpendalFs`（P3）。
- **传输队列**：
  - 任务按树形组织：一个目录任务展开成多个文件任务。
  - 并发数按主机限制。大文件在 SFTP 上同时发出多个读写请求（例如 32 个 64KB 请求），提高吞吐。
  - 任务记录写入 SQLite（`transfer_jobs`）。续传时先比较文件大小和修改时间，再从已传完的位置继续。
  - 可以选择传输完成后校验：远端支持时用 `sha256sum` 等命令校验。
  - 进度按 10Hz 合并后通过通道发送。
- **拖入上传**：用 Tauri 的拖放事件拿到本地文件路径，由后端直接读取文件上传。**不在 JS 中读取文件内容**。
- **远程编辑**（FILE-04）：先下载到缓存目录，用外部编辑器打开时监听文件变化（notify），保存时自动上传。上传前检查远端文件的修改时间，发现冲突就提示用户。

## 9. 隧道（pulse-tunnel）

- `ForwardRule { kind: Local|Remote|Dynamic, bind: SocketAddr, target: Option<(Host,u16)>, via: EndpointId, autostart: bool }`。
- 每条规则是一个 Tunnel 会话，状态和流量统计通过低频事件上报。
- 动态转发自己实现 SOCKS5 服务端：支持 CONNECT 命令和用户名/密码认证，默认只监听 127.0.0.1。

## 10. 存储（pulse-store）

### 10.1 SQLite

- 启用 WAL 和 `foreign_keys`。写操作通过单一的写线程执行，读操作使用连接池；对外提供异步接口。
- schema 迁移使用 `rusqlite_migration`，迁移前自动备份到 `pulse.db.bak-<版本号>`。
- 初始实现：`pulse-store::StateStore`单blocking worker串行typed app_state读写，16个待处理请求，250ms SQLite锁等待，WAL/foreign_keys/FULL；已扩展自动Unix Profile偏好、受控SSH目标／信任元数据、设置／主题与本地Vault密文接口；不提供任意SQL或明文读写，读连接池尚未实现。工作区应用目录已有Unix受控准入，Windows ACL仍关闭。
- `AutoProfilePreferences` schema1严格校验未知字段/身份、重复、冲突、未来版本与4096字节行预算；写入使用IMMEDIATE事务先验证旧行，坏行不会被默认值或autosave覆盖。普通修改用typed `AutoProfileEdit`基于数据库当前状态原子修改；整行`save_auto_profiles(expected,value)`做逻辑状态CAS，不匹配返回Conflict，不允许无条件旧快照覆盖。响应只是该操作的提交快照，不保证之后没有新操作，不授予目录或启动权限。当前数据库user_version4（保留本地Vault／身份表，新增含UUIDv7/created_at/HLC/deleted与目录CAS的正式信任表，见 [SSH身份本地持久化](ssh-identity-persistence.md) 与 [SSH信任记录持久化](ssh-trust-persistence.md)）；未来schema/无版本既有表拒绝，升级前使用SQLite Backup API保存未checkpoint WAL状态，existing backup不覆盖，失败迁移事务回滚且保留backup。
- caller必须提供可信私有目录；Unix要求parent权限700且属于当前有效UID；DB及预存-wal/-shm/-journal要求权限600、同有效UID、regular file和nlink1，拒绝别名、symlink和只读意图覆盖；无自动chmod/chown，并保留SQLITE_OPEN_NOFOLLOW，但不等于完整目录链所有权/ACL/TOCTOU验收；Windows目录与ACL由未来可信平台适配器负责。取消未执行请求不写，已经执行的提交不能撤回，不自动重试；drop全部handle后join收据确认关闭；可信`StateStoreTask`可cancel/shutdown，drop owner也唤醒worker，满队列不用预留额度。仍持有handle的客户端立即失去准入，未执行请求关闭；已开始的原生事务仍可能提交，shutdown不充当回滚。
- `pulse-app::profile_state`在store与PTY之间映射固定5类身份；PTY不依赖store。隐藏状态可在本次尚未发现shell时保留，删除抑制不会因PATH/程序重装复活。`mise run probe-profile-store`用5个独立进程验证保存、两个并发editor、旧快照Conflict与行保持、重开和延迟发现；candidate仅固定元数据fixture，不执行用户shell；工作区Profile隐藏／恢复已使用当前页授权开放，不接受执行路径／参数／环境修改。`mise run probe-store-crash`用独立固定子进程验证未COMMIT/已COMMIT未checkpoint的真实强制退出恢复，仅测试fixture使用固定SQL，不新增运行时SQL接口；不等于断电或文件系统损坏验收。

- **需要参与同步的表都包含以下字段**（即使同步在 P2 才做）：

```sql
id TEXT PRIMARY KEY,          -- UUIDv7
created_at INTEGER NOT NULL,
hlc TEXT NOT NULL,            -- 混合逻辑时钟，用于按条合并时判断先后（LWW）
deleted INTEGER NOT NULL DEFAULT 0,  -- 删除标记（tombstone）
```

| 表 | 是否同步 | 说明 |
|---|---|---|
| `groups` `hosts` `endpoints` `identities` `ssh_keys` `snippets` `routes` `tags` `host_tags` `forward_rules` | ✅ | Vault |
| `secrets` | ✅（密文） | `id, ciphertext, nonce, kind`；其他表通过 `secret_id` 引用 |
| `known_hosts` `trusted_certs` | 可选 | 主机密钥和服务器证书的信任记录 |
| `app_state` | ❌ | 键值对：工作区布局、窗口位置、已生成过的 profile、“不再提示”等 |
| `transfer_jobs` `connection_history` `audit_log` | ❌ | 只在本机有意义 |

- **表之间的关系**：
  - `endpoints` 属于 `hosts`，字段 `protocol` 取值 ssh/sftp/rdp/vnc/telnet/serial/mosh；协议专用的选项放在 `options` 字段（JSON，带 schema 版本号）；
  - `identity_id` 和 `route_id` 可以为空，为空时从上级分组继承。
- **模型的组织方式**：连接参数、认证、外观、行为四类配置分开存放，每类单独管理版本，避免 Host 模型字段越堆越多。

### 10.2 凭据加密

```
DEK（随机 256 位，用来加密所有凭据）
  ├ 未设主密码：DEK 保存在系统钥匙串（keyring）
  └ 已设主密码：KEK = Argon2id(主密码, salt)；保存用 KEK 加密后的 DEK；系统钥匙串可缓存 KEK（用于“记住我”）
每条凭据：XChaCha20-Poly1305(DEK, 随机 nonce, 明文, AAD = secret.id || kind)
```

- 当前共享模型：`pulse-core::secret_ref::{SecretRef, SecretKind}`及`ssh_identity::SshPasswordIdentity`仅元数据，无Debug/serde或路径／凭据值；密码／密钥passphrase角色严格区分。`VaultSnapshot::secret_ref`派生presence，缺Vault／缺值／删除／错角色显式处理；旧快照的presence不表示当前权限。身份保存与目标消费授权仍待接。
- 前端只能拿到 `SecretRef { id, kind, has_value }`；修改凭据通过 `vault_secret_set(id, value)`，后端只接收、不回传。后端也不提供任何“读取明文”的命令。唯一例外是 VNC 密码，见 §7.3。
- Linux 上如果没有 Secret Service（例如没有桌面会话），强制使用主密码模式。
- 主密码模式下的应用锁（SEC-02）：锁定时从内存中清除 DEK，已经建立的会话不受影响。解锁之前不能发起需要凭据的新连接。

- **当前实现边界**：`pulse-store::vault_crypto`保留内存AEAD原语；另有后端密文编解码与私库CAS验证基础，详见 [Vault 密文持久化边界](vault-persistence.md)，不是完整Vault或授权服务。稳定`chacha20poly1305 0.11.0`仅开启alloc/zeroize；使用既有getrandom的可失败OS熵源生成32-byte DEK和每次独立24-byte nonce，AAD是预期UUID的16个原始字节拼接受限ASCII kind。单值最大64KiB；密文/nonce/元数据错误不会返回明文。密钥与明文没有Debug/Serialize，明文包装零化Drop及显式zeroize早清，lock丢弃cipher后拒绝操作。此处不承诺交换区、寄存器或core dump完全擦除。另有`vault_kdf::derive_validation_kek`固定验证原语：Argon2id v1.3/64MiB/t3/p1/32-byte、128-bit随机salt、密码1..4096bytes。单进程私有AtomicBool/RAII令牌只容纳1个CPUjob，裸派生与后台任务共享额度，try_reserve工作区和输出皆Zeroizing，不接受未认证header成本。它是同步且不可取消的验证profile，调用者必须离开GUI线程调度、清理借用密码、检查迟到结果的授权epoch；派生成功不表示密码正确，不等于解锁。`vault_wrap`另有固定验证内存包装：新建专用测试Vault以KEK加密独立随机32-byte DEK；恢复须先匹配可信预期Vault UUID及48-byte密文，再验证固定wrapped_dek kind的AAD/tag，成功后返回不可克隆/Debug/serde、绑定预期VaultID的AuthenticatedDek证明；错密码/篡改/换身份不生成替代key。该类型故意不提供持久序列化或header参数。`vault_lifecycle`仅提供backend两阶段内存可用性控制：RAII permit只发1个弱引用worker，认证证明存入控制器的可撤销candidate key，返回不含key的receipt；owner pointer/epoch/预期ID/当前permit确认之后才启用，失败外来确认不消费回执。lock/close/permit或receiptDrop清候选及active key，迟到worker/同ID重建/重复确认不能复活，计数不回绕，Mutex poison清key并关闭。内部Rust adapter仍需独立授权SecretRef，未注册明文IPC；不能撤回先前合法发给transport的副本，进行中的CPU job仍需监督和迟到投递清理。`vault_unlock`仅提供固定验证profile的后台任务：私有可转移RAII额度在spawn_blocking之前取得，进程最多1个queued/running CPUjob；密码转为有界Zeroizing拥有输入，无Debug/Clone/serde，原调用方仍需清理之前的副本。旧epoch/接收方已关闭会跳过未开始计算；正在运行的Argon2不可取消，许可Drop/timeout立即撤销scope但不能提前释放CPU额度。候选必须完成投递和worker join后、经当前permit显式确认才启用key；Cancel返回无abort能力的join观察收据，丢失/错误/迟到结果不重试不换key，sender失败或未观察receiptDrop清scope候选。它不验证用户/窗口/SecretRef权限，也不是生产成本策略；持久化编解码不升级解锁权限。本地schema2已接vault_envelopes/secrets/删除墓碑表与备份后的原子密文迁移；仍无系统钥匙串、生产创建／解锁和SecretRef窗口授权整合、应用锁或前端明文读取IPC，固定验证KDF不当作生产成本策略；原语及持久化不授予访问权限。
### 10.3 设置

- 分三层：编译进程序的 `defaults.toml` → 用户的 `settings.toml`（只保存改过的项）→ 分组和主机各自的覆盖配置（存在数据库中）。`SettingsResolver` 计算最终生效的值，并记录每一项来自哪一层（VAULT-07 在界面上展示）。
- 设置类型用 `schemars` 生成 JSON Schema，写入文件注释，编辑器可以据此补全；前端的设置页也由同一份 schema 生成。
- 用 `notify` 监听文件变化，热加载时发出 `settings.changed` 事件。保存文件时用 `toml_edit`，**保留用户原有的注释和顺序**。
- **当前实现边界**：`pulse-store::settings::SettingsResolver`纯函数处理编译defaults、用户、分组、主机覆盖，逐字段保留来源，显式false或等于默认值仍算当前来源。初始只含终端font_size（6–72）、font_family（最多128 Unicode字符、禁止CSS语法）、cursor_blink，沿用固定DOM样例的14/monospace/false。每层先校验，拒绝未知字段、重复键、未来schema和超64KiB文档；错误不回显原文。`settings-schema`生成`assets/schemas/settings.schema.json`，`settings-schema-check`只读检查漂移并纳入CI；defaults含本地schema注释。`settings_edit::set_terminal_setting`只在内存编辑已知scalar；既有值依据不可变解析器token span替换，保留旁侧注释、顺序和空格，新增值保持稀疏且返回前校验完整模型与64KiB预算。`settings_remove::remove_terminal_setting`仅内存删除三个已知scalar：使用同一不可变解析器key/value span，普通/点键删完整赋值，内联表另删一个必要逗号；只扫描parser-bounded inter-entry trivia且跳过comments里的逗号，不解析字符串/值。保留评论、空白、空表、其他字段和Unicode字节，缺项幂等；结果必须等于预期稀疏模型，恢复该字段的下层来源。`settings_file::SettingsFile`现在提供同步后端IO：只接受可信绝对私有目录和固定settings.toml/.settings.lock，Linux/mac路径使用pinned directory fd与no-follow openat、有效UID/700目录/600单链接regular文件；不自动修权限。独立try-lock守卫显式unlock（防fork/dup描述符保留锁），原文本/文件inode与时间戳比较防合作客户端旧快照覆盖。随机同目录exclusive temp写完fsync，首次安装用NOReplace、更新用rename，最后directory fsync；rename后收据/同步失败返回Uncertain，不可自动重发。只提交typed scalar更新/删除，没有任意路径、原始替换或密码API。配置坏/未来/超预算不重置，权限/目录或lock身份替换明确拒绝，未知orphan temp不清理；不能声称抵御同UID非合作编辑器在最终检查后竞争、完整目录链/TOCTOU或断电验收。当前只有Linux隔离文件与独立进程实测，mac已注册CI待结果，Windows fail-closed Unsupported；未读取本机用户配置。`settings_reload::SettingsReloadCache`另提供同步单读取者的last-good状态：owner拥有已准入SettingsFile，invalidations用一个dirty bool合并（不排任务），坏/忙读取保留旧patch及最后成功snapshot且不自动重试；comments或同值变化只更新保存基线，不发模型变化。删除恢复稀疏下层来源，显式false即使等于default也产生source delta；收据仅含revision与3个changed-field bool，无路径/原文。close禁止后续读取发布，revision耗尽关闭而非回绕。同步缓存自身不创建worker，后台调度见下节；尚无notify watcher、settings.changed IPC或DOM应用；CC0监视依赖待授权，不是生产热加载。生产目录选择、主机/分组实体及授权、设置页仍待接入，不能计完整VAULT-07。
- **后台用户设置源的当前实现**：`settings_source`先取全进程唯一I/O额度，再在blocking任务执行private目录准入/初读；初坏文件不创建fallback或owner。一个worker拥有file/cache，dirty一位合并，独立request/verification stamp防同值与ABA复活；stale成功不发布，latest rejection保留此前已发布last-good。weak client/parsed snapshot不传原文/路径/权限，watch仅最新静态状态；comments不推进模型revision，删除/显式false保留来源变化。close/Drop同步撤权并唤醒，不能abort实际I/O或提前释放额度，shutdown监督实退；panic/错误/耗尽fail-closed，Drop不隐藏Failed。Linux真实fixture/屏障实测，mac已注册CI待实测，Windows文件层Unsupported，未读真实HOME。底层current仍瞬时校验；`with_current`允许可信后端在当前source验证的短锁内执行同步元数据确认，禁止I/O、重入、await或返回/排队延后工作。联合确认的锁顺序固定source→registry→context；source身份、verification stamp、revision及完整稀疏user patch必须相符，不能以最终渲染字体或同semantic revision代替证明。`SourceBoundSettingsSnapshot`保存不可改写的source/view/context回执，trusted invalidate即使同内容也使旧回执非current；坏文件last-good只供观察、不授权新发布。窗口/绑定权限先检查并在锁内再确认。仍无notify/page设置端点/GUI应用，也不是跨file-DB事务或生产热加载。
- **组/主机覆盖的当前实现**：`settings_scope`用两个固定UUID命名空间复用schema1的app_state，不增加表或迁移；nil身份拒绝，同UUID的group/host不能串记录，身份不是host存在性或权限证明。记录含私有schema/kind/id、1至i64::MAX revision与三个已知稀疏设置，原始读预算8192bytes；坏/未来/重复/错scope/超预算不当empty。一个16请求worker串行读写，IMMEDIATE typed edit基于当前row，snapshot CAS验证scope/revision/内容；最后字段删除保留版本tombstone，no-op不递增，ABA与版本耗尽不能复活旧snapshot。取消前dispatch不写，执行事务和失去响应仍可能提交，不自动retry。`load_settings_layers`在同一DEFERRED SQLite snapshot读group/host；`resolve_patches`保持defaults<user<group<host与逐字段来源，所有低优先级也校验。没有任意SQL、TOML原文/评论入DB或GUI载荷；最后成功用户文件cache与DB pair是独立capture，不声称跨文件/数据库原子事务。未创建主机/分组实体、祖先继承或生产窗口授权。
- **配置上下文发布的当前实现**：`pulse-app::settings_context`提供非Clone生命周期owner、weak句柄、opaque capture/candidate及不可改写epoch的published snapshot，Rust-only，尚非窗口/主机权限。先取唯一context槽再读StateStore bounded queue；切换/invalidate使旧epoch stale，槽保留至读取完成/回执丢失，store执行中事务仍由单worker/16quota处理、丢失响应不回滚。捕获token独立递增，旧Drop和同epoch旧publication不能清新slot或复活；A→B→A不回绕。候选必须显式confirm且匹配owner/epoch/当前slot，foreign/closed/耗尽拒绝。不同target清旧host值，同target失败保留last-good但非current；owner close/Drop不关闭共享DB或PTY。用户patch和DB capture独立，DB变更需显式trusted invalidate，is_current仅瞬时检查，不是原子GUI应用许可或持续数据新鲜性；尚未直接接native/page设置端点、notify或settings.changed。
- **注册视图配置的当前实现**：`registry`的Binding独占SettingsContextOwner；可信后端先决定group/host权限再configure，frontend不能提供WindowToken或scope ID。begin/read前后/confirm与last-good/current观察均验证registry身份、实际WindowToken、SessionId与binding generation，opaque回执不可充作输入/ACK/修复权限。重配置复用owner，不释放旧槽或重置计数；detach、换绑、retire_window_views、transfer、close_window、shutdown及completion移除Binding时自动撤销context。registry mutex不跨await，短元数据锁在确认和只读捕获时持有；`read_current_view_settings_from_source`以source→registry→context锁顺序捕获当前已验证模型，无I/O/await，返回值只保证捕获时正确，不是持续新鲜性或GUI应用许可；视图退役不关闭actor/共享DB。is_current仍仅瞬时检查，GUI需active-view guard；新绑定必须重新由后端配置，不继承旧页载荷。五进程仍仅用registry token与Rust诊断消费者。新增显式`settings-probe`原生验证特性：默认desktop不链接固定配置工厂；Linux真实peer页使用实际WindowToken/page nonce及活generation，临时用户层18、组20、主机22依次精确核对来源。实际导航Started退役旧候选，新页重新确认组20；实际源窗口销毁后，目标主窗口Channel已完成DOM绘制/ACK且lease仍活跃时确认用户18。随后临时文件经过typed CAS修改为19并显式invalidate，source线程读取revision2，旧候选精准Source(Stale)拒绝，新候选方可确认。关闭source/store后actor仍Running。新增`nativeSettingsChecked`只表示这组后端固定夹具成功，普通v1/v2必须为false，不能替代GUI字体应用、设置RPC、自动文件监听或生产主机授权；没有读取真实HOME、任意路径/主机选择/写入IPC。
- **主题**（UI-01 / UI-07，格式见 frontend.md §10）：内置主题从 `assets/themes/` 编译进程序，用户主题放在 `themes/*.toml`，同样支持热加载。各种外部格式的导入器（VS Code JSON、Windows Terminal JSON、`.itermcolors`、Alacritty TOML、base16/base24 YAML、Ghostty）放在 `pulse-store::theme`，统一转换成 PulseTerm 主题格式后写入 `themes/`；原文件只读取，不保留。导入时计算前景与背景的 WCAG 对比度，低于 4.5:1 时附带警告信息返回给前端。导入器都是纯函数（输入文本、输出主题），每种格式配有测试样例。
  - 当前基础实现接受一个完整 Windows Terminal 严格 JSON scheme，以及下述 Alacritty／Ghostty 颜色子集，输入最多 64 KiB、名称最多 128 UTF-8 字节且无控制字符；18 个必需颜色只接受不带 alpha 的 `#RRGGBB`，缺项／重复／未知字段拒绝，不静默填默认值。`cursorColor` 映射 `cursor`，`purple/brightPurple` 对应 ANSI 5/13；可选颜色未提供时保持未指定。输出只含 `name` 与 `[terminal]` 的 TOML 片段，**不是完整 PulseTerm UI 主题**。WCAG sRGB 未舍入比值低于 4.5 才告警，颜色不自动调整。这些导入纯函数不执行文件读取／写入或 UI 调色；完整七格式适配与受控后端保存见下文及验证记录，导入 IPC、内置配色库、自动监听或 GUI 应用仍未计完成。
  - 当前主题文件保存另由 `theme_file::ThemeFile` 提供 backend-only 同步接口：可信私有目录、非 nil UUID 固定 leaf、完整文档校验及快照 CAS；共享设置文件的私有 IO 原子替换层，独立 `.themes.lock`，空快照亦绑定目录与主题。显示名不作路径；错误内容静态，快照不 Debug/serde。坏文件不重置，Uncertain 不重试；无目录发现、自动修权、路径／写入 IPC、目录监听或 GUI 应用，仍不是完整 TOCTOU／断电／跨平台验收。`theme_io`另绑定已打开文件的非Clone owner／weak handle，全局单后台job、enqueue前准入、closure退出才释放；queued取消跳过IO，关闭／丢回执／超时不回滚已开始写入，不自动重试。ready实际join后检查owner开；其结果仍仅捕获快照，不是窗口／GUI发布许可，私有open必须另由可信后端在非GUI线程调度。
  - Alacritty 仅接受完整 `[colors.primary]`／`normal`／`bright` 配色与可选 cursor/selection，支持 `#RRGGBB`／`0xRRGGBB`，magenta 映射 ANSI 5/13；动态 Cell 颜色、indexed/dim 扩展和其他用户配置拒绝，不近似。Ghostty 仅接受完整 ANSI 0..15、foreground/background 与可选 cursor/selection 的颜色文本子集，支持无引号六位 hex／#hex、空行及独立 # 注释；重复字段／下标、额外配置／文件包含／继承、动态色、扩展下标、inline 注释与非 opaque RGB 拒绝。三种格式共用校验和 WCAG 告警，颜色未提供时保持未指定。

  - `pulse-app::theme_context`现提供后端选源 epoch 与显式发布：非Clone owner、weak handle、不可构造/改写的候选，source identity 匹配 actual ThemeIoOwner 而非仅 UUID；同源 select 是无操作，替换 source（包括同 UUID 新 owner）、invalidate、A→B→A均使旧候选失效。不同 source 清 last-good；同源坏文件刷新仅保留 stale last-good，缺文件不回退默认主题。读取完不自动发布，必须匹配 owner/epoch/token 后显式 confirm；同 epoch 第二次发布亦撤销先前 published token。取消／丢回执／timeout不提前释放实际工作占位；ticket Arc 由真实 blocking 闭包及候选共同持有，async runtime 退出不能撤回仍执行的 IO，已真正取消的排队 job 可释放占位。close/Drop只撤销本 context，不关闭共享 file owner；计数耗尽 fail-closed。可信 source 信号需显式 invalidate，没有监听器或持续文件新鲜性；is_current只是瞬时状态，不是窗口、GUI、跨文件事务或主题应用权限。候选解析只针对有界已捕获文本；不读取目录、不暴露路径 IPC。

  - 注册层 Binding 现独占上述 theme context；可信后端须先授权文件／profile选择，再 configure_view_theme，前端不能传运行时WindowToken或文件路径。begin／ready前后（失败亦重查）／confirm与观测均验证 registry identity、WindowToken、SessionId、binding generation；registry锁不跨await、IO或外部回调。configure同源保留状态，新源不重置pending槽或计数。换绑、导航retire、移窗、active窗口关闭、reap与shutdown移除Binding时自动撤销context，不关闭共享ThemeIoOwner或actor。read_current_view_theme在短metadata事务捕获当前已确认文档；快照只表示该捕获时刻，GUI仍需active-view guard，不是持续权限或新鲜性。新绑定不继承旧页主题配置，须后端重配。只提供Rust API，没有注册主题路径／写入／选择IPC、生产主题工厂、CSS／xterm颜色应用或监听器。

  - `settings-probe`另有隔离的`window_theme_probe`固定私有主题工厂：复用实际page nonce与WindowToken/generation，在导航恢复及target Channel绘制ACK后的活view阶段验证A→B→A与源CAS/invalidate，再验证SourceClose不结束PTY。模型按完整ThemeDocument比较；待确认候选跨导航、移窗须精确Stale/Forbidden拒绝。`nativeThemeChecked`是强制诊断回执，默认v1/v2及其他未启用feature模式必须false；不是GUI已应用证明或新的主题IPC权限。真实窗口组合验收结果见validation记录。

  - 固定peer页面新增`probe_peer_theme_read/confirm/drawn`只读typed交付：仅`settings-probe`可信Rust工厂能排入一个challenge；调用须匹配实际peer WindowToken、run/page nonce、已captured/rendered页面及binding generation。opaque随机ticket绑定已发布的RegisteredThemeSnapshot；read输出≤4096字节独立v1 canonical投影，不含名称、路径、CSS源码或文件句柄。confirm/drawn均再次核验source receipt当前性；阶段不可重放。异步监督future退出时ticket身份RAII清slot，旧drop不删除新挑战；没有文件/写入/输入授权。默认工厂关闭，三typed命令无challenge即拒绝。
  - peer前端沿actual ordered Begin创建ViewScope，确认后经现有guard应用固定UI overrides并读回nonce样式，再报告drawn；不应用terminal palette。`nativePeerThemePaintChecked`强制区分真实双页交付与旧native模型回执，默认false。确认是瞬时检查，不是源文件→远端DOM原子事务或持续租约；源更新通知、目标main实际绘制与生产热加载仍待接入。

## 11. Capability 层（pulse-cap）

当前 `pulse-cap` 已提供有界只读目录与typed参数校验，不含执行handler、caller认证、目标权限或风险批准receipt。固定5Surface／Scope／Risk、64动作、16KiB单schema／64KiB目录和参数；同Rust类型生成schema与语义validate，根与嵌套object须闭合，重复ID不覆盖，source字段不由客户端扩展。参数大小用discard writer计数，无另造明文序列化buffer，错误为静态码。11项行为、strict、Exact诊断、完整check/test/probe与749TS通过；workspace复用锁内schemars1.2.2，旧版本checksum保持，审计仍原6license2maintenance。`Catalogue::validate_args` 成功只表示参数形状/语义与surface允许，**不是执行permission**。随后 `pulse-app::window_action::PreparedWindowAction` 只支持Window准备：上层先认证caller/surface和选实际目标，保存原PageLaunchScope Arc/registry、固定动作/args和300s不续期截止；非Safe须原页可信GUI确认一次，重复/foreign/换页/另窗/remove/错动作/过期拒。`take_args` 消耗准备仅返回参数，并不授launch/input/nativeIO或持久session权限；实际effect仍必须同原scope及自身commit授权。8行为、261 App ssh-use／11cap／749TS、完整check/test/probe/strict与Exact诊断通过；该primitive不自行注册handler；后续Linux固定warm CLI见§12，GUI高风险批准与通用handler仍未接。独立workspace另接只读 `workspace_cap_list`：实际local-source/WindowToken/nonce原PageScope前后核验，只固定既有UI选择动作/empty闭合args/固定locale key/Risk metadata；default6、workspace-ssh7，palette自身只Keybinding、默认CLI名单为空；显式Linux workspace-cli另有独立CLI名单（见§12）。前端固定schema解码后只安装当前页已知名单，原button/target/busy/fit与Native权限不变，失败无静态fallback、queued/late旧结果不回装；8协议反例和757TS、default136/SSH142 Native/strict、真实default三页Settings跳转/exactPTY/close与7动作SSH auth1/echo/notice/native退出、keymap兼容通过。Rust诊断Exact、TSUnknown不算通过，完整check/test/probe过；真实CLI执行/风险GUI批准并未由这个只读目录开放。以下含handler/cap_invoke的内容仍是目标架构，Session与Global effect不可用该Window primitive替代。

```rust
pub struct ActionDef {
    pub id: &'static str,                 // "session.open" | "pane.split" | "desktop.send_cad" | ...
    pub title: LocalizedKey,
    pub args: schemars::Schema,
    pub scope: Scope,                     // Global | Window | Session(SessionKind)
    pub risk: Risk,                       // Safe | Confirm | Dangerous
    pub surfaces: Surfaces,               // 可以从哪些入口调用：Palette | Keybinding | Cli | Mcp | Plugin
    pub handler: Handler,                 // async fn(Ctx, Value) -> Result<Value>
}
```

- **调用入口**：前端使用 `cap_list` 和 `cap_invoke`（命令面板和快捷键都走这里）；CLI 通过 JSON-RPC 调用，方法名就是 action id；MCP 工具只包含用户允许的那部分 action；插件按其声明的权限调用。
- **风险等级**：`Confirm` 和 `Dangerous` 两级在由 CLI、MCP 或插件发起时，必须在界面上弹窗，由用户确认。
- **前端专属的动作**（例如“聚焦左侧窗格”）也在这里登记。它们的 handler 把指令转发给对应窗口的前端去执行。这样 CLI 的 `focus-tab`、`split-pane` 也能调用到。
- action id 是公开接口，发布后不能改名，只能废弃。参数只允许新增可选字段。

## 12. CLI 通信服务

- **本地命令兼容**：`open <已发现Shell标识>` 在CLI解析层映射已有 new-tab，`split-pane -v/-h <Shell>` 映射已有 split；同一Capability/原页确认、Profile/root链和不确定结果不重试。不存在通用alias执行器；host alias、URI与自动选身份不在此入口。cold open沿同 new-tab 原页presentation与确认，不扩cold方法名单。
- **敏感竞争限定证据**：双真实cold创建CLI各发一次，同步造成两个GUI启动attempt；插件保唯一D-Bus primary，敏感secondary不转发、原claim失败不能borrow主请求。四原确认场景实际通过，批准最多一PTY；原modal期间12×4实际warm open/new-tab竞争全部拒绝且原waiter/endpoint保持。不是精确连接槽耗尽或长期压力证明；取消后native信用和root回收规则不变。
- **当前Linux cold/single-instance限定**：闭合startup-actions/UUID不是权限；插件先于WebView/端点，另核D-Bus actual owner PID，拒静默继续。原document checkpoint等待原nonce Ready，refresh/同nonce替换拒旧捕获；UUID单次认领仍走Cap和原scope effect核验。仅完全无端点启动受信sibling单链接GUI，warm失败/部分/占用不cold重试。共享cold transport名单仅focus/profiles/new-tab，只读名单仍前两者，不接受路径/密码/任意启动参数。cold new-tab原Ready和原Arc presentation共享8s metadata，不占native信用或授action权；UI监听/Profile读取/初始恢复成功后report，backend检查local source/WindowToken/current nonce，迟到/换页/关闭拒绝、取消仅waiter局部。随后既有Cap/GUI30s/native5s启动，outer45s不裁慢批准。SDK构窗前Unix admission新建0700 app-data，既有unsafe不chmod。实窗Cancel/断开/refresh零PTY、16s批准一PTY/两实际echo/同PID恢复/root实际排空已验，readonly和secondary兼容过。cold split/SSH、host alias、并发cold完整矩阵与三平台待（本地 open 复用 cold new-tab 已验）。
- Linux限定并发cold已验Safe/read-only FF/PP/FP三pair：独立CLI同时exec导致实际双GUI启动竞争，既有插件决定唯一D-Bus primary，secondary退出不替换主端点；两original一次claim成功、0PTY、warm端点身份保持和actual exit。observer以broker实际owner而非scan第一PID识别主窗。生产协议未因此改动；敏感/quota/异常退出/压力和三平台完整矩阵未验。
- 真实CLI故障矩阵43场景已覆盖14端点故障×三动作和broker失败；以kernel process/connect验证无fallback/源保留，而非仅未发现瞬时GUI PID。strace不跟踪数据读写、不归档token/原始trace；42拒绝零GUIexec，broker一次尝试实际退出。只增加测试，不升级权限，不表示压力/耗尽/三平台完整覆盖。


- 使用 `interprocess` 建立本地 socket（Unix 为 UDS，Windows 为命名管道，并设置只允许当前用户访问的安全描述符）。
- 消息格式为 JSON-RPC 2.0，当前验证子集为object请求/string id/object params（不接受batch/notification/numeric id）。重复参数key（含嵌套、数组内object与escaped解码后同名）在认证/调度前整体拒绝，不用Map最后覆盖；有效标量与不同scope同名保持原JSON语义，serde递归上限维持。每条消息前加4字节小端u32长度，body≤64KiB，分帧只返回一条以便上层预算。建立连接时先发送`hello {protocolVersion:1, token}`；错误认证/重复hello/id/128请求预算耗尽均撤销连接，不执行任何动作。
- 当前pulse-ipc::cli提供有界codec与SessionAuth，不负责OS peer权限或Capability授权；token恒定时间比较，拥有secret的wire buffer/expected token/消费的hello token零化且无Debug。CLI准入后仍必须走共享Capability注册表及风险确认；nativeUnix探针只允许固定只读ping；13独立客户端包括preauth/成功hello后的重复参数及嵌套3个request静态拒绝，旧secret/version/peer权限/预算/EOF和cleanup亦验。37IPC/14CLI行为、Exact诊断、完整check/test/probe与startup消费者通过；不是生产interprocess/Windows管道实现。
- Linux opt-in `pulse-app/cli-control` 已单独实现 warm `interprocess` UDS：后端提供私有运行时目录（逐级拒绝 symlink／不安全权限）、0600 socket/token，逐连接核对 peer euid、首帧 hello，8 连接上限、hello／帧／写回／默认操作 5s 截止；仅受信后端登记的确认型new-tab可使用最多45s总operation预算（GUI30s＋native启动5s），不由请求参数修改；未知／占用端点不覆盖。取消先撤销再等 handler 实际退出，独立关闭观察者被取消也不能提早删除本次 socket/token；清理仅删除相同 dev/inode 对象。客户端也核验私有目录、端点类型/权限/owner、token 的 O_NOFOLLOW 身份与服务端 euid。六个真实 socket 反例独立覆盖传输；可选 `workspace-cli` 已接固定 `pulse focus` 产品入口，只从 OS XDG runtime 寻找现有端点，CLI surface 只单独登记 `workspace.focus`／只读 `workspace.profiles`（闭合空参数、Safe/Window）及 `workspace.new-tab`（已知Shell enum单参数、Confirm/Window），不与 Palette／Keybinding 执行权共享。服务根持有；实际 GUI 主线程 effect 前重新核对当前 Ready 原窗口 scope、Arc 身份、页面 generation、真实本地 WebView 来源、取消与单次参数消费；回执只表示 native focus 请求完成，不证明渲染或 compositor 激活。独立 Linux Openbox 实窗另验证三次从干扰窗恢复真实焦点，同一 PTY／exact 输入和退出排空；关闭观察者不能提前放掉任务。后续 `pulse profiles` 复用同一私有ProfileReader（不是新路径/发现器/偏好缓存），只发布最多五个未隐藏的固定Shell enum；读取前和读取后在GUI线程复核同一原Ready scope、本地WebView及取消，不能把新页当原页。客户端同源闭合schema检查后只打印已知enum JSON，不输出路径、环境、控制字符、nonce或ticket；读取失败不fallback，不创建会话，不授启动或修改权。后台SQL/discovery信用仍持有至实际退出，root关服务与ProfileReader并等真实drain。三代独立CLI与实窗同PTY/不增创建数、原页/foreign/cancel反例和140Native/757TS/strict/完整基线已通过，audit仍八许可＋两维护拒绝（未放宽0BSD）；专用new-tab另持原Ready scope/完整PreparedWindowAction，先读取同ProfileReader发现与可见性再出原窗口单次30s审批；票据匹配不代替原scope、registry、operation取消和PreparedWindowAction确认。默认Cancel，unknown/duplicate/expired/busy/foreign/锁页/EOF拒绝，navigation及root退出同步retire。批准后重新复核原scope并重新读取当前发现和偏好，以既有5s可取消Profile startup移交root collection，再将同ticket的SessionId递送给当前页TermView。CPU/SQL/native信用不随waiter提前释放，已经root接管的进程在CLI回执遗失时保留并可从保留会话发现，不自动回滚/重试。新会话回执只证明root接管，不证明drawing；Linuxpublic probe另证明可见modal、Cancel默认、取消/断开/未决refresh不创建、只一次新PTY、完整真实回显和refresh恢复/双PID退出。765TS/144Native（2ignored不计）/5binary/6socket及完整strict/check/test/probe通过；CLI＋SSH组合feature all-target strict亦通过（cli-newtab-ssh-combined.log），仅组合构建兼容；本轮Astrolabe diagnostic超时不计通过，audit仍八许可＋两维护拒绝。其他CLI动作、Linux以外与cold single-instance/startup-actions仍未交付。
- 可选 Linux warm `pulse focus-tab <session-id>` 对应独立 CLI-only Safe/Window `workspace.focus-tab`，仅接受单个 canonical 非零 u64 根持有会话标识；不是 launch/input 权限，cold 名单不包含它。实际本地来源／原 Ready page Arc 在 GUI 主线程取一次 PreparedWindowAction、检查 root ownership 并预留单飞 5s ticket；eval 发事件不算成功。原页经 `workspace_cli_select_admit` 消耗同 ticket/target 的一次 admission 后才选择，忙态／模态／换页拒绝；`workspace_cli_selected` 必须匹配原页、已 admit、ticket、target、deadline 与未取消 waiter，回执前再检查同原页及 root ownership。刷新 retire、关闭 close，失去 callback 接收者也一并销毁 reservation/取消通知；不确定显示效果不撤销 root 会话或自动重试。CLI 只接受原目标与 boolean true 的闭合两字段回执。已有 split pane 只聚焦原 controller、不争用新 lease；其它标签复用 TermView mount/Commit drawing gate，选择不绕过原输入／EOF／resize 授权。
- CLI `workspace.split`（`pulse split -v|-h <Shell>`）为独立Confirm/Window闭合动作，复用new-tab原page/PreparedWindowAction和Profile/root链。前端只在原可用单pane保存session与对象身份并在批准/notice重查；换页或换标签后回到同SessionId也不能借新pane继续旧意图。根移交后显示失败不回滚session、不自动重试。split收到root会话后调用既有双pane控制器、聚焦第二pane并记住会话选择；不保存双pane布局。新tab及split完整客户端response预算45s，GUI30s/native5s分别保留，握手/读取请求/写入仍原5s；8真实socket含6s批准响应证明外层预算生效。772TS/146Native（另2ignored不计）/6CLI/完整基线/组合strict/两方向实际双PTY与refresh、原new-tab兼容、native退出均验；证据见cli-split-acceptance.log。不授SSH、明文读取或通用exec能力，审核阻断保持。
- `workspace.ssh` 为独立 CLI Confirm/Window 动作，闭合保存Host/Identity UUID参数，仅Linux组合workspace-cli＋workspace-ssh启用。等待批准前捕获原PageLaunchScope/registry和keyless原Vault epoch gesture，冻结完整已存Host/Identity source；gesture单次消费与native启动均重查原epoch，不得借锁定后新解锁升级。原窗只显示有界metadata并默认Cancel；GUI30s/client45s不延长native5s。批准后准备同一source、实际peer pin验证先于credential借用，plaintext不返页。source changed/refresh/EOF拒绝；root移交后回执遗失不回滚、不重试，沿既有TermView和root drain。777TS/271App/153Native（2ignored不计）/7CLI、两feature strict、真实auth0负例及auth1/三次输入/refresh/通知型resize/actual native exit通过。首屏bootstrap与外层timeout留证、audit8+2不放宽；cold single-instance/open仍关闭。
- token 在每次启动时随机生成，写入运行时目录中权限为 0600 的文件。
- S5固定native验证中，CLI响应只确认真实窗口注册/visible状态，不声称页面已加载或compositor首帧；page-load/Destroyed另外监督。Linux样例native客户端收据约8–12ms而WebView页面加载约330ms，不能把前者当生产窗口完整可用50ms或冷启动验收。defaultdesktop不含cli-window-probe，未开放生产CLI窗口请求。
- 主进程没有运行时，CLI 以 `--startup-actions` 参数启动 `pulseterm-desktop`，参数中带上本次的请求。`single-instance` 插件负责处理启动过程中的竞争（例如同时运行了两次 CLI）。
- 当前只读Unix daemon startup probe独立证明：竞争只有一个主实例，第二实例不改active socket/token；仅取得exclusive lock后且所有socket/token对象类型/owner/mode/linkcount/format验证完成，才能清理受信任旧端点。SIGKILL释放OS文件锁，重启换token，旧token实测拒绝且不关闭新服务。此锁fixture不是对Tauri插件/生产bootstrap协议的已批准替换，coldGUI/WindowsACL仍待验证。

## 13. 错误模型

```rust
#[derive(thiserror::Error, Serialize)]
pub struct PulseError { pub code: ErrorCode, pub message: String, pub details: Option<Value>, pub retryable: bool }
// ErrorCode 是稳定的字符串枚举：ssh.auth_failed / ssh.hostkey_changed / route.proxy_refused / rdp.nla_failed /
// rdp.cert_untrusted / fs.permission_denied / vault.locked / cap.denied / internal ...
```

- 库 crate 内部各自定义错误类型，在边界处转换成 `PulseError`。`message` 只用于日志和调试，界面上显示的提示由 `code` 决定。
- 除测试外禁止 `unwrap()` / `expect()`（由 clippy 检查）。会话 actor 中的 panic 要被捕获，转为该会话失败（`Failed(internal)`），不能导致应用崩溃。

## 14. 平台层（pulse-platform）

| 模块 | Windows | macOS | Linux |
|---|---|---|---|
| `conpty` | 加载随附的 ConPTY | — | — |
| `backdrop` | 按系统版本选择 Mica / Mica Alt / Acrylic；监听系统透明开关 | Vibrancy / Liquid Glass | 纯色 |
| `kbd_capture` | 低层键盘钩子（`WH_KEYBOARD_LL`） | CGEventTap（需要辅助功能权限） | X11 键盘独占（XGrabKeyboard）；Wayland 不支持 |
| `elevation` | 以管理员身份启动独立实例 | — | — |
| `secrets` | Credential Manager | Keychain | Secret Service，没有时退回主密码模式 |
| `biometric` | Windows Hello | Touch ID（LocalAuthentication） | — |
| `power_net` | `WM_POWERBROADCAST`、`NotifyNetworkConnectivityHintChange` | NSWorkspace 唤醒通知、`NWPathMonitor` | logind `PrepareForSleep`（D-Bus）、netlink / NetworkManager |
| `proxy` | WinHTTP 系统代理设置 | `SCDynamicStore` 代理设置 | 环境变量、GNOME / KDE 代理设置 |
| `shell_integration` | 资源管理器右键菜单、任务栏跳转列表 | Finder 服务 | .desktop 文件 / Nautilus 扩展 |

## 15. 后端测试策略

| 层级 | 手段 |
|---|---|
| 协议与状态机 | 单元测试 + `proptest`；对自己写的解析器做 `cargo-fuzz`（Telnet、PPK、帧格式、Mosh、OSC 提取） |
| 终端 | 快照重放一致性测试（§4.3）；用 `insta` 做屏幕内容的快照测试；用 EchoTransport 测试发送额度和落后状态 |
| 会话与应用层 | 在 `pulse-app` 中用内存版的 `FrameSink` 和内存数据库，不启动 Tauri，跑完整流程 |
| 协议互通 | `tests/interop/compose.yml`：OpenSSH（多个版本）、dropbear、跳板链、squid（HTTP 代理）、dante（SOCKS）、xrdp、TigerVNC、telnetd、虚拟串口（socat pty）。Windows RDP 在自托管 CI 上测试 |
| 性能 | `criterion` 微基准（解析、快照、帧编码）；`xtask bench` 端到端测试（`cat` 1GB、启动时间、内存） |
