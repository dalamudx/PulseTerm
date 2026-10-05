# 架构决策记录（ADR）

> 状态：**Accepted** 已采纳；**Proposed** 待确认；**Superseded** 已被替代。
> 新增或修改架构决策时，在本表中登记。简单的决策只写一行；复杂的写成 `adr/NNN-*.md`，包含背景、可选方案、决定和后果。
> 决策中引用的 S1–S21 见文末的[技术验证清单](#技术验证清单spike)。

| # | 决策 | 状态 | 依据 |
|---|---|---|---|
| ADR-001 | 终端状态由 Rust 侧（alacritty_terminal）保存，前端 xterm.js 只是可以销毁重建的显示层 | Accepted | Netcatty #2578（终端状态都在前端，内存失控）；NovaTerm 重现了同样的问题；用户已批准 alacritty 0.26.0 非活动网格只读 getter 补丁，详见 terminal-restore-evaluation.md §7。此批准不扩展到 GTK 宏依赖 |
| ADR-002 | v1 之前不自研终端渲染器 | Accepted | NovaTerm 自研渲染器当天就放弃，改回 xterm.js |
| ADR-003 | 每个会话使用独立的二进制通道，按额度发送；传输过程绝不丢数据，前端跟不上时可以跳过中间画面；会话数据禁止用全局事件传输 | Accepted | NovaTerm 用全局事件广播 base64 输出，队列积压时丢数据 |
| ADR-004 | Transport 采用由使用方主动读取的输出接口，并提供状态监听；连接路径（Route）和协议解耦 | Accepted | Windows Terminal 的 ITerminalConnection；主动读取天然带背压 |
| ADR-005 | Windows 上使用 portable-pty + 随附 ConPTY + win32-input-mode | Accepted（随附加载待 S10 验证） | Windows Terminal spec #4999 / #13000 |
| ADR-006 | 同一窗口内不混放管理员和普通权限的标签；管理员权限使用独立实例 | Accepted | Windows Terminal spec #1032：混合提权无法安全实现 |
| ADR-007 | CLI 为独立二进制，用 JSON-RPC 与主进程通信；用 `;` 串联子命令、用 `-w` 指定窗口；主命令名为 `pulseterm`，`pulse` 为可选别名（安装时检测 PATH 冲突，不覆盖）；不提供 `pt` 别名 | Accepted | Windows Terminal spec #607；npm 包 pulse-tm 也安装 `pulse` 命令；`pt` 已被 Homebrew 的 the_platinum_searcher 占用 |
| ADR-008 | 设置分为三层（内置默认值 / 用户差异 / 分组和主机覆盖）；机器状态和用户设置分开存放；用生成器自动生成 profile | Accepted | Windows Terminal spec #754 / #8324 |
| ADR-009 | 所有入口共用同一个 Capability 注册表，并按风险等级要求确认 | Accepted | Windows Terminal spec #2046；Netcatty 后期才把 AI/MCP/CLI 收敛到一处 |
| ADR-010 | Shell 集成（OSC 133/7）在 Rust 侧解析，标记保存在会话中 | Accepted | Windows Terminal spec #11000 |
| ADR-011 | 需要同步的表从一开始就包含 `id(UUIDv7) + hlc + deleted` 字段；一台主机对应多个协议入口（Endpoint） | Accepted | Netcatty 同步 v1 推倒重做、Host 模型字段新旧并存 |
| ADR-012 | 凭据不进前端；使用数据密钥（DEK）加系统钥匙串，可选主密码（Argon2id）；唯一例外是 noVNC 解码模式下的 VNC 密码 | Accepted | backend.md §10.2 |
| ADR-013 | RDP 使用 IronRDP，在 Rust 侧解码，只把变化区域发给 WebGL；P2 支持 H.264 直通 WebCodecs；提供独立原生窗口和外部客户端作为补充；不内置 OpenH264 | Accepted | IronRDP 功能覆盖 NLA/Gateway/剪贴板/动态分辨率，H.264 解码器是可替换接口；OpenH264 有专利和分发问题 |
| ADR-014 | VNC 在 P1 使用 noVNC（在 WebView 中解码），Rust 只负责传输；vnc-rs 作为备选 | Accepted | noVNC 认证和编码最全（含 Apple ARD、H.264）；vnc-rs 只支持 None/VncAuth |
| ADR-015 | 前端使用 React 19 + Zustand + Tailwind + Radix；通过三条规则弥补和 Solid 的性能差距（数据不进 React 状态、高频数值单独处理、视图在 React 之外管理） | Accepted（2026-10 确认，不做 S20） | 重数据不经过框架；分屏/拖拽/Monaco 等组件生态差距大；Solid 2.0 仍在 RC |
| ADR-016 | 窗口材质由 `pulse-platform::backdrop` 按系统版本自动选择并降级；Win10 默认纯色；Acrylic 只在 22H2 及以上开放；终端区域默认不透明 | Accepted | window-vibrancy 已知问题：Acrylic/Blur 在部分版本拖动和缩放时卡顿 |
| ADR-017 | `pulse-app` 及其下层都不依赖 Tauri，为将来的 mux 守护进程和无界面测试留出空间 | Accepted | overview.md §3 |
| ADR-018 | 前后端的类型定义由工具生成（tauri-specta，备选 ts-rs），禁止手写重复的类型定义 | **Proposed**（S21 已取得 Linux 命令/事件/Raw Channel 证据；RC 依赖门禁与三平台验证待完成） | — |
| ADR-019 | 项目许可证采用 MIT OR Apache-2.0；禁止引入 GPL 依赖；参考 Netcatty 时遵守 clean-room 约束 | Accepted（2026-10 确认） | 主要依赖均为 MIT/Apache；Netcatty 为 GPL-3.0 |
| ADR-020 | 产品范围约束：不做 shell、脚本语言、X Server、SPICE/NX，也不内置大型 AI Agent | Accepted | features.md §20 |
| ADR-021 | 产品名定为 **PulseTerm**（原名 AstraTerm 弃用）；crate 前缀 `pulse-`，命令 `pulseterm`，URL 协议 `pulseterm://`，环境变量前缀 `PULSE_` | Accepted（2026-10 确认） | 同领域已有名为 AstraTerm 的 SSH/AI 运维终端（2026-06 上线）；`astra` 与 DataStax 的 Homebrew `astra` CLI 重名。PulseTerm 在 crates.io、npm、Homebrew、Debian 均无同名包，GitHub 上只有几个零星个人仓库 |
| ADR-022 | 内嵌 RDP（RDP-01~08）和内嵌 VNC（VNC-01~03）属于 v1.0 必须交付范围 | Accepted（2026-10 确认） | 三平台开源产品中“SSH 工作台 + 内嵌远程桌面”是主要差异化；S14/S15 未通过的功能由外部客户端（RDP-08）兜底，不再调整范围 |
| ADR-023 | 不上架 Mac App Store；macOS 通过已签名并公证的 dmg 和 Homebrew cask 分发 | Accepted | App Store 沙盒限制启动任意本地 shell 和访问用户目录，与本地终端的核心功能冲突；窗口材质依赖的 `macOSPrivateApi` 也不允许上架 |
| ADR-024 | 崩溃报告使用 `sentry` + `sentry-rust-minidump`（基于 crash-handler / minidumper，原生崩溃在独立进程中写 minidump），按 Sentry 协议上报；默认关闭，由用户主动开启；上报地址可配置为自托管的 Sentry 或 GlitchTip；上报前去除路径中的用户名、主机名和会话内容 | Accepted | 依赖活跃维护（2026-09 有发布）；Sentry 协议有开源自托管实现，不绑定商业服务；满足 REL-03 和 SEC-06 |
| ADR-025 | 配色不自研：界面使用 Radix Colors 12 级色板映射到语义变量，由“一个中性色系 + 一个强调色系”生成整套主题；终端使用标准的 16 色 ANSI 结构（字段与 Windows Terminal 配色方案对齐），内置 iTerm2-Color-Schemes 配色库；支持导入 VS Code、Windows Terminal、iTerm2、Alacritty、base16/base24、Ghostty 格式 | Accepted（2026-10 确认） | VS Code、Zed、Windows Terminal、shadcn 都采用“色板 + 语义变量”的做法；Radix Colors、iTerm2-Color-Schemes、tinted-theming 均为 MIT；VS Code 主题自带 `terminal.ansi*`，一次导入即可覆盖界面和终端 |
| ADR-026 | 终端查询（DA、DSR、DECRQM、颜色查询等）只由后端应答，前端通过 parser hook 屏蔽 xterm 的应答；终端配色同步到后端 | Accepted | 前后端各有一份解析器，两边都应答会让 shell 收到重复的应答 |
| ADR-027 | 字符编码转换在后端完成：输出在进入解析器前转换为 UTF-8，输入写入前转换回原编码；前端和解析器只处理 UTF-8 | Accepted | 老服务器和网络设备普遍使用 GBK 等编码；`alacritty_terminal` 和 xterm.js 都只支持 UTF-8 |
| ADR-028 | Route 增加外部命令（ProxyCommand）一跳；内置 SSH 不支持的场景，可以对单台主机改用系统 OpenSSH 模式 | Accepted | ssh_config 中常见 ProxyCommand（cloudflared、AWS SSM）；russh 不支持 GSSAPI 和 PKCS#11 |

| ADR-029 | 非 UTF-8 会话采用版本化双坐标：保留原始 source 字节边界，另设 UTF-8 显示游标；ACK 按实际帧额度，不与任一累计计数混用 | Accepted（用户确认双坐标） | GBK 固定样例 source33/UTF843 不等；方案与后果见 [编码边界评估](./terminal-encoding-boundaries.md)，v2 显式准入、v1 拒转码，生产 binary preclassifier 和窗口适配仍待完成 |

| ADR-030 | 已知 Unix shell 的登录启动采用显式登录参数，而非改写 argv0；保留选定程序与字面用户参数，不 patch portable-pty，不使用默认程序回退 | Accepted（用户选择登录参数） | bash 前置 `--login`，zsh/fish 前置 `-l`；generic sh、pwsh 和未知程序不自动推断支持。`$0` 不保证具有 `-` 前缀，不承诺与 Terminal.app/iTerm2 启动语义完全一致。macOS 默认登录、Linux 默认普通启动仍是目标配置，系统启动文件与跨平台兼容待实测。 |

## 待决事项

| 事项 | 影响 | 计划 |
|---|---|---|
| PulseTerm 的商标注册检索（中国、美国、欧盟）和域名购买 | 品牌 | M0 期间完成。包名和命令名已核查，结论见 ADR-021 |
| 品牌视觉：设计 Logo，并选定默认强调色系（从 Radix 色系中选） | 品牌、默认主题 | M1 之前完成。不再单独设计整套配色（ADR-025）；选定之前默认使用 cyan |
| tauri-specta 是否可正式采用（ADR-018） | 前后端类型如何生成 | 已接入受控验证；待三平台及新增 paste 维护公告审查，见 development/validation.md |
| 代码签名证书和开发者账号：Windows（OV 证书或 Azure Trusted Signing）、Apple Developer Program | 没有它们就无法发布 Alpha（未签名的程序会被 SmartScreen 和 Gatekeeper 拦截） | M1 之前办妥；申请周期可能要几周，M0 期间就要启动 |
| 内置字体选型（需要支持子集化、没有保留字体名） | 安装包体积、合规 | M1 之前确定 |

### 已决事项

| 事项 | 结论 | 决定时间 |
|---|---|---|
| 产品名 | PulseTerm（ADR-021） | 2026-10 |
| 内嵌 RDP/VNC 是否进 v1.0 | 进入 v1.0（ADR-022） | 2026-10 |
| 项目许可证 | MIT OR Apache-2.0（ADR-019） | 2026-10 |
| 前端框架 | React 19（ADR-015） | 2026-10 |
| 是否上架 Mac App Store | 不上架（ADR-023） | 2026-10 |
| 崩溃报告方案 | Sentry 协议 + minidump，默认关闭，可自托管（ADR-024） | 2026-10 |
| 配色方案 | 沿用成熟色板和通用终端配色格式，不自研配色（ADR-025） | 2026-10 |

## 技术验证清单（Spike）

> M0 阶段完成。建议顺序：S1/S13 → S2 → S14/S15 → S4 → 其余。未通过时按“未通过时的预案”切换方案，并更新对应的 ADR。

| # | 验证项 | 验收标准 | 未通过时的预案 |
|---|---|---|---|
| S1 | Tauri Channel 二进制吞吐与无损 | 三平台（含 Linux WebKitGTK）终端数据 ≥ 50MB/s 且不阻塞界面；`cat` 1GB 随机 UTF-8 文本后，Rust 侧和 xterm 的屏幕内容 hash 一致 | 数据通道改用只监听本机的 WebSocket |
| S2 | xterm.js 显示层 + alacritty_terminal 状态 | vim/htop/tmux/CJK/emoji/备用屏下快照恢复正确；20 个标签内存达标；10 万行历史 ≤ 20MB；同一份宽度测试样例（CJK、emoji、ZWJ 序列）在前后端宽度一致；经 parser hook 屏蔽后，每个查询只收到一份应答 | 缩小热区或改用 `vt100` |
| S3 | portable-pty 在 Windows 上的表现 | pwsh/cmd/WSL/Git Bash 下调整尺寸、Ctrl+C、UTF-8 正常 | 在 `pulse-platform` 中自己封装 ConPTY |
| S4 | russh 兼容性 | OpenSSH 7–9、dropbear、交换机等老设备的旧算法、keyboard-interactive、证书认证、跳板、Agent 转发；通过 Agent 使用 FIDO2 硬件密钥（`sk-ssh-ed25519`）；GBK 编码的网络设备 | 罕见场景退回调用系统 OpenSSH |
| S5 | CLI 与主进程通信 | 主进程运行时 `pulse .` 在 50ms 内出窗口；未运行时能正确拉起 | — |
| S6 | 窗口材质与自绘标题栏 | Win10 21H2 / Win11 21H2 / Win11 23H2+ 上各材质及降级正常；系统关闭透明、节电、失焦时界面正常；无边框窗口能否恢复 Win11 贴靠布局；透明下文字清晰度和缩放闪烁 | 保留系统原生标题栏 |
| S7 | 输入法与字体 | 中日韩输入法候选框跟随光标；WebKitGTK 下字体渲染可接受 | Linux 列为二级支持平台 |
| S8 | Wasm 插件原型 | WIT 定义补全接口，单次调用 < 50µs，权限沙箱生效 | 评估 extism |
| S9 | win32-input-mode | pwsh 7 + PSReadLine 中 Ctrl+Space、Shift+Enter、Ctrl+Break、Alt+Shift 组合键正常 | — |
| S10 | 随附 ConPTY | portable-pty 能加载随附的 `conpty.dll` / `OpenConsole.exe`；Win10 21H2 上调整尺寸和 vim/htop 正常；缺失时回退到系统版本 | 在 `pulse-platform` 中自己封装加载 |
| S11 | 会话与窗口解耦 | 强制刷新 WebView、跨窗口拖动标签时会话不断，屏幕内容从快照完整恢复 | — |
| S12 | 提权窗口 | 从普通窗口打开管理员 profile 时拉起独立实例；两个实例不能互相注入输入；有明显视觉标识 | — |
| S13 | 桌面画面传输吞吐 | Channel 与本机 WebSocket 对比；三平台 1080p 变化区域持续 ≥ 100MB/s，单帧 p95 < 16ms；记录各发行版 WebKitGTK 版本 | 远程桌面默认改为独立原生窗口（RDP-12） |
| S14 | IronRDP 兼容性 | Windows Server 2016/2019/2022/2025、Win10/11、xrdp；NLA（NTLM/Kerberos）、RD Gateway、RemoteFX、EGFX、剪贴板、动态分辨率；列出与 FreeRDP 相比缺少的功能 | 缺失功能由外部客户端（RDP-08）兜底 |
| S15 | noVNC 对接自定义传输 | 类 WebSocket 适配层对接 Rust 的 TCP/SSH 隧道；TigerVNC、x11vnc、macOS 屏幕共享、RealVNC、QEMU 都能连上 | 改用 vnc-rs 在后端解码并补齐认证 |
| S16 | H.264 直通 WebCodecs | 只转发不解码的 `H264Decoder`；三平台硬解正常；与其他编码区域合成无撑裂；不支持时自动回退 | 只用 RemoteFX |
| S17 | 系统快捷键捕获 | Win 低层键盘钩子、mac CGEventTap、X11 键盘独占；失焦时可靠释放、无卡住的修饰键；Wayland 降级可接受 | 只提供工具栏按钮 |
| S18 | Route 组合 | RDP 经两级 SSH 跳板、VNC 经 SOCKS5、Telnet 经 HTTP CONNECT，都只靠 Route 配置组合，不写协议专用代码 | — |
| S19 | Mosh 客户端原型 | 与 `mosh-server` 1.4 互通；网络切换或 IP 变化后会话不断；输出接入统一 Transport | 推迟到 v1.x 之后 |
| ~~S20~~ | ~~React 与 Solid 对比~~（已取消，ADR-015 已定稿） | 同一个外壳（标签栏 + 5000 台主机列表 + 20 个每秒刷新 10 次的进度条）在三平台测冷启动、CPU、内存 | 差距显著时重新评估 ADR-015 |
| S21 | tauri-specta 类型生成 | 在当前 Tauri 2 版本下稳定生成命令与类型绑定，支持 Channel 与事件 | 改用 ts-rs |
