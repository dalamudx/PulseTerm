# PulseTerm 功能规格

> 定位：**三平台、轻量的 SSH 工作台 + 现代终端 + 内嵌远程桌面（RDP/VNC）**。
> 性能对标 Electron 类产品（Termius / Tabby / Netcatty），功能覆盖面对标 MobaXterm / Royal TS。

## 0. 约定

**阶段**

| 标记 | 含义 | 大致对应 |
|---|---|---|
| P0 | MVP，alpha 版必须有 | M1 |
| P1 | v1.0 必须有 | M2–M3 |
| P2 | v1.x 增强 | v1.0 发布之后 |
| P3 | 探索 / 差异化，是否做视反馈而定 | — |

**平台**：W = Windows，M = macOS，L = Linux。未标注的表示三个平台都支持。
**编号**：功能 ID（如 `SSH-03`）在 issue、PR、测试用例中引用，**编号一旦分配就不再复用**。

---

## 1. 终端核心（TERM）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| TERM-01 | 标签与分屏 | 水平/垂直分屏可任意嵌套；拖动调整比例；窗格可以最大化或还原；支持用方向键导航 | P0 |
| TERM-02 | VT/xterm 兼容 | 256 色/真彩色、备用屏、鼠标上报（SGR）、bracketed paste、焦点事件、光标样式、OSC 8 超链接、窗口标题 | P0 |
| TERM-03 | Unicode | CJK/emoji 宽字符（Unicode 11+）、组合字符 | P0 |
| TERM-04 | 输入法 | 中日韩输入法的候选框跟随光标，组字过程中不误发按键 | P0 |
| TERM-05 | Rust 侧 scrollback | 完整的历史记录保存在后端（默认 10 万行，可配置），前端只保留最近一段；更早的历史按需分页拉取 | P0 |
| TERM-06 | 后台会话零开销 | 不可见的标签不向前端推送数据；前端视图可以释放，切回时从后端快照恢复 | P0 |
| TERM-07 | 复制粘贴 | 有选区时 Ctrl+C 复制，否则发送中断；可选“选中即复制”；粘贴多行或含敏感字符的内容前确认；右键粘贴可配置 | P0 |
| TERM-08 | 字体 | 连字、粗体/斜体、行高和字间距、回退字体链；内置一个子集化的 Nerd Font（< 1MB） | P0 |
| TERM-09 | 搜索 | 正则、区分大小写、整词匹配；在后端的全量历史中搜索，并在滚动条上标出命中位置 | P1 |
| TERM-10 | 链接识别 | URL、文件路径（可用外部编辑器打开）、IP:端口 | P1 |
| TERM-11 | 自动滚到底部可配置 | 用户在回看历史时，新输出不把视图拉走 | P1 |
| TERM-12 | 通知 | 响铃、后台标签有新输出、长时间无输出、长命令执行完成（依赖 SHELL-02）时给出提示或系统通知 | P1 |
| TERM-13 | 关键词高亮与输出触发器 | 按正则高亮输出（ERROR/WARN 等）；匹配到时执行动作（通知、自动回复） | P2 |
| TERM-14 | 内联图片 | Sixel / iTerm2 / Kitty 图片协议 | P2 |
| TERM-15 | 会话录制与回放 | 录制为 asciicast v2 格式，内置回放器 | P2 |
| TERM-16 | 会话日志 | 把终端输出写入文本文件：可选纯文本（去掉控制序列）或原始字节；可以按主机自动开启；文件名模板支持主机名和时间；可按大小或日期分割。默认关闭，因为日志里可能有敏感信息 | P1 |
| TERM-17 | 字符编码 | 按 profile 或主机选择编码：UTF-8（默认）、GB18030/GBK、Big5、Shift_JIS、EUC-KR、ISO-8859-x 等；输出和输入都做转换；可以在会话中临时切换 | P1 |
| TERM-18 | 终端行为选项 | 按 profile 或主机设置：上报的终端类型（`TERM`，默认 xterm-256color）、Backspace 发送 `^?` 或 `^H`、Delete 的编码、Alt/Option 是否作为 Meta 键 | P1 |

