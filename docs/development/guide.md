# 开发指南

## 专项实现索引

指南说明工具与开发流程；功能契约在架构文档，当前验收状态在[主计划](plan.md#15-动态交付清单与当前进度)和[验证记录](validation.md)。

| 范围 | 设计与验证入口 |
|---|---|
| 跨平台生产链 | [Windows 存储／Shell／原生验收](windows-product-core.md)、[CLI 控制](../architecture/windows-cli-control.md)、[Profile 环境刷新](../architecture/windows-profile-environment.md) |
| SSH 文件来源 | [配置导入](../architecture/ssh-config-import.md#windows-来源与共享确认算法)；Windows 同源组件用 `mise run test-windows-ssh-config` |
| 独立 Windows 前置包 | [无 Rust 存储测试包](../../scripts/windows-storage-probe/README.md)；不启动产品，不能替代普通用户生产验收 |
| 设置与键盘 | [设置导航](../architecture/workspace-settings-navigation.md)、[快捷键来源](../architecture/keymap-source.md)、[TERM／Delete／Profile](../architecture/terminal-behavior.md)、[Backspace](../architecture/terminal-backspace.md)、[Option／Meta](../architecture/terminal-option-meta.md) |
| 配色与继承 | [分组／SSH 行为覆盖](../architecture/terminal-scoped-behavior.md)；新会话冻结设置，旧会话不在线切换 |
| 输入与滚动 | [IME 边界](../architecture/terminal-ime.md)、[输入回底](../architecture/terminal-input-scroll.md)、[输出／手动回底](../architecture/terminal-output-scroll.md)、[可视响铃](../architecture/terminal-bell.md) |
| 页面与系统交互 | [后台视图](../architecture/terminal-background-views.md)、[链接检查与系统交接](../architecture/terminal-links.md) |
| 私钥导入 | `mise run probe-workspace-key-import`：原页面授权、密文保存／重开与退出；截图前清除输入，导入不授予连接权限 |
| 性能／资源 | [Linux 运行时评估](linux-runtime-performance-review.md)；受控缩放、交叉编译和 headless 不能代替系统输入／真实图形与平台验收 |

历史容量默认 100000（1000～100000 可设），新本地／SSH 会话采用，旧会话不缩容，冷区预算 16MiB。入口或默认键位变化需同步命令目录／面板、keymap、dispatcher、后端 Action／Capability，并比较 Rust／TS 原始策略定义。各项只运行受影响检查；注册和完整任务参数以 `.mise.toml` 为准。
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
├─ NOTICE / THIRD_PARTY.md  # 已生成工作区署名材料；缺项与发行审查仍阻断
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

启动／关闭故障阶段取证见[原生窗口调查](native-window-close-investigation.md)：显式 `workspace-startup-diagnostics` 诊断构建只向调用者已有的私有目录写固定阶段码，每进程64条。以 `PULSE_STARTUP_DIAGNOSTICS=1` 和 `PULSE_WORKSPACE_STARTUP_TRACE_DIR` 运行现有关闭任务，仍保留原启动／关闭期限；正常构建不读该目录变量。`mise run check-startup-diagnostics` 配对记录器回归、严格Clippy和收据拒绝检查，已接入Linux CI；Linux诊断构建24轮阶段／PID及实际退出已关联通过，不能以有限成功循环关闭历史故障。

页面确认和CLI展示回执的有界等待见[启动调查](native-window-close-investigation.md#页面确认的有限等待与恢复)：前端只等待五秒，页面撤销立即结束等待，迟到结果不推进流程；不取消原生工作或自动重发。超时使用现有刷新入口，后端授权与原生启动／关闭期限不变。仅运行受影响Readiness／冷SSH／CLI新标签／可见性回归、类型／双语／格式和产品前端构建，沿用现有CI；受控Chrome检查不算原生WebView故障注入。

原生恢复专项使用 `mise run probe-workspace-page-readiness`：仅Linux验证构建的 `workspace-readiness-probe` 按固定模式延迟第一个文档或CLI展示回执6500ms，再通过真实AT-SPI刷新、PTY输入与实际进程回收检查恢复。普通生产构建不含故障注入；相同前端、原授权实现及原期限保留。Linux两个场景及两GUI／PTY退出已通过，不代替历史黑窗／SIGSEGV定位。

工具版本只修改 `.mise.toml`，不另建 `rust-toolchain.toml`、`.nvmrc` 或其他版本文件。库依赖仍在 Cargo / package manifests 中声明，提交 `Cargo.lock` 和 `pnpm-lock.yaml`；mise 管理执行这些依赖的工具。配置尚未信任前不要执行任务。

**当前可执行：**

```bash
mise trust .mise.toml              # 先审阅配置，再信任
mise install                      # 安装固定版本的工具
mise run bootstrap                # 依据 pnpm-lock.yaml 安装依赖
mise run check                    # Rust 格式/clippy/依赖分层 + TS 类型/Biome
mise run check-ssh-adapters        # Linux探针复用边界 + 完整生产SSH工作区的严格Clippy
mise run test                     # Rust 单元/属性测试 + Vitest
mise run probe                    # Rust 与 xterm headless 共享样例比较
mise run probe-workspace-system-appearance # Linux：真实门户主题信号→Tao/GTK/WebView，显式保存/刷新/重开/恢复继承
mise run probe-workspace-terminal-system-theme # Linux：系统深浅选新PTY配色，OSC前景/背景与真实像素、冻结/刷新/恢复/独立重开及退出
mise run probe-workspace-ime # Linux：真实IBus拼音候选、取消零输入、选词单次提交、刷新不重复及原生退出（需ibus/ibus-gtk3/ibus-libpinyin）
mise run probe-workspace-ime-popup # Linux：严格要求Escape后候选窗口隐藏；IBus 1.5.32隔离环境仍失败，不能以位置／字节通过替代
mise run probe-workspace-fcitx-input # Linux：Fcitx5原窗口光标跟随／严格隐藏／精确字节／刷新／回收；明确不移动窗口
mise run probe-workspace-fcitx # Linux：完整窗口移动几何门禁；Fcitx GTK 5.1.3仍复现旧坐标，尚未通过
mise run probe-pty                # Unix 真实 PTY、查询回写、尺寸与背压
mise run probe-ssh-private-key-use # Rust-only：Vault密文→实际Ed25519签名，独立于产品绘制验收
mise run probe-profiles           # Unix只读Shell发现、自动profile规则与字面PTY启动
mise run probe-login              # 公开默认login builder真实argv0/程序选择及回退限制
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
mise run audit                    # Rust＋前端独立完成；任一失败仍阻断，最新 summary 指向独立 runs/<UUID>/ 日志，旧批次保留于 target/probes/dependency-audit/
mise run audit-check              # 审计先失败仍执行下一项、无效回执、跨批次日志保留与陈旧报告的子进程反例
mise run third-party-write        # 校验锁定归档＋父包/VCS/内嵌commit/源码官方引用，生成NOTICE/清单/原文；需网络
mise run third-party-check        # 39项行为反例＋离线摘要、来源关联和完整集合检查，不代表发行批准
mise run third-party-release-check # 缺完整原文或只有许可链接仍失败，许可证政策另行审查
mise run distribution-review       # 五目标冻结依赖投影＋12项反例，不替代实际发行或许可批准
mise run fmt                      # 统一格式
mise run desktop-check            # 打包前端并检查真实 Tauri 适配层
mise run desktop                  # 打开原生开发验证界面（不是完整终端）
mise run probe-channel            # native PTY/窗口授权/EOF ACK/Commit/Data idle Marker及精确屏幕修复，两轮headless+两轮可见DOM及7组测量
mise run probe-native-parallel     # Linux原生测试默认/64线程/重叠进程有界复现；不改变原5秒断言，不计历史故障修复
mise run probe-channel-v2         # --probe-v2固定UTF8样例，两轮headless/DOM、版本收据和8220-byte窗口
mise run probe-window             # Linux独立Xvfb/Openbox的真实native minimize/unminimize，同样验证PTY重绑/DOM/EOF
mise run probe-window-external    # 独立xdotool进程通过WM最小化/恢复，应用不调用自身minimize API
mise run probe-route              # 独立Direct/HTTP↔SOCKS双认证、真实binary半关闭与拒绝清理
mise run probe-cli                # cli-probe可选feature，私有UDS/13独立固定子进程/握手与长度/权限/EOF/重复参数前置拒绝验证
mise run probe-cli-window         # cli-window-probe显式构建；真实CLI→native可见注册/page-load/销毁，独立计时
mise run probe-cli-startup        # 只读daemon真实进程竞争/存活端点保护/SIGKILL后安全恢复与旧token拒绝
mise run workspace-cli-build      # opt-in Linux：warm focus/profiles/focus-tab/new-tab/split/ssh；cold focus/profiles/new-tab，创建/SSH须原窗口确认
mise run probe-workspace-cli      # 真WebView/Openbox三代原页focus/只读profiles且不增PTY创建数、同PTY输入、socket/token与Root排空；临时XDG只在/data/temp
mise run probe-workspace-cold-new-tab # 冷GUI取消/断开/refresh零PTY；16s批准一次、实际回显/同PID恢复及原生退出
mise run probe-workspace-cold-concurrent # FF/PP/FP独立CLI同冷启动，actual2GUI竞争/唯一brokerowner/0PTY/端点保留与真实退出
mise run probe-workspace-cold-failures # strace只跟踪process/connect：43残缺/占用/不安全端点、EOF/5s超时及broker失败，不修复、不cold重试
mise run probe-workspace-cli-new-tab # 默认取消/CLI断开/未决刷新不创建，明确批准创建一次、实际可见modal与新PTY回显/refresh恢复、两PID与端点退出
mise run probe-workspace-cli-split # 原单pane左右/上下双PTY，Cancel/CLI断开/换页拒绝，慢批准/第二pane输入/同两PID恢复和root排空
mise run probe-workspace-cli-selection # CLI原页一次选择两个已有Root会话；精确PID输入、重复选择、split lease保持与实际退出
mise run probe-workspace-cli-aliases # open <已发现本地Profile>、split-pane -v/-h；复用原页创建确认、取消/换页、实际输入/同PTY恢复/回收
PULSE_CLI_PENDING_PRESSURE=1 mise run probe-workspace-cold-sensitive # actual双CLI冷创建竞争＋48暖请求，原modal保持／零或单PTY／实际退出
mise run probe-workspace-cli-capacity # 实际8个认证槽，第9个CLI拒绝；关闭单槽后回收、端点保持、原窗口三次聚焦及退出
mise run probe-workspace-cli-window-selector # -w workspace闭合单窗口选择；实际三页focus／同PTY输入／端点排空
mise run probe-workspace-cli-multiwindow # Linux同进程原窗确认建空窗/精确选窗/双PTY刷新/主窗先关与最终退出；SHA见cli-multiwindow-acceptance.json；1063Rust/1208前端、7忽略不计，清理25.2GiB
mise run probe-workspace-cli-multiwindow-ssh # Linux双窗Vault/原页批准、密码/选定软件Agent/真实三跳；343份SHA见cli-multiwindow-ssh-acceptance.json；冷SSH/本地多窗/七链/六冷分屏及1063Rust/1208前端通过，7忽略不计，清理12.4GiB
mise run probe-workspace-cli-ssh # Linux opt-in保存host/identity UUID，原page/Vault epoch+source批准；锁定/Cancel/断开/换页/同库外部变更auth0，新source慢批准auth1、真实输入/refresh/resize/SDK-native-root排空
mise run probe-workspace-cli-ssh-private-key # 同一原epoch捕获、联合双目录消歧；锁定/Cancel/断开/换页/旧source零签名，新source慢批准一次真实签名、零密码回退及TermView刷新/输入/resize/退出
mise run probe-workspace-cli-ssh-agent # 原SavedGesture三目录唯一ID、原epoch继承、锁定/取消/导航/断连零签名、原窗批准所选Agent一签名、终端/刷新/重开/退出
mise run probe-workspace-cli-agent-interactive # --agent-interactive：冷暖批准、所选Agent签名、问答取消与真实退出
mise run probe-workspace-cli-password-interactive # --password-interactive：原捕获、冷暖明确批准、零提前密码使用、问答取消/刷新/四连接退出
mise run probe-workspace-route-password-interactive # 每跳密码＋两轮MFA、完整第一因素成功拒绝、刷新、叶跳取消父链排空
mise run probe-workspace-route-agent-interactive # 三跳密码/Agent+MFA转发/Agent+MFA终端：逐跳批准、刷新与取消排空
mise run probe-workspace-password-interactive # 保存/临时密码+MFA：首次指纹独立核对、取消零写、新主机保存、刷新/独立重连、16连接/两窗退出
mise run probe-workspace-settings-contract # 全局保存重开、刷新按键字节、GUI/CLI新旧TERM/前景色/历史冻结，主/备用/结束查找、真实PTY滚动/响铃/回放隔离、链接/文件检查与GTK剪贴板回读、隔离GIO/Chrome本机页面交接与三PTY退出（需已有Chrome/Chromium）
mise run probe-workspace-history-stop # 独立真实100000行PTY：双向停止保页、重查、自然EOF只读及零输入／物理退出；观测延迟含AT-SPI开销
mise run probe-workspace-scroll-settings # 显式输入/输出滚动、保存旧视图不变、同PTY刷新、CLI新会话继承、恢复默认与精确字节/两PTY退出
mise run probe-workspace-dpi # 顺序1x/2x受控GTK缩放：实际物理窗口、四组逻辑PTY网格、字体保存/刷新/重开及六会话退出；实际多屏另验
mise run probe-workspace-ssh-settings # SSH原生系统配色与组/入口固定覆盖、远端OSC10和终端像素、TERM/按键、清除覆盖/冻结/刷新、异步滚动/显式开关、软换行/EOF及五连接/门户退出
mise run probe-workspace-startup # release新进程参考：Profile可见、选择到可输入、真实首条PTY输入、六次退出；Linux数据不判定Mac/Windows启动目标
mise run probe-workspace-performance # Linux真实WebView整进程树PSS/RSS：单会话、十万行、20标签；采集完成与性能达标分别记录
mise run probe-workspace-performance-release # 同场景release构建，记录原产品MB目标；软件渲染基线不能代替硬件/跨平台验收
mise run probe-workspace-load # release真实cat 1GiB和Ctrl+C到cat物理退出延迟；不替代回显p95／完整屏幕状态／硬件验收
mise run check-presentation-floor # 15项：像素时钟、PSS／映射、失败清理、锁定资源及公开策略请求／实际读回核验；不是产品性能
PULSE_PRESENTATION_RUN=minimal-v1 mise run probe-presentation-floor # 原样像素helper：X11直接绘制／WebKit立即DOM／一帧DOM各两组100项
PULSE_PRESENTATION_RUN=xterm-v1 PULSE_PRESENTATION_SUITE=xterm mise run probe-presentation-floor # 锁定xterm6.0.0页面内脏行／整屏回显对照；无PTY／生产IPC，不替代产品门禁
PULSE_PRESENTATION_RUN=runtime-memory-v1 mise run probe-runtime-memory # 原生／最小WebKit／页面xterm镜像六组，全进程PSS/RSS；不是产品预算通过
PULSE_WEBKIT_POLICY_RUN=public-policy-v1 mise run probe-webkit-policy # 夹具公开API默认／never／never／默认内存及像素对照，读取实际策略；不修改产品
mise run check-tauri-memory # 同Tauri宿主采集收据、空白页零授权、早退／中断与旧收据拒绝9项回归
PULSE_TAURI_MEMORY_RUN=same-host-v1 mise run probe-tauri-memory # 显式Linux验证feature；同二进制空白／工作区空窗ABBA，原生物理退出核查
mise run probe-workspace-load-state # 独立验证构建：真实1GiB标记绘制后、刷新前核对原输出的可见网格／样式／光标／主备用屏；不证明隐藏解析器／全部历史
mise run probe-workspace-echo # 100次原生XTest→PTY→字形像素回显p95；需X11/XTest开发库，输出原始像素／PTY JSON，helper在私有目录；软件环境不代表硬件验收
mise run probe-ssh                # optional xtask SSH feature，隔离loopback pinned hostkey/auth/PTY/系统OpenSSH客户端验证
mise run probe-ssh-owned-session  # 显式会话动作：私库密文→真实SSH→根actor，换页/EOF/read取消与SDK+流join；合成PTY尺寸通知与显示应用，不冒充kernel ACK，非产品/SSH GUI/OS PTY
mise run probe-ssh-system         # Linux临时sshd与真实PTY，russh公钥认证/native尺寸/EOF验证
mise run probe-ssh-jump           # 双SSH/GBK actor与通用HTTP→SOCKS BoxStream接pinnedSSH/错key拒绝/EOF清理
mise run probe-ssh-pressure       # 真远端window-aware生产者；ACK背压/detach/timeout及共享连接HOL限制
mise run probe-route-ssh-pressure # 上述三路径全部经HTTP→SOCKS真实代理链；独立JSON拒direct证据混用
mise run probe-ssh-certificate    # 临时CA、principal/有效期/critical options/私钥持有证明
mise run probe-ssh-agent          # 私有软件Agent，仅合成identity的签名/auth/删除/清理
mise run test-ssh-agent-transport # 当前平台有界Agent协议/撤销/所选签名，Windows含自有命名管道夹具；非用户服务/GUI验收
mise run probe-ssh-legacy         # 仅test profile旧KEX/MAC，安全default前后明确拒绝
mise run probe-encoding-attachment # validation-only GBK actor/v2原子重绑/history/双坐标headless渲染
mise run probe-encoding-window    # 显式encoding-probe build：固定分类GBK actor→真实Channel→DOM，两轮不同source/UTF8边界
mise run probe-workspace-keymap-state # 当前表单票据期间独立atomic外改→CAS拒写/新源确认；两真实应用进程同GUI保存文件重开，各3页/PID回收/字节不改。
mise run probe-workspace-keymap # 真实Linux当前页GUI保存/禁用/恢复/物理两步键位，focus A→B→A旧前缀拒绝，3页同PTY/PID/精确输入与实际Root排空；唯一/data/temp fixture，非三平台或systemIME。
mise run probe-keymap # 固定内存Rust→TS6投影/1158策略对照，三独立进程私有keymap.toml旧快照拒写/提交重开/自有fixture清理；无真实用户配置或GUI键位编辑。
mise run probe-workspace-commands # Linux当前页固定7动作palette：真实CtrlShiftP/检索/Settings跳转/empty取消/三页同PTY无按键漏写/default保留与确认close；不是通用Capability或可配置快捷键。严格CSP与原8s/80s截止。
mise run probe-workspace-clipboard # Linux原生GTK剪贴板→WebView用户事件：copy／单行／审批取消／CtrlC中断／换页和resize撤销
mise run probe-workspace-selection-copy # Linux默认关闭／字段搜索保存／旧视图冻结／同PTY刷新／显式false与恢复继承／真实鼠标和系统剪贴板／最终输入快照及GUI实际退出
mise run probe-workspace-right-click-paste # LinuxGTK单次原页原连接读取，单行／多行批准取消／刷新退休／鼠标报告优先／迟到X11回调与退出等待；非跨平台结论
mise run probe-workspace-bracketed-paste # 真实 Bash／GNU Readline 缓冲：多行取消／批准后等待Enter、待决输入刷新、UTF8预算及模式开关；非系统IME
mise run probe-workspace-vim       # 真实Vim：审批／缩进及Unicode原文／插入缓冲跨页／一次撤销／文件字节保存／未保存关窗回收；非系统IME
mise run probe-workspace-history-copy # Linux候选：600长逻辑行／软换行Unicode／真实鼠标跨历史选择／GTK字节核对／同进程刷新；未计验收
mise run probe-workspace-settings # Linux设置：default14／逐字段CAS／行高与字距独立内核网格／刷新同PTY／两进程重开／恢复继承／退出排空
mise run probe-workspace-font # Linux字体：预设与许可零写／明确保存／缺失主字体后备／同PTY重新加载与退出；当前生产组合／私有构建／搜索定位及3个PTY物理退出已重验；不计连字
mise run probe-workspace-ligatures # Linux按需WebGL：明确保存运算符连字／绘制完成ACK／同PTY原文输入／独立重开；不计GPU性能或跨平台
mise run probe-workspace-profiles # Linux私有DB：自动Profile隐藏／恢复、现有PTY存活、换页／新进程持久、旧页／目录拒写
mise run probe-workspace-profile-default # Linux默认选择：保存/清空零启动、隐藏不可用、刷新/双进程重开、明确一次启动与退出回收
mise run probe-workspace-profile-lifecycle # Linux删除/恢复：取消零写、默认墓碑、原PTY输入/刷新、双进程墓碑持久及明确恢复零启动
mise run probe-workspace-profile-editor # Linux真实编辑：参数/cwd/env原生字节、GUI/CLI按键与前景色覆盖、旧会话冻结／刷新／重开及恢复继承
mise run probe-ssh-first-pin       # Unix固定TCP/私库：实际offered key与原registry page，原页/最终换页回滚、新页批准、重开strict peer；auth0，非真实WebView/产品连接
mise run probe-workspace-fingerprint # 显式workspace-ssh/Linux：独立OpenSSH＋真实键鼠，未知取消／刷新／明确保存／匹配重开／服务器换key阻断；auth0、非SSH会话。随后普通workspace-preview-build恢复默认工厂关闭。
mise run probe-workspace-ssh-session # 显式workspace-ssh/Linux：合成私库密文＋实际russh peer→真实TermView、auth1、三唯一键盘echo、换页同Root、SSH尺寸入队、窗口/确认close与native drain、EOF final画面/两次刷新同Ended快照/输入持续关闭、握手挂起时取消/换页实际流退出与零会话；remote exit-status7不作本地退出证明，非远端OS/跨平台验收。fixture显式重建，不沿用基础probe的默认xtask。
mise run probe-workspace-ssh-private-key-session # 显式 workspace-ssh/Linux：已存加密 Ed25519→真实签名／TermView，三次回显／同连接刷新／尺寸通知／EOF／取消导航与 native drain；零密码回退
mise run probe-workspace-ssh-route # Linux保存路线双跳：每跳实际peer/身份明确批准、精确首次pin、取消零认证、终端输入/刷新/独立重开与父native退出
mise run probe-workspace-ssh-agent # Linux后端发现两钥、明确保存一钥、取消零签名、所选Ed25519单钥终端/刷新/重开/实际退出；不计硬件或其他平台
mise run probe-workspace-ssh-agent-route # Linux密码外层/Agent中间与叶：三类联合来源、两次所选签名、每跳实际描述与SQL pin、叶/父从内到外回收
mise run probe-workspace-cli-ssh-route # Linux CLI原SQL快照／原epoch：初始目标批准、三peer逐跳批准、首跳取消零认证、两selected Agent签名与父native退出；已归档39份SHA证据
mise run probe-workspace-cold-ssh # Linux密码冷窗口已验：锁定取消／解锁后独立确认／精确目标批准／一次认证／生产刷新双原文echo／端点与peer退出；冷Agent/跳板未计
mise run probe-workspace-cold-ssh-agent # Linux软件冷Agent已验：两取消零签名／独立点击／仅第二钥匙签名一次／零密码／刷新原文与native退出；最终共同回归通过，24份SHA见cold-agent-acceptance.json
mise run probe-workspace-cold-ssh-route # Linux冷password/Agent三peer已验：首跳取消零认证／精确逐跳批准／外password1＋中叶Agent各1／两精确pin／刷新与父连接/转发退出；33份SHA见cold-agent-route-acceptance.json，批次清理完成
mise run probe-workspace-cold-split # Linux六模式已验：双明确确认、首步取消零PTY、次步取消/断连/换页保留首个、左右/上下双pane原文刷新同PID及native/端点退出；56份SHA见cold-split-acceptance.json
mise run probe-workspace-cli-chain # Linux七链+六coldsplit兼容已验：整链预检、同实例各步确认、原页呈现ACK、失败保留前缀、双PTY原文刷新与退出；111份SHA见cli-chain-acceptance.json；默认1050Rust/1204前端、7忽略不计；清理20.5GiB，审计/历史关闭仍阻断
mise run probe-workspace-saved-ssh-alias # Linux八模式功能验收：open --host精确保存入口/default身份、冷暖密码/Agent/三跳、同名/缺默认拒绝与原文刷新；305份SHA已归档，历史SIGSEGV/非零关闭仍待定位
mise run probe-workspace-native-close # Linux累计88正常+8 GDB关闭观测、3项真实退出屏障通过，254份SHA保留前批129份；历史SIGSEGV未定位不计修复，新增72次（48次未决时序）正常，见native-close-pending-acceptance/962d5f04；默认各12轮，可选PULSE_COLD_CLOSE_CYCLES<=32／PULSE_COLD_CLOSE_GDB=1／PULSE_COLD_CLOSE_PHASE=cancel-unobserved或pending-dialog，失败取栈PULSE_COLD_CLOSE_FAILURE_GDB=1
mise run probe-ssh-config # Host/Include/常见Match、7样本OpenSSH差分、Unix固定文件与显式Include集合/原实例源确认；不授予导入/身份读取/连接权限
mise run probe-workspace-catalogue-batch # Linux真实三目录搜索/取消零写、一次CAS批量墓碑删除、Vault逻辑内容保持、刷新/进程重开及零启动
mise run probe-ssh-config-page # 原页配置元数据/一次性源确认、旧票据/跨实例拒绝；取消后的真实阻塞I/O与退出屏障（Unix/可选SSH）
mise run probe-workspace-ssh-config # Linux真实预览/源冲突/刷新/进程重开；专用私目录、AT-SPI可见完成状态、全库摘要不变及零会话启动
mise run probe-ssh-config-import # 原页独立保存授权、全源重验与数据库CAS；提交前源变回滚和取消后实际SQL额度保留
mise run probe-workspace-ssh-config-import # Linux两步保存、取消／源冲突零写入、同主机ID刷新／重开；仅主机元数据变化，零终端启动
mise run probe-workspace-ssh-config-status # Linux可见预览源变化／保留旧绘制／原票据不消费／刷新重开；全库不变、零会话
mise run probe-workspace-selection-copy # Linux真实Settings开关、默认关闭、同PTY刷新、鼠标选择与GTK系统剪贴板；保持原安全粘贴门
mise run probe-workspace-hosts    # Linux私有DB：当前页主机元数据增改删／字面显示／换页及三进程重开；零Shell启动，非SSH连接验收
mise run probe-workspace-identities # Linux私有DB：缺Vault／身份名称+用户名增改删／刷新／三进程重开；非密码认证／连接验收
mise run probe-workspace-key-identities # Linux私库：私钥身份元数据／双引用／刷新／四进程重开／删除墓碑；零终端启动，非密钥导入或认证验收
mise run probe-workspace-identity-password # Linux预览：已保存身份／原epoch单次ticket，锁定拒绝、密码只接收→密文保存、刷新／重开及独立进程固定字节验证；不读取密码／连接SSH
mise run probe-workspace-vault    # Linux预览单向主密码：创建保持锁定、错误密码拒绝、前端确认解锁、锁定／刷新／私库重开；不读取秘密或连接SSH
mise run probe-workspace-vault-revocation # Linux认证已准入后关对话框／换页：无迟到启用、Profile读取等待实际旧信用退出
mise run probe-workspace-labels   # Linux窗口显示偏好：原生modal编辑不泄入PTY、固定标色RGB、换页当前目录校验
mise run probe-workspace-split    # 一个任务依次验split／split-adjust：侧栏可见控件、显式折叠、55%比例跨方向保留、键盘焦点、刷新及独立关闭／实际回收
mise run probe-workspace-tabs     # Linux双Profile PTY：标签键盘切换／活动选择刷新／独立关闭／完整回显像素／退出排空
mise run probe-workspace-close-duplicate # Linux单PTY：命令面板复制会话（来源已知）／confirm_close=false 免确认关闭／进程回收
mise run probe-workspace-eof      # Linux隔离Profile：自然EOF及结束后两次刷新，保画面/禁输入/End ACK/独立native退出码与回收
```

`probe-local-session`另含真实PTY detach6秒后继续Running/解析Unicode marker/准确新generation重绑、旧input拒绝、EOF ACK/direct-child join；报告schema4的detached证据独立于GUI有限5秒resume策略，不当长期GUI验证。

`probe-pty` 当前在 Unix 运行，依赖系统 `/bin/sh` 和 `stty`，不需要显示服务；输出 `target/probes/pty.json`。监督器最长 25 秒，交互 helper 自带 10 秒截止时间。三平台 CI 新增 `probe-pty-portable` 和 `probe-local-session`，不依赖Windows的stty；仅有Linux运行证据，不能把注册成功当作ConPTY验收。Windows helper只测canonical输入和native尺寸回读，不测raw/Win32键盘、child尺寸query或随附conpty.dll。

`probe-channel` 报告schema 4要求两轮headless及两轮DOM的native session运行、隐藏peer窗口跨owner拒绝、真实Destroyed撤销且会话存活、EOF同边界digest以及原有7组传输测量。每次约3.1MB Unicode输出含截断UTF-8尾部；客户端一次延迟ACK 250ms，额度最多1帧/8212字节（不含6字节outer header），callback完成才ACK。相同label重建与旧nonce/generation ACK也在真实WebView验证。复用同一sink协议运行`@xterm/headless`和`@xterm/xterm` 6.0.0 DOM renderer；DOM每帧在parser callback及两次animation机会后ACK（不是compositor回执），最终12行DOM文本逐行与buffer核对。Tauri动态页nonce通过公开documentOverride授予可信renderer style，无nonce拒绝，不放开unsafe-inline；helper固定且40秒截止，不允许用户指定shell或命令。新增有限`probe_session_input`端点（只接受固定字符/≤1KiB UTF8）：两轮DOM由textarea合成ASCII键盘、中文/emoji composition和Enter，通过onData/串行有界队列/generation+lease授权后在PTY显示INPUT_OK；headless走同队列但不是键盘。报告要求inputChecked、keyboardChecked、compositionChecked及pasteBlocked；合成事件不等于OS键盘/IME验收。前端InputQueue含活动IPC最多32项/8KiB、一秒截止，超时不自动重发；paste始终阻止，host撤销/错误按generation detach。每轮DOM连续3次ResizeObserver自动适配（初始→104×12→60×10→104×12，尺寸来自cell/content box，50ms去抖），native ACK后按Reconfigured逐次重捕获；随后native-only样例不改host.hidden、不绑定page隐藏来源，调用finite probe_session_native_cycle仅预留窗口动作并hide；typed原生hidden事件独立触发SurfaceVisibility退役旧view/pause，backend最长2秒等真实pause票据后才产生HIDDEN_SEED，100ms后show/可见回读。native restore触发controller，恢复callback先等native动作Promise完成及无旧Channel帧证据，再自动resume/准确重绑（不是探针直接pause/resume）。每generation控制器1.5秒IPC截止，early show等detach收据，失败不自动重试，handoff/abort清理监听。新增typed nativeVisibilitySample定向事件与owner授权probe_session_native_snapshot，先listen后snapshot，再合并原生来源；Focused/Resized/DPI只触发有限runtime采样、无idle poll，false/true必须同incarnation且revision递增。报告要求nativeSignalChecked/nativePauseChecked以及后台nativeOnlyChecked，证明实际native事件收到且唯一来源驱动pause，不能只靠命令返回值；headless nativeOnlyChecked=false。Rust控制show不依赖hidden JS，nativeWindowChecked由backend赋值，DOM报告必须true、headless必须false。Destroyed还会立即关闭gate/唤醒resume waiter；CloseRequested（可能阻止）与失焦均不会当成销毁。报告要求fitChecked/visibilityChecked、resizeCount=3/pauseCount=1及最终columns/rows，并核对native syscall成功次数；headless两项计数仍0。旧ACK/input/resize/detach/resume、旧帧与旧Promise结果不能影响新绑定。普通probe-channel不算系统最小化证据；独立probe-window取得Linux/X11/Openbox的受控native minimize/unminimize证据，仍非用户最小化自动事件、真实持续拖动、跨屏DPI或全平台fit验收。权限/错误仅使用稳定标识，不把会话内容或底层主机信息写入报告。

无显示服务的 Linux 运行 `probe-channel` 需要 `xvfb`、`dbus-x11` 和 coreutils 的 `timeout`；探针监督整个 Xvfb/DBus/WebView 进程组，最长运行 180 秒。Linux 原生构建还需 GTK3/WebKitGTK 开发包，Windows/macOS 使用对应系统开发环境。原生依赖只在 `pulseterm-desktop` 的 `desktop` feature 启用，默认 headless 检查无需 GUI 库。

`probe-window` 当前仅Linux：需`openbox`、`x11-utils`（xprop）、Xvfb、DBus和coreutils timeout。总是新建独立Xvfb display，不更改已有桌面的WM；wrapper等待EWMH `_NET_SUPPORTING_WM_CHECK`最多5秒再启动app，EXIT/INT/TERM清理自己启动的Openbox，外层180秒timeout覆盖整个组。CLI `--probe-window`选择minimize模式（不能通过JS指定），native cycle改为minimize→is_minimized=true→后台输出/100ms→unminimize/show→visible&&!minimized读回，之后复用原来的自动resume/精确重绑。输出`target/probes/window-minimize.json`与`window-manager.log`，不会覆盖普通channel.json。server与xtask分别要求DOM nativeMinimizeChecked=true、headless=false，普通probe-channel模式要求二者false；mode错配/非bool/缺失不能通过。Linux CI独立步骤执行并上传两种JSON及WM log；Openbox仅测试系统前置，未成为打包或Cargo/pnpm依赖。失败不会fallback成CSS隐藏或假成功。已通过native事件作为唯一source独立驱动pause/resume；窗口minimize动作仍由runtime测试命令发起，未实际人工点击标题栏按钮；Wayland/其它WM或Windows/macOS仍未验收。

`probe-window-external` 当前仅Linux私有X11：额外需xdotool。CLI --probe-window-external使应用正常动作路径只写minimize/restore:decimal-generation请求（atomic rename），独立controller wrapper用umask077临时目录传给直系app进程；按该app PID查找唯一可见window，每次getwindowpid核对、timeout2s xdotool minimize/activate，经WM动作改变native状态。不是应用调用minimize/unminimize/show，也不接受任意XID或shell源。应用仍按runtime readback/native-only controller pause/隐藏输出/resume验收；报告window-external.json、window-external-actions.log，externalWmChecked须true且headlessfalse。xtask再要求两轮共4条minimize→同gen restore、generation递增且正整数规范、长度≤128B；普通模式externalWmChecked=false。脚本cleanup停止自己app、删除私有dir，退出码保留；Openbox由外层owned wrapper清理，外层timeout监督整组。Linux CI注册并上传新log，不作为CI已运行证明。xdotool只系统验证前置；不是人工点击按钮、其它WM/Wayland/平台或任意长时间外部最小化验收。

finite terminal sink现在使用GenerationViewHost/ViewScope按后端Begin管理surface资源：consumer/render/query/onData/input/idle作用域统一清理，异步创建在abort时即时reject，晚到结果清理且不影响新generation。报告schema4新增viewHostChecked，要求每DOM3次resize+pool恢复+native恢复共5次scope重建，重复旧renderer.dispose不得改变新.xterm root。另有实际view-pool.ts/ViewHost与ViewSlot接主React容器（32总/6 parked/5分钟TTL、mounted不淘汰），33项池/票据回归与EOF-only真DOM xterm root迁移/park/复用/LRU/TTL探针；报告新增viewPoolChecked且两DOM结束主pool factory=1。池只通知同步setVisible、backend串行pause/resume由controller负责；finite adapter已用PaneVisibility同步通知当前SurfaceVisibility，实际parking不取消pane/PTY，显式pause→隐藏期间解析HIDDEN_SEED→fenced resume→新Begin后再做native cycle；报告activeParkChecked和pauseCount2均必须满足，headless0。ViewSlotOwner确保React cleanup跟随imperative resume后的最新ticket，过期parked claim不重建factory。未绑定控制面时false仍fail closed；不是生产用户shell/任意长期GUI停车完成。另加finite fit交接6秒scope deadline、phase诊断及取消即时终结，不因缺measurement无限等待或放宽70秒总deadline；native visibility恢复控制面仍跨surface退休保留，不能随scope清理提前解绑restore事件。

`probe-ssh`需系统OpenSSH客户端，仅使用xtask可选ssh-probe feature（russh0.63.3/ring、RSA/压缩/legacy关闭），不把russh接desktop/生产连接。临时生成Ed25519 host/user密钥，server只bind127.0.0.1:0，client显式pinned hostkey，changed key须在认证前拒绝；wrong password/publickey、keyboard-interactive固定echo=false challenge（错误答拒绝/正确答消费/取消连接独立排空，不代表真实MFA UI/硬件）、PTY请求成功回执、window-change91×37、固定UTF8数据回显与exit7/EOF排空、拒绝非固定exec。系统ssh使用-F空配置/独立known_hosts/StrictHostKeyChecking=yes/IdentityAgent=none/无SSH_AUTH_SOCK，执行固定标识而非任意shell，读出精确stdout与exit7后join。server handle与listener正常join、active handlers归零；总25秒、连接/阶段3–5秒，不接受用户endpoint/凭据/命令。输出target/probes/ssh.json，失败先删除旧报告、缺真实字段不能通过；Linux CI注册运行+feature test/clippy不等于远端已执行。当前本机OpenSSH10.0p2 client互通，不证明S4要求的OpenSSH7–9/server矩阵、dropbear/旧设备、MFA/证书/跳板/Agent/FIDO2/GBK通过。

`probe-ssh-system`仅Linux，需要/usr/sbin/sshd、stty和既有/run/sshd。不编辑daemon/global/user SSH配置、不创建/解锁用户；使用当前测试account与既有PAM，临时private config/host key/authorized key/随机127.0.0.1端口启动-D sshd，ForceCommand只允许xtask固定helper，客户端命令和输入从不作为shell source执行。测试daemon的StrictModes=no仅为private temp authorized key路径，tempdir0700/keys0600，不能复制为生产设置或放宽应用pinned hostkey。真实PTY用stty读取80×24→91×37，UTF8字面输入、exit7/EOF排空、disconnect后/proc owned-master children必须清空再kill/join master。输出target/probes/ssh-system.json，10项SSH feature单测/validator门禁；现仅本机OpenSSH10.0p2，7–9/dropbear/硬件/FIDO2等未验收。CI系统安装openssh-server并确保privsep目录，不据注册声称远端运行。

`probe-ssh-jump`复用optional ssh-probe、两个owned127.0.0.1临时russh server；outer和inner各自pinned key，inner错误key拒绝，不能因已信任jump就跳过目标校验。direct-tcpip只允许exact owned loopback target+port、拒绝其它host/port/第二并行channel；1个owned permit贯穿relay worker直到实际结束、6秒worker监督。内层SSH经channel.into_stream完成独立KEX/auth，UTF8+字面shell符号4400bytes精确回显与EOF/Close，outer/inner全部连接和relay在listener shutdown之前归零，最后join两listener。报告ssh-jump.json/validator、13feature tests与CI登记；只限制并发worker，不声称SSH库所有buffer在高压/慢consumer下的RSS已验收。不是生产Route池、多级真实jump hosts、SFTP/Agent/FIDO2或SSH actor集成。

`probe-ssh-certificate`使用临时Ed25519 CA/subject/server，只owned loopback。签名+trusted CA指纹+valid_after≤now<valid_before不足以授权：必须User类型、exact user principal、正确username、critical options为空（当前test policy未知选项全部拒绝）；拒空principals的golden ticket。实际6invalid cert及错username/错leaf私钥全部auth失败，1valid cert固定exec/exit7/EOF/Close与connection-before-stop drain通过。subject持私钥证明由russh协议验签，并单列证据，不能把CA签名当client持钥。输出ssh-certificate.json与validator；15feature tests及CI登记。未实现生产CA存储/撤销/证书UX/设备矩阵，不输入用户certificate、CA secret或Agent/FIDO2。

`probe-ssh-agent`当前Unix，ssh-agent -D/private temp sock启动，明确connect_uds私有路径，不read/connect_env或操作用户SSH_AUTH_SOCK/已有Agent。初始identities空，ssh-add仅向private socket加载1份synthetic Ed25519（30秒TTL、temp key0600/dir0700），raw signature经signature::Verifier验证，authenticate_publickey_with通过Agent Signer而非传SSH私钥。固定exec/exit7/EOF/Close、SSH connection-before-stop排空后只remove_identity该synthetic key，列表空+removed-key签名AgentProtocolError且Agent仍可list确认；最后join owned agent daemon。上游russh0.63.3 remove_all_identities帧header有bug导致OpenSSH Agent EOF，不调用、不patch源码，详dependency-review。另含controlled Agent forwarding：OpenSSH -a 未grant的原channel明确拒fixed-forward exec，-A按该channel授予permission后server channel_open_agent远端查询唯一synthetickey并raw sign/verify，单task permit/3秒监督、fixed stdout/exit7及worker/连接排空；18feature tests和report.forwarding必须齐全。仅OpenSSH-client→russh-server受控路径，不代表Pulse client默认授权/转发UX、FIDO2、Pageant/macOS矩阵通过。

`probe-ssh-legacy`只在独立test连接显式启用DH-group14-SHA1/AES128-CTR/HMAC-SHA1，私有sshd仅offer该组合，host仍Ed25519且pinned。普通client必须typed NoCommonAlgo(Kex)拒绝，不能把任何network/timeout/host-key失败冒充secure-default；显式profile完成原native PTY/尺寸/UTF8/EOF后新普通client再次拒绝，证明没有全局downgrade/retry。模式flag必须与CLI请求匹配，现代/legacy report文件独立，20feature tests+CI登记；不含RSA-SHA1/DSA/group1/3DES或真实旧设备/用户确认UX。

IPC 绑定提交在 `ui/src/ipc/gen/commands.ts`，禁止手改；Biome 排除此目录，tsc 仍检查它。变更命令或 DTO 后运行 `mise run bindings`。生成与实际 invoke/event 注册共用一个命令表，`bindings-check` 已接入三平台 CI；这些任务需对应平台的原生开发依赖，但无需启动显示服务。

输出在 `target/probes/`，详见 [验证记录](./validation.md)。使用未激活 mise 的 shell 时，单条工具命令加 `mise exec --` 前缀。CI 同样读取此配置，不在工作流里重复声明 Rust/Node/pnpm 版本。

### 验收后的构建缓存清理

多轮 dev/test、不同 feature 和原生 GTK/WebView 构建会累积 `target/debug/incremental`、`deps` 与带调试信息的测试二进制。每个验收批次全部结束、日志／截图归档且构建／探针／窗口进程退出后执行：

```bash
mise run clean-dev  # cargo clean --profile dev；保留 target/probes/ 和 release 产物
```

不要无范围执行 `cargo clean`，也不要在后续探针仍需已构建二进制时逐命令清理。清理后下一批构建会重新编译，不重跑已通过的验证仅为重建缓存。磁盘不足同时检查 `df -h` 与 `df -i`；共享 `/tmp` 的 inode 耗尽与本项目磁盘容量是不同问题，不删除其他项目缓存或用户数据。

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
- Windows 私有目录基础能力放在 `pulse-platform`，仅私有 `windows::sys` 允许带 SAFETY 说明的 FFI；`mise run check-platform` 在当前宿主做严格检查和策略测试，Windows 另外注册原生目录测试。交叉编译及其他平台的策略测试不算 Windows ACL 验收，生产存储入口在文件权限／原子替换和实机验证完成前保持关闭。边界见 [Windows 私有目录](../architecture/windows-private-directory.md)。
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

| 已实现工作流 | 触发方式 | 范围与证据 |
|---|---|---|
| [check.yml](../../.github/workflows/check.yml) | PR／push／手动 | 三平台核心与组件、Linux 产品实窗、release 负载；依赖审计和完整许可门禁保留，上传各自探针证据 |
| [windows-product.yml](../../.github/workflows/windows-product.yml) | 相关路径 PR／手动 | MSVC 精确原生用例、PS5.1 采集回归、完整桌面检查与本地工作区构建，上传逐组原始日志 |

接入远程后先运行这两个入口，按具体 job／步骤／收据判断，不以整个 workflow 的存在代替执行。当前审计与完整许可检查预计仍会失败；保留失败，禁止通过忽略步骤或删除依赖范围获得绿色状态。远程首次执行与普通用户 GUI／真实 GPU 验收分别登记。

`interop.yml`、`bench.yml`、`e2e.yml` 和签名／公证／更新的 `release.yml` 尚未实现；它们属于计划目标，不是当前可运行 CI。发布前补齐真实平台、互通、性能、密钥配置及失败恢复，见[下一阶段计划](next-stage-plan.md)。

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

界面外观支持浅色／深色／跟随系统，草稿仅局部预览；用户级保存沿用原页快照与CAS，成功后更新当前界面，不改终端ANSI或获取主题执行权限。其他窗口显式重读或重载后采用新值。开发验证与系统查询失败的行为见[界面外观说明](../architecture/ui-appearance.md)；沿用现有设置schema／前端检查，无新增重型任务。

终端配色提供经典黑底／午夜／纸白预设，在终端设置中局部预览并单字段保存；GUI和CLI下一次新建本地或SSH会话冻结后端基色，SSH覆盖三类身份与跳板叶，已有会话保持原色。经典默认与14px不变，使用现有设置CAS／I/O监督及schema／前端／原生CI入口，单个真实SSH探针核验首条OSC及导航恢复，不新增重型任务。产品实窗配色仍待；见[配色预设说明](../architecture/terminal-color-presets.md)。

## 10. 开源协作

- 仓库根目录提供 `CONTRIBUTING.md`（开发流程、提交规范、如何运行测试）、`CODE_OF_CONDUCT.md`、`SECURITY.md`（通过 GitHub Security Advisories 私下报告漏洞，不在公开 issue 中披露），以及 issue 和 PR 模板。
- 贡献采用 DCO（提交时加 `Signed-off-by`），不要求签 CLA；依据 Apache-2.0 的约定，贡献沿用项目的许可证。
- 用户文档站点（REL-05）和代码放在同一仓库的 `site/` 目录，随版本发布。
