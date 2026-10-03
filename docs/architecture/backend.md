# 后端架构（Rust）

> 前置阅读：[overview.md](./overview.md)。本文描述 crate 划分、运行时模型、各类会话的实现、存储与安全，以及 Capability 层。

## 1. Cargo workspace

```
crates/
  pulse-core       领域类型：Id(UUIDv7)、Hlc、ErrorCode、Host/Endpoint/Identity/Route 等模型、设置的 schema 类型。不做任何 I/O
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
  - PTY 的读写是同步接口，各用一个线程，用有界的 `mpsc` 和 actor 通信。
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
- **启动时的环境变量**：
  - `TERM=xterm-256color`、`COLORTERM=truecolor`、`TERM_PROGRAM=PulseTerm`、`TERM_PROGRAM_VERSION`；
  - `PULSE_PANE_ID`、`PULSE_SOCKET`（供 CLI 定位当前窗格）；
  - pwsh 加上 `-NoLogo`，并设置 `POWERSHELL_UPDATECHECK=Off`。
- **登录 shell**：macOS 默认以登录 shell 方式启动（argv[0] 加 `-` 前缀），和 Terminal.app、iTerm2 的行为一致，保证 `/etc/paths` 和 `~/.zprofile` 生效。Linux 默认不使用登录 shell，可在 profile 中修改。
- **Windows 环境变量刷新**：新会话启动前，从注册表重新读取用户和系统的环境变量，再合并 profile 中的设置，使得安装新软件后不用重启 PulseTerm 就能用上新的 PATH。
- **Shell 集成脚本注入**（SHELL-03）：bash 用 `--rcfile`；zsh 用临时 `ZDOTDIR`；fish 使用 `XDG_DATA_DIRS` 下的 vendor_conf.d；pwsh 使用 `-NoExit -Command`。每种方式都要确保用户原有的配置文件仍然会被加载。

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

- **SSH 连接池**：以 `(endpoint, identity, route)` 区分连接，并做引用计数。同一主机的 shell、SFTP、端口转发和以它为跳板的连接共用一条 SSH 连接；最后一个使用者离开后延迟 30 秒再断开（SSH-06）。
- **认证流程**：按顺序尝试 Agent 中的密钥、配置的密钥、密码、keyboard-interactive。需要用户输入时（MFA 码、密码、私钥口令），会话状态变为 `Prompt { id, kind, text }`，前端弹窗后通过 `session_prompt_reply` 回传。用户输入的内容用 `Zeroizing` 包装，用完即清除。
- **主机密钥校验**：先查自己的 `known_hosts` 表，再查 `~/.ssh/known_hosts`（只读）。密钥变化时会话直接失败，错误码 `ssh.hostkey_changed`，需要用户在界面中明确确认后才能更新。
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

- 前端只能拿到 `SecretRef { id, kind, has_value }`；修改凭据通过 `vault_secret_set(id, value)`，后端只接收、不回传。后端也不提供任何“读取明文”的命令。唯一例外是 VNC 密码，见 §7.3。
- Linux 上如果没有 Secret Service（例如没有桌面会话），强制使用主密码模式。
- 主密码模式下的应用锁（SEC-02）：锁定时从内存中清除 DEK，已经建立的会话不受影响。解锁之前不能发起需要凭据的新连接。

### 10.3 设置

- 分三层：编译进程序的 `defaults.toml` → 用户的 `settings.toml`（只保存改过的项）→ 分组和主机各自的覆盖配置（存在数据库中）。`SettingsResolver` 计算最终生效的值，并记录每一项来自哪一层（VAULT-07 在界面上展示）。
- 设置类型用 `schemars` 生成 JSON Schema，写入文件注释，编辑器可以据此补全；前端的设置页也由同一份 schema 生成。
- 用 `notify` 监听文件变化，热加载时发出 `settings.changed` 事件。保存文件时用 `toml_edit`，**保留用户原有的注释和顺序**。
- **主题**（UI-01 / UI-07，格式见 frontend.md §10）：内置主题从 `assets/themes/` 编译进程序，用户主题放在 `themes/*.toml`，同样支持热加载。各种外部格式的导入器（VS Code JSON、Windows Terminal JSON、`.itermcolors`、Alacritty TOML、base16/base24 YAML、Ghostty）放在 `pulse-store::theme`，统一转换成 PulseTerm 主题格式后写入 `themes/`；原文件只读取，不保留。导入时计算前景与背景的 WCAG 对比度，低于 4.5:1 时附带警告信息返回给前端。导入器都是纯函数（输入文本、输出主题），每种格式配有测试样例。

## 11. Capability 层（pulse-cap）

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

- 使用 `interprocess` 建立本地 socket（Unix 为 UDS，Windows 为命名管道，并设置只允许当前用户访问的安全描述符）。
- 消息格式为 JSON-RPC 2.0，每条消息前加长度前缀。建立连接时先发送 `hello {protocol_version, token}`。
- token 在每次启动时随机生成，写入运行时目录中权限为 0600 的文件。
- 主进程没有运行时，CLI 以 `--startup-actions` 参数启动 `pulseterm-desktop`，参数中带上本次的请求。`single-instance` 插件负责处理启动过程中的竞争（例如同时运行了两次 CLI）。

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