## 2. Shell 集成（SHELL）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| SHELL-01 | 跟踪当前目录 | 通过 OSC 7 / OSC 9;9 获取 cwd。新标签和分屏继承当前目录；SFTP 面板跟随远端目录 | P1 |
| SHELL-02 | 命令标记 | 通过 OSC 133 标记每条命令：在提示符之间跳转、复制单条命令的输出、标出失败的命令、在滚动条上显示标记 | P1 |
| SHELL-03 | 集成脚本注入 | bash/zsh/fish/pwsh 本地会话自动注入集成脚本（可关闭）；远端会话提供一键安装脚本 | P1 |
| SHELL-04 | 失败命令的修复建议 | 根据失败命令及其输出给出修复建议（规则库，可接入 AI） | P3 |

## 3. 本地终端（LOCAL）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| LOCAL-01 | Shell 自动探测 | pwsh、Windows PowerShell、cmd、Git Bash、**每个 WSL 发行版**、zsh/bash/fish。自动生成 profile，可以隐藏，删除后不会自动重新出现 | P0 |
| LOCAL-02 | Profile | 可配置命令、参数、起始目录、环境变量、外观；可设为默认 profile。macOS 默认以登录 shell 方式启动；Windows 上新开标签使用最新的系统环境变量（不需要重启应用） | P0 |
| LOCAL-03 | 随附 ConPTY | 优先使用随应用分发的 `conpty.dll` + `OpenConsole.exe`，缺失时回退到系统版本（W） | P0 |
| LOCAL-04 | win32-input-mode | 在 PSReadLine 等程序中完整支持 Ctrl+Space、Shift+Enter、Ctrl+Break 等按键（W） | P1 |
| LOCAL-05 | 管理员权限 profile | 在独立的提权窗口中打开，窗口有明显的视觉标识；**同一窗口内不混放管理员和普通标签**（W） | P1 |
| LOCAL-06 | “在此处打开” | 资源管理器 / Finder / 文件管理器的右键菜单入口 | P1 |

当前交付边界：Linux 自动 Unix Shell 的默认选择、隐藏、删除／显式恢复及参数／cwd／环境／稀疏外观编辑已限定验收。使用“本地 Profile → 管理 → 编辑”，参数按顺序填写并保留空值，cwd 留空继承或填写绝对路径，环境可覆盖或移除继承值；外观留空继承设置。保存不启动进程，下一次明确打开时应用，已有会话保留启动快照。任意命令／自定义 Profile、macOS 启动文件、Windows／WSL 与系统环境刷新仍是本表的未交付要求。详见 [编辑器边界](../architecture/profile-editor.md) 和 [当前进度](../development/plan.md)。

## 4. SSH

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| SSH-01 | 认证 | 密码、公钥（OpenSSH / PEM）、SSH Agent（OpenSSH agent、Pageant，以及经 agent 接入的 1Password 等）、keyboard-interactive / MFA | P0 |
| SSH-02 | 其他密钥格式 | PuTTY PPK（v2/v3）、OpenSSH 证书 | P1 |
| SSH-03 | 主机密钥校验 | known_hosts 校验（支持读取 `~/.ssh/known_hosts` 中经过哈希处理的条目）；首次连接时确认指纹；密钥变更时强提示并阻断连接 | P0 |
| SSH-04 | 跳板链 | 多级 ProxyJump，每一跳独立认证 | P0 |
| SSH-05 | ssh_config | 导入并实时读取 `~/.ssh/config`（Host、Include、Match 的常见用法） | P0 |
| SSH-06 | 连接复用 | 同一主机和身份的多个标签、SFTP、端口转发共用一条 SSH 连接 | P1 |
| SSH-07 | 保活与重连 | keepalive；断线后自动重连（可配置），重连时保留屏幕内容；系统从睡眠中唤醒或网络切换后立即检测连接，不用等到 keepalive 超时 | P1 |
| SSH-08 | Agent 转发 | 按主机开启，默认关闭 | P1 |
| SSH-09 | 旧算法兼容 | 按主机开启 ssh-rsa(SHA1)、dh-group1/14-sha1、CBC、hmac-sha1，用于老旧网络设备 | P1 |
| SSH-10 | 连接后动作 | 设置远端环境变量、连接后自动执行的命令或脚本 | P1 |
| SSH-11 | X11 转发 | 使用本机已有的 X Server（XQuartz / VcXsrv / 本机 X），**不内置 X Server** | P2 |
| SSH-12 | 终端内文件传输 | Zmodem（rz/sz）、trzsz | P2 |
| SSH-13 | 系统 OpenSSH 模式 | 对单台主机改为在本地 PTY 中运行系统的 `ssh` 命令，用于内置实现不支持的场景（GSSAPI/Kerberos、PKCS#11、ControlMaster、少见的 ssh_config 配置）。该模式下不能复用连接，也不能使用集成的 SFTP 和端口转发 | P2 |

## 5. 主机管理 Vault（VAULT）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| VAULT-01 | 主机、分组、标签 | 树形和列表两种视图；模糊搜索；最近连接；收藏 | P0 |
| VAULT-02 | 一台主机多个入口 | 同一台主机可以配置 SSH / SFTP / RDP / VNC / Telnet 等多个入口（Endpoint），不用复制主机。数据模型 P0 就按这个设计，RDP/VNC 入口在 P1 开放 | P0 |
| VAULT-03 | 身份复用 | 身份（Identity：用户名 + 凭据）可被多台主机引用 | P0 |
| VAULT-04 | 凭据安全存储 | 凭据加密保存，密钥托管在系统钥匙串；可选主密码保护（见 SEC-02） | P0 |
| VAULT-05 | 快速连接 | 在地址栏输入 `user@host:port` 或 `rdp://host` 直接连接，连上后可一键保存 | P0 |
| VAULT-06 | 密钥管理 | 导入私钥；生成 ed25519/RSA 密钥；导出公钥；一键把公钥部署到服务器（类似 ssh-copy-id） | P1 |
| VAULT-07 | 分组继承 | 分组可以设置默认身份、路由（Route）、外观和行为；主机逐层覆盖；在界面上显示每项设置来自哪一层 | P1 |
| VAULT-08 | 片段库（Snippet） | 可带变量占位符；可发送到当前窗格或多个窗格；支持绑定快捷键 | P1 |
| VAULT-09 | 导入 | ssh_config（P0）；`.rdp` 文件、PuTTY 注册表、MobaXterm、SecureCRT、Xshell、Termius CSV（P2） | P0 / P2 |
| VAULT-10 | 连接历史 | 记录连接时间、时长和结果，可以用来排序和快速重连 | P1 |

## 6. 文件（FILE）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| FILE-01 | SFTP 双栏 | 本地和远程并排；支持路径栏、书签、显示隐藏文件、多选、排序；上万个文件也能流畅滚动 | P1 |
| FILE-02 | 传输队列 | 可设并发数；支持暂停/恢复、断点续传、冲突策略（覆盖/跳过/重命名/较新覆盖）、限速；应用重启后继续未完成的任务 | P1 |
| FILE-03 | 拖拽 | 从系统文件管理器拖入即上传（P1）；从应用拖出到系统文件管理器（P2） | P1 / P2 |
| FILE-04 | 远程文件编辑 | 用内置 Monaco 编辑，或用外部编辑器打开；保存时自动写回服务器 | P1 |
| FILE-05 | 跟随终端目录 | SFTP 面板跟随同一会话终端的当前目录（依赖 SHELL-01） | P1 |
| FILE-06 | 高级操作 | 修改权限和属主、符号链接、以 sudo 身份操作、压缩或解压（在远端执行） | P2 |
| FILE-07 | FTP / FTPS | — | P2 |
| FILE-08 | 对象存储 / 网盘 | S3、WebDAV、OneDrive 等，和 SFTP 共用同一个文件面板 | P3 |

## 7. 网络与隧道（NET）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| NET-01 | 可组合的连接路由（Route） | 直连 / HTTP CONNECT / SOCKS5 / SSH 跳板 / 外部命令（即 ProxyCommand，例如 `cloudflared access`、`aws ssm`、`nc`）可以任意串联，并且**适用于所有协议**。P0 支持 SSH 的跳板，P1 支持全部协议和全部路由类型 | P0 / P1 |
| NET-02 | 端口转发 | 本地/远程/动态（SOCKS）转发；规则可保存；可随应用启动自动开启；断线后自动重连 | P1 |
| NET-03 | 转发状态面板 | 显示每条规则的状态、当前连接数和流量 | P1 |

## 8. 远程桌面 RDP（RDP）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| RDP-01 | 内嵌 RDP 会话 | 作为标签或分屏窗格嵌入工作区，支持位图编码和 RemoteFX | P1 |
| RDP-02 | 安全与认证 | TLS、NLA（NTLM / Kerberos）、服务器证书确认、域账号 | P1 |
| RDP-03 | 分辨率与缩放 | 窗口大小变化时动态调整远端分辨率；HiDPI；支持适应窗口、1:1、滚动查看三种显示方式 | P1 |
| RDP-04 | 输入 | 键盘（扫描码，与键盘布局无关）、中文输入（按 Unicode 发送）、鼠标、滚轮（含横向） | P1 |
| RDP-05 | 系统快捷键捕获 | 开启后 Win、Alt+Tab 等按键发给远端；Wayland 下无法捕获，改用工具栏按钮 | P1 |
| RDP-06 | 工具栏 | 发送 Ctrl+Alt+Del、全屏、截图、重连、显示延迟和帧率 | P1 |
| RDP-07 | 剪贴板 | 文本和图片（P1）；文件（P2） | P1 / P2 |
| RDP-08 | 外部客户端 | 生成 `.rdp` 文件并调用 mstsc、Windows App 或 FreeRDP/Remmina 打开，用于兜底 | P1 |
| RDP-09 | RD Gateway | 通过远程桌面网关（MS-TSGU）连接 | P2 |
| RDP-10 | 音频输出 | 远端声音在本地播放（在后端播放，不经过 WebView） | P2 |
| RDP-11 | H.264 显示 | 远端发送 H.264 视频流时，交给 WebView 的 WebCodecs 硬件解码；不支持时自动回退 | P2 |
| RDP-12 | 独立窗口 / 全屏 | 弹出到不带 WebView 的原生窗口，直接绘制画面 | P2 |
| RDP-13 | `.rdp` 文件 | 导入和导出 | P2 |
| RDP-14 | 设备重定向 | 磁盘（P2）、智能卡（P3） | P2 / P3 |

## 9. 远程桌面 VNC（VNC）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| VNC-01 | 内嵌 VNC 会话 | 基于 noVNC。支持的认证：无认证、VNC 密码、RealVNC RSA-AES、Apple ARD（macOS 屏幕共享）、VeNCrypt Plain | P1 |
| VNC-02 | 走隧道连接 | 可以通过 SSH 跳板或代理连接（依赖 NET-01） | P1 |
| VNC-03 | 显示与交互 | 缩放、剪贴板、只读模式、画质和压缩级别可调 | P1 |
| VNC-04 | H.264 | 服务端支持 H.264 时使用 | P2 |

## 10. 其他协议（PROTO）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| PROTO-01 | Telnet | 自动协商终端类型和窗口大小；支持自动登录脚本 | P1 |
| PROTO-02 | 串口 | 可设波特率、数据位、校验位、停止位、流控；支持本地回显、换行转换、十六进制显示；串口拔插时自动发现 | P1 |
| PROTO-03 | Mosh | 自研客户端，网络切换或 IP 变化后会话不断 | P2 |
| PROTO-04 | Docker / Kubernetes exec | 进入容器或 Pod 的 shell，作为一种终端会话 | P3 |

## 11. 工作区（WS）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| WS-01 | 命令面板 | 可以搜索并执行所有操作、主机和片段 | P0 |
| WS-02 | 快捷键 | 内置快捷键方案，用户可以自定义，并检测冲突 | P0 |
| WS-03 | 多窗口 | 标签可以在窗口之间拖动。拖动或刷新界面都不会断开会话 | P1 |
| WS-04 | 会话恢复 | 重启后恢复窗口、标签和分屏布局；本地会话在原目录重新打开，远程会话自动或手动重连 | P1 |
| WS-05 | 广播输入 | 把输入同时发送到选中的多个窗格，窗格上有明显标识 | P1 |
| WS-06 | 下拉终端（Quake 模式） | 全局热键呼出下拉式终端；品牌功能名待定 | P1 |
| WS-07 | 布局模板 | 把当前布局保存为模板，包括每个窗格的会话；一键打开模板 | P2 |
| WS-08 | 批量执行 | 在多台主机上执行同一命令并汇总结果；执行前需要确认 | P2 |
| WS-09 | 标签管理 | 复制当前会话（SSH 会话复用已有连接）；标签可以改名、设置颜色；关闭仍有活动会话的标签或窗口前确认（可关闭此确认） | P0 |

## 12. 命令行（CLI）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| CLI-01 | 基础命令 | `pulse .`、`pulse ssh <target>`、`pulse open <alias>`、`new-tab`、`split-pane`、`focus-tab`；用 `;` 串联多个子命令，用 `-w` 指定目标窗口；`pulse split -v` 在当前窗格旁分屏 | P0 |
| CLI-02 | 其他协议子命令 | `sftp`、`rdp`、`vnc`、`telnet`、`serial` | P1 |
| CLI-03 | URL 协议处理 | 注册 `pulseterm://`，并可选接管 `ssh://`、`rdp://`、`vnc://`、`telnet://` 链接 | P1 |
| CLI-04 | Shell 补全 | 生成 bash/zsh/fish/pwsh 的补全脚本 | P1 |
| CLI-05 | 管理类命令 | `hosts ls/import`、`run --hosts tag:x -- cmd`、`diag bundle`（导出诊断包） | P2 |
| CLI-06 | MCP | `pulse mcp serve`：只暴露用户允许的能力 | P2 |

## 13. 外观（UI）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| UI-01 | 主题 | 界面配色基于 Radix Colors 色板，选择一个强调色系即可换色；深色、浅色、跟随系统（深浅色各指定一套主题）；内置 PulseTerm Dark/Light、One Dark/Light、Catppuccin、Dracula、Solarized；终端配色可以按 profile 或主机单独设置 | P0 |
| UI-02 | 窗口背景材质 | Mica / Mica Alt / Acrylic（W）、Vibrancy（M）、纯色；按系统版本自动选择，系统关闭透明或进入节电模式时自动降级；Linux 只用纯色 | P0 |
| UI-03 | 终端区不透明度 | 和窗口外框的材质分开设置，默认不透明 | P1 |
| UI-04 | 自绘标题栏 | 标签栏放进标题栏；支持拖动窗口和双击最大化；尽量保留 Win11 的贴靠布局弹窗（需验证） | P0 |
| UI-05 | 多语言 | 简体中文、英文（P0）；其他语言由社区贡献 | P0 |
| UI-06 | 无障碍 | 所有功能都能用键盘操作；屏幕阅读器能读取界面控件；满足对比度要求；终端开启最低对比度保护（xterm `minimumContrastRatio`） | P0（键盘、对比度保护）/ P2 |
| UI-07 | 主题导入与配色库 | 导入 VS Code 主题（同时得到界面和终端配色）、Windows Terminal 配色、`.itermcolors`、Alacritty、base16/base24、Ghostty；内置 iTerm2-Color-Schemes 终端配色库供选择；导入时检查对比度 | P1 |

## 14. 安全（SEC）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| SEC-01 | 凭据不进前端 | 前端只拿到凭据的引用（SecretRef），密码和私钥只在后端解密使用 | P0 |
| SEC-02 | 主密码与应用锁 | 可选主密码；空闲一段时间后自动锁定；支持 Windows Hello / Touch ID 解锁 | P1 / P2 |
| SEC-03 | 证书与主机密钥确认 | SSH 主机密钥（P0）、RDP 服务器证书、VNC/TLS 证书（P1） | P0 / P1 |
| SEC-04 | 高风险操作确认 | 多行粘贴、广播输入、批量执行，以及由 CLI 或 MCP 发起的高风险动作，都需要用户确认 | P1 |
| SEC-05 | CLI 通信鉴权 | 本地 socket 权限设为 0600，并验证随机 token | P0 |
| SEC-06 | 日志脱敏 | 日志和诊断包不包含密码、私钥和终端内容 | P0 |
| SEC-07 | 本地审计日志 | 记录连接和敏感操作（可选） | P2 |
| SEC-08 | 远端访问剪贴板（OSC 52） | 远端程序写入本地剪贴板：可设为允许、每次询问或禁止，默认每次询问；远端读取剪贴板始终禁止 | P1 |
| SEC-09 | 打开终端中的链接 | 点击链接时显示完整地址；来自远端的 `file://` 和自定义协议链接需要确认才能打开 | P1 |

## 15. 同步与备份（SYNC）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| SYNC-01 | 加密导出和导入 | 导出一个用口令加密的备份文件，内容包括 Vault、设置、快捷键和自定义主题 | P1 |
| SYNC-03 | 恢复默认设置 | 可以单独重置设置、快捷键或主题，不影响 Vault 数据 | P1 |
| SYNC-02 | 端到端加密同步 | 通过 WebDAV / S3 / Git / OneDrive 等同步；以单条记录为单位合并冲突（采用 LWW + HLC） | P2 |

## 16. 扩展与 AI（EXT）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| EXT-01 | Wasm 插件 | 可扩展的内容：命令、补全、主题、新的传输协议、导入器；插件需要声明权限 | P2 |
| EXT-02 | MCP Server | 让外部 AI 在用户授权的范围内操作会话（读取输出、执行命令、截取桌面画面） | P2 |
| EXT-03 | AI 助手 | 解释输出、生成命令；使用用户自己的 API Key | P3 |

## 17. 发布与更新（REL）

| ID | 功能 | 说明 | 阶段 |
|---|---|---|---|
| REL-01 | 安装包 | Windows NSIS（安装到当前用户，不需要管理员权限）；macOS dmg（签名并公证）+ Homebrew cask，不上架 Mac App Store（ADR-023）；Linux AppImage / deb / rpm（暂不提供 Flatpak / Snap：在沙盒内启动宿主机的 shell 需要额外适配） | P0 |
| REL-02 | 自动更新 | 更新包带签名校验；分 stable 和 beta 两个渠道；可以关闭自动更新，或改为内网更新地址；应用自身的网络请求（更新、同步、崩溃上报）遵循系统代理设置 | P0 |
| REL-03 | 崩溃报告 | 默认关闭，用户主动开启后才上报；同时捕获原生崩溃（minidump）和 Rust panic；上报地址可配置为自托管服务；上报前脱敏（ADR-024） | P1 |
| REL-04 | Windows 便携版 | 解压即用，数据保存在程序目录下 | P2 |
| REL-05 | 用户文档 | 文档站点（中文和英文）；应用内可打开对应的帮助页；更新后展示更新说明 | P1 |

---

## 18. 非功能指标（CI 门禁）

| 指标 | 目标 |
|---|---|
| 冷启动到可以输入 | M 系列 Mac < 400ms；Windows < 700ms |
| 1 个标签空闲时的内存（含 WebView 全部子进程） | < 120MB |
| 20 个标签（19 个在后台） | < 250MB |
| 单个会话 10 万行 scrollback 的内存 | ≤ 20MB（待 S2 验证） |
| `cat` 1GB 文本 | 界面不卡死；Ctrl+C 在 100ms 内生效；后端和前端屏幕内容一致 |
| 本地回显延迟 | p95 < 16ms |
| RDP 1080p 办公场景 | 局域网内从输入到画面更新 p95 < 50ms；帧率 ≥ 30fps |
| 安装包体积 | < 15MB（不含随附的 ConPTY；含的话 < 18MB） |
| 前端首屏加载的 JS | < 300KB（gzip） |

## 19. 平台支持

| 平台 | 最低版本 | 架构 | 备注 |
|---|---|---|---|
| Windows | Windows 10 21H2 | x64、arm64 | WebView2 由安装器检测，缺失时自动安装 |
| macOS | macOS 12 | Apple Silicon、x64 | 窗口材质需要开启 `macOSPrivateApi`；不上架 Mac App Store（ADR-023） |
| Linux | Ubuntu 22.04 / Debian 12 / Fedora 38（WebKitGTK 4.1） | x64、arm64 | X11 和 Wayland 都支持；Wayland 下 RDP 无法捕获系统快捷键；H.264 依赖用户安装的 GStreamer 插件 |

## 20. 不做的事项

- 不做 shell 和脚本语言；不自研终端渲染器（v1 前）。
- 不内置 X Server；不支持 SPICE / X2Go / NX。
- 同一窗口内不混放管理员权限和普通权限的标签。
- 不内置大型 AI Agent 编排，也不接入多家 AI 厂商的 SDK。
- 不内置 OpenH264，不使用任何 GPL 组件。
- 不做云端账号体系；同步只使用用户自己的存储。

## 21. 里程碑

> 详细排期、分工和风险见 [development/plan.md](../development/plan.md)。

| 里程碑 | 内容 | 退出标准 |
|---|---|---|
| **M0 技术验证**（3 周） | [技术验证清单](../architecture/decisions.md#技术验证清单spike) S1–S21，优先 S1/S13、S2、S14/S15、S4 | 各项验收标准通过；未通过的项已按预案切换方案 |
| **M1 Alpha**（12 周） | 全部 P0 | 内部日常使用；非功能指标全部纳入 CI |
| **M2 Beta**（17 周，含春节） | P1：SFTP、端口转发、RDP/VNC、Telnet/串口、Shell 集成、多窗口等 | 公开 beta；互通测试矩阵全部为绿色 |
| **M3 v1.0**（6 周） | P1 收尾、稳定性、文档 | 崩溃率和性能达标 |
