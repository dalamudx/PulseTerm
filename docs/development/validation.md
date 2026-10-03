# 已实现范围与验证记录

本文记录可复现证据，不替代架构决策或验收清单。验证只在当前 Linux 环境执行；已添加三平台 CI，但尚无 CI 执行结果。

## 工程基础

- `.mise.toml` 固定 Rust 1.95.0、Node 22.23.3、pnpm 10.34.6、cargo-deny 0.20.2；任务统一使用 `mise run`。
- workspace 实例化 `pulse-core`、`pulse-ipc`、`pulse-term`、`pulse-pty`、`pulse-app`、`pulseterm-desktop` 与 `xtask`；原生 GUI 依赖受 `desktop` feature 控制，默认仍可做 headless 核心检查。
- `cargo xtask check-deps` 检查已实例化 crate 的直接依赖方向、Tauri 隔离和领域层已知 I/O 依赖；这是初始规则，不是完整的传递依赖/源码安全证明。
- Rust 禁止 unsafe；生产代码禁止 unwrap/expect；TS 开启 strict 和 noUncheckedIndexedAccess，使用 Biome。
- GitHub Actions 配置 Linux/Windows/macOS 检查、测试和探针；VT 样例禁止换行归一化。

## 二进制 IPC 编解码

`pulse-ipc` 编解码与 `ui/src/ipc/frames.ts` 解码共同使用 `tests/fixtures/frames/data.hex`。

- 6 字节头：kind、保留 flags、little-endian u32 sequence；payload 不超过 1 MiB。
- 拒绝短头、未知 kind、非零 flags、超长数据；TS 正确处理非零 byteOffset，payload 引用原 buffer。
- Rust 属性测试覆盖随机载荷往返、任意输入不 panic。新增 sans-I/O `CreditWindow`：同时限制帧数和载荷字节，只接受已发送序号的累计 ACK；重复、过期、未来 ACK 不增加额度，支持 u32 回绕。
- 真实 Raw Channel 与本机 WebSocket 复用此帧格式和信用窗口；尚未实现生产会话重绑定、顺序恢复或桌面载荷分片。

## 终端双解析器探针（S2、ADR-026 的部分证据）

```sh
mise run probe
# target/probes/terminal.json：Rust 观测、VT 重放、应答和内存下限
# target/probes/xterm.json：跨解析器比较结果，探索性失败同样记录
```

- 使用 alacritty_terminal 0.26.0 / vte 0.15.0 和 xterm headless 6.0.0；主探针使用项目 SharedUnicodeProvider，宽度表从锁定 unicode-width 0.2.2（Unicode 17）生成，公开 experimental API 注册，无第三方 fork。
- 通过 7 个样例（包括原探索性 ZWJ 现已提升为严格门禁）：ASCII、颜色/样式、中文与基本 emoji/组合音符、备用屏、返回主屏、光标位置查询。比较内容、宽度、颜色、部分样式、光标位置和备用屏标志。
- 前端策略阻止常见 CSI/DCS/OSC 查询应答及 OSC 52；新增公共 DECRQM（`CSI $ p`）拦截，修复只拦截私有模式而遗漏公共模式的重复应答缺口。单独测试先观测 xterm 原有应答，再安装 hook 验证应答消失，并在释放 hook 后验证模式写入未被吞掉。
- `tests/fixtures/terminal/queries.json` 的 23 组共享用例覆盖 DA1/DA2、DSR、CJK 光标位置、公共/私有 DECRQM、字符尺寸、多查询应答顺序及 OSC 52 读写拒绝。Rust 检查每种固定块大小（1 到输入字节长度），逐块消费应答；前端检查 1/2/3/7/整流分块，必须没有协议应答且屏幕与 Rust 一致。查询矩阵写入 `terminal.json`，跨解析器结果写入 `xterm.json`，由现有 `mise run probe` 和 CI 执行。
- OSC 4/10/11/12 由 backend 使用主题 fallback 与 remote OSC override 应答；在 BEL/ST 边界先收取事件，再继续解析，防止后续 set/reset 改变较早查询的回答。actor 提供有界 palette 与 cell geometry 控制命令。像素尺寸只有实际测量且乘法不溢出才应答；未知时不编造尺寸。这不是完整查询兼容性证明，真实 UI 配色/测量同步仍待接入；矩阵中 OSC 52 无应答仅证明探针的拒绝策略，不证明应用已实现完整的剪贴板权限流程。
- 原先 ZWJ 的格子布局还随 xterm raw Uint8Array 分块改变。每流 `Utf8Stream` 先保留不完整 UTF-8，空解码不调用 xterm；再使用同一 Unicode 17 scalar provider。`zwj.vt` 已改为严格门禁。16 组共享文本以 1/2/3/7/整流分块检查格子、光标和分块稳定性，shared provider 均通过；Unicode6/11/15 与实验 grapheme provider 仅作差异对比，不用于主链路。
- `unicode-table-check` 重新从实际 Rust 表生成内容并只读比较；TS 穷举 0..0x10FFFF 的分类指纹和 Rust 生成值一致。表与适配器用的是**标量宽度**，并非现代 grapheme 宽度：例如家庭 emoji 仍占多个宽格子，没有宣称完整 EGC 字形/字体/IME 支持。实验 grapheme addon 只作为 MIT 开发依赖，不进入产品渲染路径。
- `mise run probe-snapshot` 是完整恢复门禁，并输出双解析器分歧矩阵与恢复体积（`target/probes/snapshot*.json`、`token-matrix*.json`）。已按用户批准增加唯一的非活动网格只读getter，恢复隐藏主屏及备用屏保存光标，保留发布包Apache许可证、VCS来源和SHA-256，211文件来源校验、178项上游测试接入CI。源代码故意漂移探针确认校验失败且不覆盖。三万组release属性测试无失败（延期和近似没有算成精确通过），指纹加入隐藏主屏格子/current/saved和备用屏saved cursor。新增shrink/grow、退出/重入及超链接笔回归。不能无损编码的隐藏光标分离和非行尾pending-wrap返回明确RestoreGap，不再静默丢主屏。xterm矩阵有545个可编码非近似场景，条件M1|M2为299/300，random-85仍记录为分歧；不能声称补丁根治解析差异或S2整体通过。本轮仅有Linux验证，尚未接入生产attach。原有10/62个解析序列分歧仍需同步策略。数据、方案对比与建议见 [完整恢复评估](../architecture/terminal-restore-evaluation.md)。尚未接入生产 attach。
- 非规范 UTF-8/C1 在浏览器的实际行为、组合簇边界的更多控制序列场景、live 字体/IME、历史同步与真实程序录制流仍待验证，总体 S2 未通过。

### 已修复：UTF-8 分块时漏字符

全流解析和分块解析对 `e + U+0301 + 空格 + 🌍` 曾产生不同结果。vte 0.15.0 在补全前一块的部分码点时，可能把紧随其后的 ASCII 字节计入消费长度，却只输出第一个码点。

适配层在进入解析器前保留最多 3 字节的未完整 UTF-8 尾部，下一块补齐；不修改非尾部的非法字节或 C1 控制字节。新增所有固定块大小的状态一致性、组合音符后空格、分块中文和非法字节边界回归测试。

`TerminalProbe::finish_input()` 明确 EOF 策略：一个未完整的尾部 Unicode 标量替换为一个 U+FFFD；重复调用不重复输出，完整输入与未闭合的 CSI/OSC 不被强行补全。6 种不完整标量前缀及幂等性有回归测试。该方法不会恢复/序列化 ANSI 解析器状态，生产 transport 的关闭路径尚未接入。

新增 proptest 混合流检查：ASCII、CJK、emoji、组合音符、SGR、光标移动、备用屏切换和查询按随机长度分块，逐块收取应答后，最终屏幕和应答顺序必须与整流输入一致。范围限于这些合法 UTF-8/控制序列，不等于任意恶意字节流已完整验证。

### 视口重放不是生产快照

`TerminalProbe::screen_replay` 仅用于可见格子重放，不得用于真实 detach/attach；完整恢复使用 `TerminalProbe::restore_sequence`，覆盖范围与缺口见评估文档。

### 重连协议、压缩与最近历史（新增证据）

详见 [事务协议](../architecture/terminal-attachment.md)。已实现二进制 Attachment v1、generation/offset/额度校验、TS有界事务接收和渲染callback ACK监督，禁止Commit前live/部分渲染。共享固定hex fixture检验u64极值与旧generation ACK。

`diagnostic_bootstrap` 在同一actor命令边界捕获历史/屏幕；源状态不变、超限不返回部分结果。最近2000物理行/4MiB cap，软换行cutoff前移至合法逻辑行边界（可能少于请求）；主屏历史在alt激活仍可读取。不支持的宽字符边界明确拒绝，未做deep paging/resize reflow完整验收。

恢复按连续pen压缩和省略默认尾格，并增加不含RIS的screen-only方式保护历史，显式恢复两buffer的tabstop和scroll margin。发现并修复SGR颜色直接写buf绕过Emitter预算的旧漏洞，新增多色预算回归。`probe-snapshot`现在559个可编码场景，条件M1|M2=311/311，无RIS与完整恢复也必须一致（不能据此宣称所有解析序列一致）。纯文本120×40恢复从约62KB降到5625 bytes，最坏159950 bytes未明显下降；性能时间需release报告，debug约8ms不作产品指标。

3个`probe-attachment`真实headless案例通过：183/196/161个片段，primary中文/emoji不完整UTF-8、alt部分CSI、soft-history；检查10/10/8行历史的每格颜色/属性/wrap，2字节live续流无查询泄漏。新增前端取消、慢sink、额度过载和stale generation测试；仍未接入Tauri窗口/PTy。

EchoTransport及screen_replay已限制到test/validation，独立默认构建负向编译门禁通过，防止Cargo workspace feature统一掩盖错误。全部新任务加入三平台CI，仅Linux本地验证，其他runner尚无执行证据。workspace实际57项pulse-term + 12项IPC + 3项PTY + 1项core + 2项xtask测试，TS50项通过；以实际运行数字为准而非历史总数。

### 分歧检测与同边界摘要

372个token/base组合中18个差异（10种token）均被保守风险提示覆盖，漏标记会使CI失败；11个可精确编码的状态已在真实headless消费器中验证screen-only修复，另外7个不可表示/延期不能算修复通过。发现并修复恢复器对“宽head的spacer已擦除”和“origin域外current cursor”的精确性误报，actor拒绝这些帧，未修改解析器。

digest v1按统一UTF-8/u32投影计算，372个跨语言摘要fixture均匹配；checksum相等只证明当前visible投影，非hidden/history/全样式/安全认证。actor只在同revision/parsed_bytes/size且digest相同时清risk，旧boundary/错误digest不清；REP跨分块观察和RIS不得静默清风险都有回归。所有新Rust检查通过；快照严格分类现557，条件M1|M2=311/311（此前559中的2个新增不可表示状态没有伪装精确）。可见窗口的idle/attach/resize限频监督仍未接入生产driver。

### 历史内存预算仍未满足

120 列、40 行视口、保留 10,000 行历史时，按 `size_of::<Cell>()` 计算的格子存储下限为 **28,915,200 字节（约 27.6 MiB）**，不含 extra、分配开销或进程内存。

当前是有上限的热历史，不是冷热分层/压缩归档。10 万行历史目标需要独立压缩存储，不能直接沿用 alacritty 原始格子。此数值也不是整棵进程树的内存测试。

## 真实 WebView 数据通道（S1 的部分证据）

```sh
mise run desktop-check
mise exec -- cargo test -p pulseterm-desktop --features desktop --locked
mise run probe-channel
# target/probes/channel.json
```

- Linux Debian / WebKitGTK 2.54.0 / GTK 3.24.49，Xvfb 1280×800；debug Rust 构建、production Vite 资源。没有关闭 WebKit sandbox。
- 每种传输运行：64 KiB × 256 帧（终端流大小）、256 KiB × 256 帧（桌面块大小）、64 KiB × 32 帧（75 ms 延迟累计 ACK）。Rust 发送确定性载荷，前端逐字节检查数据、长度、序号、总量，ACK 仅在消费后发送。
- 实测完整性通过；最大在途载荷不超过 `window × chunkBytes`，最大帧数不超过 window。取消测试通过，取消后新 Channel 能重新运行。另有信用不足等待、伪 ACK、重复 ACK、独占运行槽与无令牌 Socket 拒绝测试。
- 两次本地完整运行均通过。示例测量：Channel 终端流 21.40 MB/s、桌面块 52.43 MB/s；WebSocket 对应 37.62 / 49.93 MB/s。Channel 送达 p95 为 24.19 / 20.88 ms，WebSocket 为 5.66 / 4.28 ms；数值存在运行间波动，不能作为基准或性能承诺。
- Socket 仅绑定 127.0.0.1 随机端口、单次连接、随机令牌及固定子协议；接收数据消息上限 4 字节，关闭/无 ACK 均有超时或取消保护。probe command 只向本地 main WebView 注册，没有开启远端权限。
- 送达时间通过 7 次时钟往返选择最小误差样本校准；报告含时钟不确定度、ACK RTT p95、JS 16 ms 定时器延迟及在途峰值。定时器响应不等于输入到绘制延迟。
- 合成数据并不经过 terminal parser、Canvas、WebGL 或 RDP 解码，不测 GPU/端到端渲染，也不证明生产缓存/整棵进程树内存有界。三个平台的真实显示环境、系统输入负载、多会话公平性及持续高吞吐仍待验证；S1 尚未整体验收通过。

## 类型、命令与事件生成（S21 的 Linux 控制面证据）

- 精确固定 `specta` / `tauri-specta` 2.0.0-rc.25 与 `specta-typescript` 0.0.12；只启用在桌面适配器，headless 核心没有 Tauri/Specta 依赖。ADR-018 暂保持 Proposed，未自动转为正式采用。
- 最初8个、现12个命令与 `ProbeEcho` 事件共用 `command_bindings()`：同一份注册表生成 `ui/src/ipc/gen/commands.ts` 并注册 invoke/event handler。DTO 包含嵌套对象、枚举、数组和 nullable 字段；前端不再手写重复的配置、统计、测量与报告字段。
- `RawFrame` 不实现 Serialize，仅通过 `IpcResponse` 返回 `InvokeResponseBody::Raw`；反射映射为 `ArrayBuffer`，生成参数为 `Channel<ArrayBuffer>`，不存在 number[] 或 JSON 字节序列的替换。Rust 单元测试和实际 WebView 的逐字节检查均通过。
- 增加带随机 nonce 的生成命令 → Rust 类型化事件 → 生成事件监听器回环。监听先注册，随后发命令，超时/成功都释放监听；最初原生报告版本改为2，后续窗口组合探针升级为3（详见下文），必须有 `typedEvents: true`、取消证据及 7 组测量才可标记成功。Linux 实测回环与全部传输组通过。
- `mise run bindings` 写入绑定；`mise run bindings-check` 比较重新生成的临时内容但不覆盖源文件。手动改变绑定注释的行为探针确认检查失败且源文件哈希不变；随后恢复原文。三平台 CI 已加检查，但尚无远端运行证据。
- **已验证的限制**：rc.25 的递归 `serde_json::Value` 反射会栈溢出，因此未启用 Specta 的 serde_json feature，报告改为显式 DTO；不依赖 generic JSON 反射。`usize` 被精度策略拒绝，控制 DTO 改为有界 u32、内部乘法用 u64 验证，没有放宽 BigInt 禁令。Serde 的浮点输出可能为 null，生成器如实导出 `number | null`；前端仅在有限数运行时检查后用于算术，不用手写接口掩盖 null。
- 新增 `paste 1.0.15` 维护公告来自 Specta 的函数反射，详见 [依赖审查](./dependency-review.md)。没有添加 ignore 或宏补丁。S21 功能链路的 Linux 证据不代表依赖、三平台或正式采用门禁已通过。

## 本地 PTY 原语与真实交互探针（S3 前置基础）

```sh
mise run probe-pty
# target/probes/pty.json
```

- 新增独立 `pulse-pty`：封装 portable-pty 0.9.0，使用显式 executable / literal argv、cwd、环境覆盖与默认 TERM；提供单 reader、完整写入、字符尺寸读写、非阻塞退出观察、等待、关闭输入、终止并回收直接子进程。不依赖 Tauri、解析器或前端。
- 原语是**阻塞接口**，不隐藏队列或线程，不能直接运行在 UI/async worker 上。已新增pulse-app监督native read/write/control到独立actor的有界桥，但窗口订阅、任意阻塞写入取消、进程树/Job Object回收和平台Shell探测尚未实现。Drop 是直接子进程的尽力清理，不是取消任意阻塞 I/O 的保证；Windows 调用方必须在关闭 ConPTY 期间持续排空 reader。
- Linux 真实 PTY helper 检查 stdin/stdout 为 TTY、literal argv/cwd/env 未变、输入逐字节未变；resize 到 91×37 后既检查 master readback，也由子进程 `stty size` 观测。中文与光标查询进入 Rust `TerminalProbe`，应答 `CSI 1;7 R` 原样写回 PTY，并由子进程核验，最后检查 Rust 屏幕及尾部输出、退出码 7 和 EOF。
- 使用固定 `/bin/sh -c` 脚本作系统 Shell 冒烟：确认 TTY、输出 VT 流、Rust 解析结果和正常退出。不是交互/login Shell、OSC 133、vim/tmux 或 Shell 环境探测的兼容性验收。
- 慢消费者暂停读队列，观测 reader 的 send attempt 停止推进，子进程仍未退出；恢复消费后按顺序逐字节验证 **1 MiB** 确定性输出和结束标记，没有丢弃数据。探针限定 2 个排队块，每块 ≤4 KiB；计入消费者当前块、producer 待发送块与固定 reader scratch，分块暂存上界为 **20 KiB**。这是代码结构的缓冲预算，不是 RSS/整个进程树内存测量，排除 OS buffer、线程栈其余部分、握手捕获及解析器状态。
- 检查关闭输入幂等、单 reader、重复 terminate 的退出状态稳定、直接存活子进程 Drop 后 EOF、所有正常 reader join、不存在可执行文件时 spawn 失败。本机直接终止 + 排空约 50 ms，用 2 秒上限检查；不能外推到其他系统或子孙进程。
- helper 独立 10 秒截止时间，worker 外层 25 秒监督，失败不读取上一轮结果；报告要求完整布尔观察、版本、字节/队列预算、精确应答/退出与取消上限，负向单元测试拒绝仅有 completed 标志等不完整报告。
- 已接入 Linux/macOS CI 的 `probe-pty`，三平台CI新增finite Rust helper和native actor桥测试；本轮仅取得Linux结果。**S3/S10 尚未通过**：随附 ConPTY 加载、win32-input-mode、Win10/11、PowerShell/WSL 及真实 UI 兼容性待验证。

## 原生actor监督与三平台PTY探针

- 新建既定应用组装层 `pulse-app`，setup在blocking pool，常驻reader/writer/controller专用线程；`ChannelTransport::bounded_driver`提供固定8块output/input通道，每块上限64KiB（另有native reader scratch/current块、writer在途、command/reply预算与OS缓冲）。无Mutex<Term>，无Tauri和新增第三方依赖。
- native resize由controller确认后才修改parser尺寸，失败保持原state，超过5秒缺ACK明确Failed并cancel；等待ACK时暂停解析（不停止native reader的有界读取）。旧尺寸排队输出仍可能在新size下reflow，这不是跨窗口attach或resize完全顺序性的验收。
- writer阻塞时controller仍可kill/reap直接child；renderer取消后reader不停止，而是discard/drain直到EOF，避免ConPTY master关闭等待无人排空的output。组合receipt join actor/direct child及三worker；Drop receipt请求close，不能保证进程树或OS-hung syscall的退出。input成功仍只是admission，不是physical write receipt。
- 6项native-driver单测验证成功/失败resize、pending ACK取消、正常EOF vs fatal I/O、32次fail/EOF竞态，以及真实5秒deadline。`probe-local-session`在Linux用raw helper证明输入/query回写/child尺寸观察、exit7/EOF、native actorrestore、direct cancel/join和receipt Drop关actor。Windows使用canonical Rust helper，不冒充raw模式/child尺寸query/Win32键盘覆盖。
- `probe-pty-portable`有限Rust helper不依赖stty，验证真实TTY、argv/cwd/env、Unicode canonical输入、native尺寸回读、exit7/EOF和幂等direct termination；helper15秒截止、外层worker40秒监督，report不复用旧成功。本轮Linux通过，已注册三平台CI；Windows/macOS结果待执行，也未引入微软DLL打包决定。

## 独立会话 actor / EchoTransport（S11 前置基础）

- `pulse-term::session` 由一个 Tokio task 独占解析器/网格，外部只有有界命令和小型 watch 元数据；没有 `Mutex<Term>`、Tauri、窗口或 DOM 依赖。`SessionTask` 提供可监督的 join，终态屏幕仅供诊断，不是恢复快照。
- Echo 驱动的输入/输出队列各 8 块，块上限 64 KiB；命令队列 8 条；协议应答积压载荷上限 256 KiB，另有字符串/队列开销。超量消息拒绝；满队列明确返回 Busy（未接受），不伪装写入成功。调用方已构造的消息、无界并发调用数量、内核缓存及网格不在此预算内。
- 后端查询回复单独写入 upstream，不再当作程序输出回送。输出解析每轮有界，选择循环公平处理控制命令；取消在每轮先检查且走带外 watch，不排在数据后面。正常 EOF 完成 UTF-8 尾部；取消/协议溢出与正常 Ended 分开标记。
- 11 个本地测试覆盖无视图输出、观察者释放、最后控制 handle 放弃、字节 roundtrip、尺寸变化、协议应答、输入/输出超限、满队列关闭、持续输出下 inspect/close、公平调度和最终 EOF 替换字符。合成驱动的结束 join 包括 driver 回收；2 秒测试 deadline 不等于 OS I/O 的退出保证。
- 新增 `SessionHandle::diagnostic_restore`：在 actor 命令序列点返回 `SessionState` 边界（size、revision、parsed_bytes）与恢复字节。`parsed_bytes` 仅是解析进度，不是传输 ACK 或 attach epoch；palette/cell pixel 控制变化也递增 revision。
- 默认序列生成和 actor API 的有效载荷上限 1 MiB，actor 请求可进一步收紧预算（大于上限时收敛到上限）。Emitter 的普通追加、格式化写入与字符写入都在生成期间检查预算，待补充 CSI/UTF-8 尾部计入同一预算；超限返回 OutputTooLarge，不返回截断序列。此预算不覆盖 grid、分配器容量/元数据或调用者保留的多个已完成结果；它也不是执行时间上限。
- actor 诊断 API 显式拒绝已知 approximations，单独返回 RestoreGap，不终止会话；可在程序解除条件后重试。满队列、关闭后的请求不接受，已取消的请求若尚未开始会跳过生成。新增 7 个 actor 测试及 2 个 Emitter/精确尾部预算测试，包括备用屏 + 部分 CSI 续流、主屏退出、零/精确/超大预算、revision、已知近似/非行尾 wrap 的解除和重试。
- 已修复 random-85：xterm 的 scroll margins 是每屏状态，Rust 是全局状态；恢复隐藏主屏时也设置 margins，再保存光标/切屏。新增最小命名用例 `primary-scroll-margins-after-alt-exit`。当前 546 个可编码无已知近似样例：M1 381、M2 301、M3 430，条件 M1|M2 **301/301**。TS 断言现在覆盖所有命名/随机 `strict && M2 && !M1`，不再只报告随机恢复失败。负向探针临时省略隐藏屏margins时，命令退出1，同时列出命名回归和random-85；源码已恢复后重验。条件计数不涵盖原流本身已有跨解析器分歧的样例，也不是总体 S2 通过。
- actor 使用 VTE 的真实同步刷新 deadline，在没有新输出时也会主动排空缓冲，经同一 tracked handler 更新状态与 revision；parsed_bytes 保持原始接收/解析进度，不制造额外输出字节。正常 ESU 或超时会解除 SyncPending。deadline遵循VTE，远端重复BSU会延期，未引入独立总时长限制。EOF 也排空未关闭同步刷新和不完整UTF-8；存在不能送达的上游回复则标记Failed，不伪装Ended。
- 批量 flush 内的颜色 query/set/reset/RIS 经 handler 捕获查询当时的override，避免较早查询使用后来的动态颜色；RIS遵循上游保留色覆盖，OSC110才明确重置。6 个回归覆盖 idle超时、deadline前不执行/只执行一次、上游应答、EOF尾部与未应答失败、正常ESU和EOF颜色顺序。
- 扩展30,000组 release 属性测试无失败：日志 exact=18,298、deferred=8,377、approx=2,459、alt-active=4,606（精确续流检查4,018）、alt-reentry=3,967；日志桶不是互斥的全部三万分类，延期/近似不是精确通过。生产 attach、历史/reflow、跨窗口和协议额度仍未验收。
- `pulse-app` 已接入有界真实 PTY async bridge（见 S3/S10 一节），原生 resize 收到成功回执后才更新 parser；IO错误明确 Failed，resize回执超时5秒失败且可带外取消。Echo只在validation构建可用；input成功仍仅代表队列接受，不代表物理写完。传输额度已在pulse-app窗口无关driver贯通；Registry、多视图关联、落后快照策略、生产生命周期事件节流和OS进程树回收仍待实施。
- S11 仍未整体验收：没有实际窗口迁移或 UI 销毁/恢复结果，视口投影禁止充当 attach 快照。此实现只建立与视图无关的基础，不宣称已经实现完整会话恢复。

## 真实终端软件录制及恢复回归

`mise run probe-software` 启动 vim/less/htop/tmux 原生PTY：每个程序记录一个键盘输入和一次80×24→91×37 resize边界，共8个受控样例；Rust严格比較restore前后完整已承诺结构及续流，真实headless xterm比较M1/M2/M3与query抑制。本轮8/8均匹配后端。不是WebView/窗口验证，也不是重新同步频率的代表性统计；风险位可有false positives（样例都有MODE/CELLS/GEOMETRY提示）。原.vt只作来源记录，尺寸转换需要software.json。

- 新发现并修复：alacritty的ED2会把当前主屏推入history，即使主屏空白。旧恢复在vim备用屏重建时制造1行空history，后续扩大窗口会把隐藏current/saved cursor都下移1行；现在恢复用逐行EL，不使用ED2。2项回归验证fresh history为0、grow结构一致、live恢复保持原history每格及wrap。
- 再验三万组release属性无失败：exact=18,602、deferred=8,518、approx=2,048、alt-active=4,538（检查4,395）、alt-reentry=4,073；桶非互斥完整分类。当前Rust/xterm矩阵557个可编码无已知近似样例，M1=391、M2=311、M3=442，条件M1|M2=311/311；基线解析漂移和不可编码案例没有算成功。
- 采样每程序日志≤2MiB、reader仅2块4096字节，父进程40秒deadline；拒绝不能idle或正常quit的程序，不静默跳过。htop只观察采样进程本身；tmux只用独立socket和12秒有限shell，受控kill后client退出1是明确记录的server-exited，其他程序需0。故障保存prefix/suffix/恢复帧与结构，旧成功报告启动前删除。
- Linux CI安装这4个工具并注册探针，只上传JSON，不上传原.vt；测试样例不含用户session内容。原依赖门禁不变。当前仍需原子live订阅/流控/窗口绑定、自动idle同步限频、深历史和reflow UX及Windows实际执行。

## 原子bootstrap/live订阅与Checkpoint

- 新增公开 SessionStream/StreamRecord/StreamStatus 与 diagnostic_subscribe：同actor回合建立history/screen边界和唯一bounded live lease；full先停止解析，再沿transport/native队列回传背压。drop订阅恢复无视图处理，不关闭session。成功替换旧lease，cancel/错误capture保留旧lease；id不是授权/global generation。
- 9项单测覆盖partial UTF-8/备用屏续流、full下控制close/detach、取消/失败capture和替换、pending/native resize、EOF替换字符及sync Fence。控制配置/成功resize结束旧geometry lease，driver须建新generation。
- wire Checkpoint=6携带u64 revision，offset为当前live cursor，同32位sequence和发送额度。Receiver拒绝旧revision、错误cursor/phase/长度；sink处理完成才ACK，缺实现取消。新增4项TS及1项Rust验证精度、callback及phase/geometry/credits。
- probe-local-session真实2MiB输出在Linux填满8条lease后，80ms停读parsed_bytes不前进，inspect仍可用；原子重新capture后旧leaseReplaced，连续offset重放全部边界后流。本轮paused=29,743、最终parsed=2,097,221、live=508块（OS分块会变化），最终marker/Ended/direct child join通过。不是RSS/进程树、网络ACK或窗口验收。
- probe-attachment第四个案例：actor原子lease→binary→TS/headless，在部分UTF-8和备用屏后续流，sync Fence映射Checkpoint，sink严格比较后端屏幕后ACK。check/77项term/13项IPC/57项TS、API隔离及vendor211文件门禁通过；Astrolabe Rust无错误，无依赖变化。
- 已新增 `pulse-app::attachment` 的窗口无关wire/ACK监督：config先验、bootstrap转移所有权、固定frame/byte预算、oldest ACK deadline、正常End与callback契约，取消/失效只释放lease不关闭session。7项测试覆盖零/不合理config保留旧lease、2.5MiB输出无ACK冻结→timeout恢复、EOF截断UTF-8、正常final ACK、End缺ACK不能完成、wrong generation/duplicate ACK、receiver drop、reconfigure与旧binding替换；TS3项覆盖End渲染后ACK/缺finish拒绝/取消无迟到ACK。
- 新probe-local-session wire测量真实2MiB helper→actor→AttachmentSender；1帧、8KiB分片、9000bytes credit，暂停ACK时parsed_bytes冻结且inspect可用，最终2,097,221字节/Ended及child join通过。一轮521帧、credit峰值6,587 bytes、暂停边界24,623（OS分块会变化，不能固定该计数）。ACK来自Rust validator不是前端callback。第五个probe-attachment案例把driver输出回放到headless，End后decoder生成一个替换字符并匹配后端。native压力与headless callback分别实测，不冒充实时WebView/网络往返。
- CI沿用三平台probe-local-session/probe-attachment任务，已包含新路径；本轮Linux执行。77项term、14项IPC、7项app、60项TS及API隔离/check通过。Cargo.lock只增加pulse-app→pulse-ipc内部edge，无第三方版本/许可证/补丁变化。
- 仍非production attach：一个诊断订阅、未覆盖非UTF-8双坐标和任意用户会话。Tauri窗口/命令授权与真实WebView callback ACK→PTY已有下文组合证据；生产旧surface管理、自动idle同步限频和多视图策略仍待实现，不代表三平台/S2/S11整体通过。

## 应用注册层授权与撤销（后端基础）

- 新增 `pulse-app::registry`：32个可信window incarnation、64个session、64个监督task上限及per-session capture try-lock；generation backend单调分配，id耗尽/容量满拒绝且不替换当前binding。元数据短锁不跨await，capture future编译期验证Send。
- 13项新增测试涵盖同actor重复注册/替换watch不能绕过owner、外来window/跨registry token权限、unknown session统一拒绝、旧ACK/detach/晚完成不能影响替换、failed/busy/取消前capture保留旧view、窗口关闭时并发capture不复活、同label重建不继承授权、EOF final ACK清理、应用Drop关闭session、window/session/task容量及ID耗尽。原7项driver测试同样通过。
- probe-local-session的2MiB native wire流现在通过registry注册/input/ACK与监督completion；Linux实际helper连续offset、EOF/child join、暂停ACK时parsed_bytes冻结和inspect通过。测试使用可信in-process WindowToken，不能视为Tauri command授权/WebView窗口事件验收。CI现有三平台任务自动包含此路径，Windows本轮未实测。
- window关闭撤销view但保留application-owned session；新window不会凭相同label继承访问权。跨window迁移/重新授权尚未实现；注册者必须监督native receipt，shutdown不是process-tree join。无第三方依赖、锁文件或parser补丁变更，发布门禁不变。

## Tauri窗口授权与真实PTY/Channel组合（Linux）

- `mise run probe-channel`已升级schema 3：在真实WebKitGTK WebView内连续运行两次native PTY→actor原子lease→registry→Raw Channel→AttachmentConsumer→headless xterm callback→生成ACK命令→backend credit→ordered End链路。四个新增命令由同一Specta表注册/生成；u64控制整数为canonical十进制string，数据始终ArrayBuffer。
- 固定helper接受唯一`go`输入，输出Unicode、颜色和截断UTF-8后正常退出；客户端在一个live ACK延迟250ms，driver最多1帧/8212 envelope/body credit。一次第二轮实际parsedBytes=3,112,985、frames=780、peakCreditBytes=6092、generation=2、exit=0，前后端最终visible digest/输出边界一致。数字为单次测量，不作为性能门槛。
- 创建实际隐藏peer WebView，由runtime关联window token；跨owner input/ACK拒绝。实际Destroyed撤销窗口权限，原native actor仍Running，经显式close/join后回收。相同label连续重建无权限继承；第二轮实际拒绝旧nonce、旧generation及错误generation ACK。origin限制只接受无port/credential的本地tauri host，外部/伪造host拒绝。
- 新增可信registrar `reap_closed`：窗口销毁后只清已结束actor，不可清Running session；70轮关闭/回收单元测试验证window/session容量不累积。失败路径guard关闭peer/helper，有界5秒等待终态回收，child自带40秒硬截止；未知或逾时的join不算完成。
- 组合测试发现EOF后actor已不接受diagnostic_digest命令。新增`LocalSessionTask::join_with_screen`从actor终态receipt取visible projection后一次计算摘要，与driver End边界比较；该screen不是完整snapshot，不改变SessionState或二进制协议。
- native控制器原先优先检查stop，actor EOF与已退出child同时就绪时可能错误标记cancelled；现先try_wait，再决定是否终止仍存活child。确定性回归保留已退出child的零/非零状态，stop仍终止存活child，native I/O失败仍按失败处理。修改后3个独立WebView进程全部通过，各执行两次窗口重建；少量重复运行不代表长期稳定性证明。
- 原7组Channel/WebSocket完整性、慢消费者credit和取消恢复测量仍全部通过。11项native adapter测试、24项pulse-app测试、77项pulse-term测试、14项pulse-ipc测试、60项TS测试、clippy/tsc/Biome与bindings drift通过。Astrolabe用于调用范围，native diagnostics请求超时，不能视为LSP通过。
- 限制：浏览器内使用headless xterm callback，不测DOM绘制、IME、真实用户shell、生产view重建与跨窗口迁移。自动idle/resize重同步、多视图、深历史与Windows/macOS实际组合仍未验收。Cargo.lock只增加现有workspace crate的桌面依赖边，未升第三方版本、改变补丁或放宽审计。

## 绑定摘要比较与有限idle检查（Linux）

- 新增`SessionRegistry::compare_visible`与`RenderedScreen`/`VisibleComparison`：先验证当前window owner和generation，再进行只读actor摘要捕获，返回前重查授权。每session单请求、500ms最小间隔、250ms截止；取消和替换不绕过限频。当前actor状态或捕获边界变化返回Stale；Mismatch和Equal都不清risk。7项Rust回归覆盖一致/差异、不合法输入、foreign/旧generation、输出推进、替换/撤销、取消和single-flight。
- `IdleComparison`以600ms空闲、350ms证据期限、最多3次尝试调度；新输出入队、隐藏、替换或销毁拒绝迟到结果。未完成IPC仍占single-flight，失败不变成分歧。8项TS受控时钟测试验证有限重试、限频、隐藏、Data边界失效和取消。Commit/Checkpoint（及下一节新增Marker）提供revision；普通Data后必须等待新的有序检查点，不能猜测revision。
- 新生成`probe_session_compare`命令使用canonical十进制u64 counter（允许0）和显式小DTO，数据仍Raw Channel；generation必须非0。真实WebKitGTK完整probe连续两轮：错误/旧generation拒绝，人工改变摘要返回Mismatch，紧接重复请求因限频拒绝，600ms后真实headless回调摘要Equal。`channel.json`要求`session.idleChecked=true`，原7组运输与EOF校验同时通过。
- 此阶段仅受控Commit idle验证；后续Data空闲Marker见下一节。仍不自动重绘，同generation/boundary原子恢复授权及生产DOM生命周期待接入；Equal不证明hidden/history/parser一致。未更改第三方依赖、alacritty补丁或审计门禁。

## 普通Data后的有序只读Marker（Linux）

- v1新增Part::Marker=8，与Data/Fence/End使用同一CreditWindow和ordered binary Channel；body是LE u64 revision，offset是当前原始parsed_bytes，未新增旁路事件或JSON终端数据。Marker不同于同步Fence，不写2026l、不finish解码器或更改backend revision，不能打断半条CSI/UTF8。
- actor `diagnostic_marker(lease_id)`只向当前lease的有界队列try-reserve；满队列返回Busy、旧/已detach lease返回StaleLease；处理前取消不注入Marker。active BSU/pending resize明确Busy，不提前flush/resize。`AttachmentHandle::request_marker`/`SessionRegistry::request_marker`贯通，owner/current generation前后校验；请求每session 500ms限频、single-flight和250ms截止，换绑/取消不重置频率。IPC成功仅证明队列admission，不能拿它当边界或ACK。
- `IdleComparison`对新Data入队先作废旧证据，600ms空闲请求Marker（最多3次），等待ordered Marker callback且没有后续Data入队才观察精确边界，再按原调度比较。Consumer必须显式支持只读marker callback且完成后才ACK；不支持、取消、迟到回调均不能释放信用。marker可早于IPC返回，仍不并发发起第二请求。
- 真实WebKitGTK连续两轮native helper输出约3.1MiB后停在半条CSI，前端自动Marker→callback/ACK→摘要Equal，再发送固定m输入续流，最终EOF/边界/digest一致。PTY本身回显m输入也属于原始输出，未过滤或伪造流量。实测第二轮parsedBytes=3,113,003、generation=2、frames=777、peakCreditBytes=8212；schema3要求`markerChecked=true`，数字非性能门槛。peer实际拒绝Marker权限，原7组transport、两次窗口重建和EOF证据同时通过。5个headless attachment场景、真实2MiB native registry/wire探针、native clippy/test与binding drift也通过。
- 此阶段未实现自动重绘和同generation/boundary原子compare→restore；下一节补受控精确修复。生产DOM/user shell、多视图或Windows/macOS实际验收仍未完成。连续BSU、持续新输出或队列满可以延期/拒绝，不承诺总会得到可重绘边界。未修改第三方依赖、vendor补丁或审计门禁。

## 原子精确屏幕修复事务与受控自动重绘（Linux）

- actor `diagnostic_repair`同一回合核对lease、取消信号、全部输出/尺寸revision及digest，Equal/Stale不排帧；Mismatch才尝试精确恢复。gap/approximation、满队列、pending resize或取消明确拒绝，风险标记不清除。`SessionRegistry::request_repair`验证window owner/current generation，single-flight、500ms限频与250ms截止；返回后重新校验generation，换绑不能让旧恢复进入新绑定。
- 新Part9/10/11分段修复与Data共用Channel和CreditWindow：最多1MiB暂存，Commit前无部分渲染，期间不交错Data/End；修复不推进原始parsed_bytes。单份repair permit从actor捕获持有到最终Commit ACK；未ACK时后续Data不会发出，第二次修复明确Busy。receiver/sink不支持、错offset、取消和中断事务fail closed。
- 专用非RIS同步序列CAN→2026h→screen restore→2026l→pending CSI/UTF8，结束同步不能放到待续parser尾部后。sink重置decoder而非terminal，保留scrollback。新增3项core回归含半CSI续流、完整UTF8中、history颜色/cells、旧lease/取消、nonedge wrap拒绝和active BSU拒绝；新增driver/registry两项授权/晚完成/ACK限流回归，1项IPC大整数/NoCredit/section offset回归，3项TS Commit/ACK/unsupported/取消/中断回归。
- 真实WebKitGTK两轮受控自动修复：fixed helper在半条CSI暂停，Marker callback中故意修改xterm可见内容及cursor；idle检测真实Mismatch，重新捕获同边界精确Repair，Commit后再次摘要Equal再续流。修复前后history文本完全相同，最终EOF/native exit0/digest及原始字节边界一致，peer修复权限拒绝；schema3要求`repairChecked=true`。实测第二轮parsedBytes=3,113,003、frames=783、generation=2、peakCreditBytes=8212；原7组运输检查同时通过，数字不作性能门槛。
- `mise run probe-snapshot`默认600个随机样例及既有完整续流回归通过；条件xterm矩阵311/311（只包含原输入解析能与Rust一致的案例），分歧矩阵仍18/372，11个可精确修复、7个明确不可编码/延期且全部风险标记。不是重跑30,000样例或完整S2验收。5个binary/headless场景及2MiB真实native授权/ACK/backpressure探针也通过。
- 自动修复在此probe每绑定最多一次，不能因仍Mismatch循环重绘；持续输出/Stale/近似状态的生产重试UX与visible-view全局调度仍待接入。当前只验证真实WebView里的headless callback，不是DOM绘制/IME/selection。未宣称S2/S11、history全部恢复或Windows/macOS验收；vendor/依赖锁和发布门禁未变。

## 可见DOM终端组合验证（Linux）

- `probe-channel` schema4保留两轮headless基线，新增两轮可见`@xterm/xterm` 6.0.0（MIT）DOM renderer；固定PTY 40×12、128行history。同一session probe/consumer走窗口owner/generation授权、Raw Channel、250ms延迟ACK、空闲Marker、半CSI显示损坏→精确Repair→摘要Equal→续流/EOF。每绑定只修复一次，不开放任意shell、键盘/粘贴输入或外部链接。React只注册host，输出及decoder留在imperative controller。
- DOM sink先等待xterm write callback，再等两次animation机会（1秒有限截止）；隐藏、卸载或取消不绕过等待、不返ACK。最终DOM renderer `.xterm-rows`的12行文本与同viewport buffer逐行相同，屏幕位于可见viewport、几何非零、onRender事件及frame fence非零才算完成；不是GPU/compositor回执，也不是input-to-paint时延。screenReaderMode开启只说明选项接入，未测真实读屏器。
- 严格CSP阻止xterm动态stylesheet曾造成文字不可读；已用本地HTML空style anchor的Tauri每页nonce与公开documentOverride隔离样式创建，确保nonce先于CSS文本/插入。全局document不改、只授nonce给renderer style；缺nonce或未替换token明确拒绝。不加unsafe-inline、不改tauri.conf.json、不引入xterm fork。原生窗口截图已读回确认文字/中文/emoji可见及布局不溢出；开发界面沿用现有Radix tokens，机械检测零发现。
- 初轮再次出现真实EOF早于child.try_wait收据被标为native_cancelled。reader只在成功read0记录EOF；controller只在真实EOF且无I/O failure时提供单次100ms退出发布宽限，截止不续期（正常20ms监督采样），非EOF stop及I/O error仍立即terminate，过期继续终止。不把cancelled+exit0当成功。新增回归覆盖99/100ms界限、非EOF/失败/未stop、重复调用不延长期限；真实2MiB PTY→registry/ACK压力、resize、EOF及取消仍通过。
- 两次成功的独立Linux运行各含2轮headless+2轮DOM及原7组transport/cancel测量，最后`generation=4`、`parsedBytes=3,113,003`、`frames=780`、`peakCreditBytes=8212`、digest=`b17f4532b0d32770`；DOM `renderEvents=776`、`frameFences=776`、`rowsChecked=12`、320×192 CSS px。时间/帧数是单次证据，不是吞吐或延迟门槛。此前一次报告因错误地把固定样例行数写为8而被拒绝；已改为真实12并保持逐行全量验证。前端70秒会话deadline保留首个renderer错误，后续取消不能覆盖根因；后端仍有既有60秒relay/10秒ACK/helper40秒限制。
- 完整check/test、native clippy -D warnings、12项native测试及bindings-check通过；85项TS含9项新增fence/nonce回归。`pnpm audit`无已知漏洞；lock仅新增@xterm/xterm 6.0.0单包。未更新Cargo.lock/vendor/CSP策略或依赖豁免。生产DOM交互/用户shell、ResizeObserver/fit、IME/selection、全局resync/隐藏暂停UX以及Windows/macOS仍未验收；不宣称S2/S11通过。

## 有界generation输入与DOM键盘回写（Linux）

- 新增`SessionHandle::input_for_lease`、`AttachmentHandle::input`与`SessionRegistry::input_bound`：runtime owner/current generation检查后，actor处理回合核对当前lease、取消watch、subscriber仍在及reply未abandon。pending resize/replies或满transport队列拒绝，旧lease不能把晚键盘输入发送到新view。返回前再次核对generation；准入后的input无法撤回，成功不是native write收据。常规后端`input`仍用于非viewer可信控制（固定go/resume等），未错误声称它已经generation-bound。
- `InputQueue`按UTF8实际字节计费：单条≤1024B、活动+排队≤8192B/32项、一个IPC。空/孤立surrogate/单条超限不调用native；overload/取消/错误作废所有pending，一秒截止视为uncertain，不能自动retry或在late success后继续drain。取消/错误向后端请求generation-scoped detach，旧cleanup不能撤销新绑定；detach本身不关闭会话，开发finite helper仍由自身supervisor负责资源回收。
- 固定helper在半CSI修复后显示INPUT_READY，等待精确`typed中文🌍`一行再显示INPUT_OK、EOF及截断UTF8尾部。有限`probe_session_input`只允许该固定字符集与CR、≤1024 UTF8 bytes，并拒绝LF/ESC/其他字符；不是任意shell/input端点。peer窗口跨owner、wrong/old generation及非法LF均在真实WebView拒绝。
- 两轮headless通过同队列但不宣称键盘；两轮可见DOM临时启用disableStdin=false，textarea合成keydown/keypress/keyup输入ASCII `typed`、composition提交`中文🌍`及Enter，经xterm公共onData→InputQueue→typed invoke→actor→real PTY。onData序列必须精确为`typed中文🌍\r`（漏字/重复/查询泄漏失败），helper回显INPUT_OK且最终digest/EOF相同。DOM最终仍只读；capture paste listener须阻止合成paste且不产生onData。screenReaderMode保持开启，但合成composition不是OS IME输入/候选框/无障碍读屏器验收。
- schema4新增session.inputChecked及dom.keyboardChecked/compositionChecked/pasteBlocked门禁。两次独立完整Linux运行成功（每次2headless+2DOM+原7运输/取消场景）；最后generation=4、parsedBytes=3,113,061、frames=789、peakCreditBytes=8212、digest=`2ba9204154c3b86c`，DOM 12行文本一致、320×192、renderEvents=787/frameFences=785。数值为证据，不作性能承诺。
- 2项actor回归、1项registry回归、7项TS队列回归及1项native输入validator回归新增；全workspace check/test、native clippy -D warnings、13项native tests、bindings-check、2MiB真实授权/ACK压力通过。没有升级依赖、变更锁文件/CSP/vendor或豁免门禁。生产输入快捷键/广播/粘贴确认/fit/ResizeObserver、真实OS键盘与IME、Windows/macOS仍待验收。

## 自动fit、原生resize回执与新generation恢复（Linux）

- `resize_for_lease`/AttachmentHandle.resize/SessionRegistry.resize_bound在owner+current generation入口及actor回合验证lease/取消watch/调用reply；same-size不发native、不推进revision、不撤销lease，pending resize明确Busy。native ACK成功才更新grid并发Reconfigured；失败保旧grid/lease，既有5秒未ACK失败关闭会话语义保持。准入之后native不可回滚；不能因不确定结果自动retry。resize成功必终止旧binding，registry返回时只重检owner而不误要求旧generation仍存在。
- `measuredGrid`按实际DOM cell大小与host content box去padding计算整数grid：hidden/0/负/nonfinite/小于2×2不发native，最多160×60。FitScheduler 50ms合并observer风暴，单IPC、6秒不确定截止（native最长5秒），错误/取消不optimistic改renderer、不重试、不允许late结果重绑。9项TS回归覆盖最后观测、等待ACK、无效直接geometry、no-op、hidden、取消和deadline；2项core native-ACK/取消/abandon/失败回归及1项registry跨owner/旧控制/同session新generation回归通过。
- fixed helper在go前输出已解析`RESIZE_SEED 中文🌍`。两轮DOM初始Commit触发真实ResizeObserver适配40×12→104×12；native ACK后relay只对一次Reconfigured重新capture，同session的新Begin使用新consumer/decoder/DOM renderer，按输出顺序恢复seed和精确尺寸，再执行原query/Marker/Repair/keyboard/EOF门禁。禁止旧ACK/input/resize/detach，新receipt回调校验自身generation。headless两轮无resize；report schema4要求DOM fitChecked及resizeCount=1，并与NativeExit真实resize_count对照，最终columns/rows从actor终态提供，不只相信frontend标志。
- 综合检查意外发现已有capture发布竞态：actor先oneshot.send新stream，旧lease.status稍后才Replaced，多线程下caller可在间隙醒来。修复增加私有InstalledStream commit oneshot，public diagnostic_subscribe必须等待actor撤销旧lease/安装新lease后才返回。捕获前取消保旧lease；send之后/安装后的取消仍已换绑，不承诺rollback。新增确定性回归人为延迟commit10ms确认API仍pending；原2MiB native冻结/重新捕获的Replaced断言保持不变并通过，不放宽门禁。
- 三次独立完整Linux WebView运行成功（最后一次包含commit gate，每次2headless+2DOM+7运输/取消场景）；最后DOM generation=6、104×12、832×192 CSS px、parsedBytes=3,113,086、frames=783、peakCreditBytes=8212、digest=`00b088cbb609382c`、renderEvents=780/frameFences=779，12行DOM/buffer逐行一致。截图已读回确认auto-fit可见、不溢出；数字为单次证据，不承诺性能。完整check/test、89 core/38 app/16 IPC/13 native/101 TS、bindings-check、native clippy -D warnings及真实2MiB native probe通过。
- 这里只验一次初始列宽适配（容器高度当前由固定12行决定），不宣称连续拖动/ResizeObserver持续换绑、独立布局高度改变、跨屏DPI/zoom或生产TermView/IME完成；正式策略应新generation重新测量、合并最新geometry，并保留精确恢复拒绝/延期UX。没有新增依赖、改锁文件/CSP/vendor或放宽发布门禁。

## 多阶段observer缩放、容器隐藏恢复与迟到view隔离（Linux）

- fixed DOM每新generation重装ResizeObserver/FitScheduler，容器独立208px高度不再从旧terminal行数倒推。初始40×12→104×12；连续写入3个width/height样例（464×192、480×184、496×176）由observer/50ms合并后只提交一次60×10；恢复自然宽+208px后第三次提交104×12。native ACK后精确capture、旧surface销毁与decoder重建，seed/尺寸必须相同；finite relay cap3，native完成的resize_count须等于重捕获次数，普通错误不得重试。
- 新增显式owner/nonce/generation授权的pause/resume控制：VisibilityGate先在短元数据锁内完成registry.detach，再发布pause票据；driver Detached只能对同一票据、至多一次、5秒内显式resume重新capture。暂停不关闭registry session，Rust继续处理输出；无票据Detached、旧/重复resume、wrong generation或owner撤销不能授权新view。watch先注册后查票据，早到resume不会丢失；3项native回归覆盖未pause/准入失败、generation/单次消费及显式等待。没有持有terminal grid锁或引入轮询native控制。
- 每轮DOM第三次resize后显式pause、隐藏host、销毁旧consumer/renderer；固定helper接受trusted内部hidden控制并输出HIDDEN_SEED，后台parsed_bytes继续增加且Lifecycle=Running。100ms期间Raw Channel帧数不增加、旧input/ACK拒绝；重新显示host并显式resume，以新generation恢复RESIZE_SEED/HIDDEN_SEED及对应parsed boundary，然后继续原query/Marker/Repair/键盘/EOF验证。隐藏恢复是受控DOM容器，不是OS窗口minimize/hide或真实document visibility事件。当前hidden-output证明请求最多等2秒，不开放用户命令。
- `SurfaceGeneration`要求requested replacement后才接受更大Begin，保留U64精度；已撤销generation帧不触发idle活动或ACK。AttachmentConsumer旧Promise完成/错误按consumer identity过滤，late callback错误也不取消新surface。共享attachmentEnvelope负责bounded length/version/flags/zero generation，不复制codec；8项TS回归覆盖无Begin/重复/未请求替换、连续resize/resume、stale frames、late success/error及64位边界。
- 两次独立完整Linux运行成功（最后含新router/late callback guard，每次2headless+2DOM+原7transport/cancel）；DOM最后generation=12、3次native resize、1次pause/resume、104×12、parsedBytes=3,113,119、frames=794、peakCreditBytes=4115、digest=`00b088cbb609382c`，DOM 12行buffer投影一致、832×192、renderEvents=791/frameFences=790。report schema4要求visibilityChecked、resizeCount=3/pauseCount=1；headless计数均0。数值只是单次证据，未承诺性能。
- 完整check/test、89 core/38 app/16 IPC/16 native/109 TS、native clippy -D warnings、bindings drift、原2MiB native压力及5个跨语言binary/headless场景通过，机械UI检测零发现。没有新增依赖/锁文件/vendor/CSP或ignore/许可放宽。尚未验收真实OS窗口拖动/minimize、跨屏DPI/zoom、长期隐藏/恢复失败UX、生产ViewHost/用户shell与Windows/macOS；精确恢复拒绝/延期仍是硬门禁。

## 真实Tauri native hide/show与Destroyed暂停撤销（Linux）

- 新增finite `probe_session_window_cycle`控制，仅owner/nonce/当前paused generation可单次调用；wrong/duplicate cycle拒绝。Rust先回读window可见且非minimized，再hide、有限2秒内回读is_visible=false；hidden_output在native隐藏期间注入固定helper控制、parsed_bytes推进且session仍Running，后台等100ms后show、有限2秒内回读is_visible=true且is_minimized=false，然后标记native收据。窗口恢复由Rust监督，不依赖隐藏期间JS/rAF/document.visibilityState的执行。resume在native_inflight中拒绝，show回读完成后才允许新generation捕获。
- NativeVisibilityGuard在错误/取消路径仅对仍匹配caller WindowToken的同runtime窗口尝试show；不会重建已销毁窗口、自动resume或关闭PTY。native操作有不确定副作用，不能盲重试。finite gate仍最多一次pause、5秒监督，失败会由probe guard最终清理样例，非长期生产隐藏策略。报告schema4新增nativeWindowChecked，由后台gate而非frontend填写，DOM必须true、headless false；checked=true与pauseCount=0冲突被拒绝，generated命令/DTO与bindings drift已注册。
- Destroyed回调撤销窗口时立即关闭当前owner gate、清除票据并watch通知；等待者不再要等5秒。closed拒绝所有后续pause/resume/native finish；CloseRequested可被应用阻止、Focused(false)只是失焦，不做同等处理。锁poison也要发布closed、唤醒watch并撤销registry owner。5项新增native回归覆盖已poll waiter、关闭早于wait/晚于early resume、native在途destroy、show收据前禁止resume及metadata panic；报告fixture增加计数/布尔一致性回归。
- 一次完整Linux WebView运行成功，内含2headless+2DOM及7transport/cancel。每DOM有3native resize、1pause/native hide-show/resume，最终generation=12、104×12、parsedBytes=3,113,119、frames=786、peakCreditBytes=4115、nativeWindowChecked=true、digest=`00b088cbb609382c`；12行DOM投影一致，832×192px、renderEvents=783/frameFences=782。hidden期间Channel无新帧，重绑后包含HIDDEN_SEED，并继续输入、repair、EOF。完整check/test、21native/109TS、native clippy -D warnings、bindings-check及UI机械检测通过；最后仅增强close-poison/报告冲突负向门禁，不重复已通过的native成功链。
- 环境Xvfb没有WM（openbox/xfwm4/fluxbox均不可用），因此未声称最小化通过；native.hide/show不是minimize，也不是实际用户触发的document.visibility事件。生产自动可见性adapter、OS最小化/恢复、长时间隐藏失败UX、跨屏DPI和Windows/macOS仍待实测。未增加依赖、修改锁文件/CSP/vendor或放宽release gate。

## 自动容器/页面可见性合并与单generation恢复（Linux）

- 新增可复用SurfaceVisibility（不绑定React）与bindSurfaceVisibility；每generation一个控制器，container/document两来源AND；页面初测/事件测量原子提交两值，不能在容器show、document仍hidden之间制造短暂resume。focus不是来源。host.hidden变更由真实MutationObserver驱动，visibilitychange读取真实document.visibilityState，不篡改浏览器状态；任意ancestor CSS遮挡/DOM解绑并非自动覆盖，解绑必须由ViewHost AbortSignal退出。
- 收到不可见通知同步取消input/idle/consumer/renderer，再异步pause IPC；即使show提前到达，也等detach成功收据再resume。phase只允许visible→pausing→paused→resuming→handoff；每个控制IPC1.5秒不确定截止，错误/timeout/取消不重试，已准入副作用不可回滚。resume期间又隐藏会fail closed，不拿旧generation发送第二次pause。恢复callback在IPC前只授权一次新Begin，准确恢复门禁不变，controller在handoff后忽略旧来源/结果，新generation负责之后生命周期。
- fixed DOM探针移除直接pause/resume调用：绑定controller，设置实际host.hidden=true，等待controller.paused；native backend执行窗口hide/隐藏期输出/show后，host.hidden=false让AND测量自动resume。observer在handoff/失败/abort时断开并移除visibilitychange监听；React宿主传入的dom.signal通过lifetime.abort立即取消控制器、renderer及generation detach，而不是等observer猜测卸载。
- 17项新增TS回归覆盖同步retire/可见no-op、AND、early show必须等pause、来源风暴single-flight、handoff旧通知无效、调用前取消无IPC、在途取消/late结果不动新view、pause/resume错误和1.5秒timeout不重试、resume在途hide、retire失败、already-aborted、真实事件订阅接口与监听清理，以及原子snapshot切换。126项TS、全项目check/test（89core/38app/16IPC等原门禁）与tsc/Biome/Vite通过。本轮不改Rust/DTO/依赖/CSP/vendor，原21native测试证据仍来自前一轮。
- 两次独立完整Linux WebView通过（最后含原子snapshot和自动监听清理，各2headless+2DOM+7transport/cancel），最终DOM generation=12、3resize/1pause/nativeWindowChecked=true、104×12、parsedBytes=3,113,119、frames=787、peakCreditBytes=8212、digest=`00b088cbb609382c`、renderEvents=785/frameFences=783、12行DOM匹配。期间无旧Channel帧，恢复seed/输入回显/repair/EOF均通过。native shutdown出现GLib-GIO name释放连接已关闭warning，不影响完整report/退出收据，未把它隐藏或作为失败重跑。
- 浏览器的visibilitychange来源合并在单元模拟源中验证；真实WebView证据是host属性observer自动pause/resume和native hide/show，不声称浏览器必在OS最小化时发事件。系统minimize、应用多pane ViewHost、跨屏DPI/IME/Windows/macOS及长期hidden策略仍未完成；release gate未放宽。

## Linux X11/Openbox真实native最小化与恢复（受控样例）

- 新增`mise run probe-window`→cargo xtask probe-window→新建私有Xvfb/DBus/Openbox（即使已有DISPLAY也隔离），CLI --probe-window选择backend模式，JS不能选择minimize。普通probe-channel保持原无WM路径与报告channel.json；最小化单独生成window-minimize.json/WM log。Openbox/xprop属于验证环境系统前置，不引入Cargo/pnpm或分发依赖；Linux CI安装并新增独立执行步骤/证据上传，注册CI不当作远端已运行证明。
- native cycle准入/owner/generation/pause票据不变：minimize→最长2秒确认runtime is_minimized=true→后台产生固定HIDDEN_SEED、parsed_bytes推进且Running→100ms→unminimize/show→最长2秒确认visible&&!minimized→finish native receipt。controller之后AND确认容器/页面可见、授权新Begin并精确恢复；不是CSS隐藏或失焦替代。guard失败/取消仅对同WindowToken窗口尝试unminimize/show，不自动resume/新建窗口/盲重试。
- 新增后台DTO nativeMinimizeChecked，必须与nativeWindowChecked/pauseCount一致；server与xtask各自核对mode：minimize DOM=true、headless=false，普通channel二者false。2项native测试覆盖mode错配/缺失/headless claim与全部visible/minimized布尔组合；SessionEvidence fixture拒绝不具备window收据的minimize true。xtask新增2项验证缺失/字符串/数字不是bool收据，现23native/4xtask/126TS通过；bindings generation/drift、native clippy -D warnings及完整check/test通过。
- wrapper等待EWMH支持窗口属性最多50×100ms，owned WM失败立即退出、未ready明确deadline失败；EXIT/INT/TERM清理自己启动的Openbox，外层180秒timeout覆盖完整进程组。直接负向probe用stub openbox退出17确认wrapper失败1而非fallback；用stub xprop使ready永远失败确认5秒timeout；真WM下app退出7保留7；三种退出后pgrep无遗留Openbox。script sh -n及git diff --check通过。
- 一次独立带WM完整运行成功（内含2headless+2DOM+7transport/cancel）；nativeMinimizeChecked=true、generation=12、3resize/1pause、104×12、parsedBytes=3,113,119、frames=792、peakCreditBytes=8212、nativeExit=0、digest=`00b088cbb609382c`；DOM12行匹配、832×192、renderEvents=790/frameFences=788。最小化期间无旧Channel帧，恢复HIDDEN_SEED，后续输入/repair/EOF全通过。最后普通无WM Channel另一次完整运行也通过，其nativeMinimizeChecked=false、headless=false，后续check/test通过；没有重复已通过的WM正向样例。
- Openbox启动log有系统菜单文件缺失message，minimize运行退出时有X授权释放warning；完整runtime状态回读/report/exit均成功且WM无泄漏，没有放宽WebKit sandbox或忽略门禁。数字只是单次受控证据，不承诺性能。尚未验收用户点击最小化的自动事件捕获、其它X11 WM/Wayland、Windows/macOS、长时间最小化/关闭竞态实际UI、跨屏DPI与生产ViewHost；所有dependency/release gate保持。

## 原生事件提示采样、typed可见性信号与三来源合并（Linux）

- 新增NativeWatcher，在runtime install时创建每窗口独立UUID incarnation与ledger；Focused/Resized/ScaleFactorChanged只作hint，永不按Focused(false)直接判hidden。watch mailbox合并hint，初测及每批50/50/100ms有限查询visible/minimized；无idle持续poll。runtime getters在spawn_blocking，不等待GUI阻塞Tokio core；单window一份owned permit留在实际job中，500mstimeout只结束观测、job未结束不能释放信用，busy/error/invalid发invalid并关闭observer。状态不变不推进revision、不重发相同revision；Destroyed/shutdown关闭worker，late commit失败。
- 注册typed定向NativeVisibilitySample与owner/nonce授权的native snapshot命令；采样后重验WindowToken，不把incarnation当权限凭据。前端先targeted typed listen再snapshot，使用NativeVisibilityFilter只接当前incarnation且更大u32 revision；validate nonempty/bounded identity、integer revision与bool字段。bootstrap最多4份identity、每份只保最高revision、clone对象，foreign或乱序不能覆盖当前；overflow明确fail closed。snapshot后清空buffer，invalid样例取消当前view、不fabricate visibility。
- SurfaceVisibility新增native来源，原子DOM snapshot只更新container/document并保留native值；native.valid&&visible&&!minimized为第三份AND。unit证明native false单独同步retire/pause、native restore不能绕过document hidden。实际finite probe订阅真实event并要求hidden→restored均收到，2秒限期，report schema4新增nativeSignalChecked由DOM阶段验证；命令返回本身不算事件证据。
- 7项native回归覆盖hint不修改projection、重复状态/真实minimized转换、closed拒绝late结果、invalid不是visible/minimize凭据、每window独立identity、u32溢出拒绝、1000 hints合并与close唤醒、真实blocking read timeout后permit仍为0且下一read不执行直到释放。9项TS回归覆盖old/duplicate/reordered过滤、invalid/bounds、三来源AND、bootstrap乱序foreign/容量/clone。完整check/test、30native/135TS、4xtask、89core/38app/16IPC、native clippy -D warnings、bindings drift及UI机械检测通过。Astrolabe依赖索引标记stale，只用于参考，未作为LSP诊断通过。
- WM模式与无WM默认模式各一次完整Linux运行成功（各2headless+2DOM+7transport/cancel），两份report nativeSignalChecked=true。WM nativeMinimizeChecked=true、generation=12、104×12、frames=788、peakCreditBytes=4115、renderEvents=785/frameFences=784；默认nativeMinimizeChecked=false、frames=792、peakCreditBytes=8212、renderEvents=790/frameFences=788；二者parsedBytes=3,113,119/nativeExit=0、digest=`00b088cbb609382c`、12行DOM匹配。最后仅强化超时job信用与bootstrap乱序负向分支，确定性新增测试/完整门禁通过，未重复已通过正常GUI路径；单次数值不承诺性能。
- 实际信号来自Tauri窗口事件hint后的runtime采样，native cycle命令没有伪造observer hint或native event；但实际probe仍先host隐藏pause，然后minimize/hide，因此只证明采样/定向事件/AND集成，不是用户点击最小化作为唯一来源独立驱动pause验收。通用WindowEvent没有可靠desktop minimize专用事件，超过有限settling窗口或无hint的WM变化可能漏判；其它WM/Wayland/Windows/macOS、production多pane ViewHost、长时间hidden策略/跨屏DPI/IME仍未完成。未新增依赖/改锁文件/CSP/vendor/release忽略。

## 原生信号作为唯一来源独立暂停/恢复（Linux）

- 新增finite probe_session_native_cycle并复用原来GUI恢复guard/状态回读，共用run_native_cycle；新入口先runtime owner/nonce/current binding只读准入、external generation单次预留，然后只调用hide/minimize，绝不主动detach或伪造native hint/event。最长2秒等待frontend controller真正pause票据（registry.detach成功）后，才让helper产生HIDDEN_SEED并继续Rust parsed_bytes；100ms后Rust show/unminimize并严格读回可见、finish native收据。
- finite gate.external预留本身不授权resume/hidden或排pause；pause仅对应generation并成功revoke才设置native_started/inflight、watch发布；old/duplicate/zero reserve失败。native finish前resume拒绝，Destroyed可即时唤醒尚未pause的等待者，early frontend pause先于wait注册不会丢失。4新native回归覆盖预留无pause/错误generation/单次、关闭未pause action、失败revoke/早到pause及已paused不得reserve。
- public SessionRegistry.check_binding(owner,id,generation)是元数据只读准入，不ACK、不输入、不resize、不改变信用，也不保证asynchronous后来仍current；真正pause仍经原owner/generation门禁。新增1app回归覆盖未attach、foreign owner、换绑old gen、detach/close拒绝，controller/SessionTask仍Running直到explicit shutdown，driver输入为空。
- DOM native-only阶段移除host.hidden=true/false与bindSurfaceVisibility（document/container来源保持true），不直接pause/resume。typed native false通知里记录nativeHidden，controller retire要求nativeHidden且host.hidden=false，cancel旧ACK/input/render然后自身pause；native restored只触发controller resume，resume callback先等待同次native动作Promise完成并核对hiddenBoundary推进/旧Channel帧数不变，再期待new Begin及resume。这样visible event早于finish_native不会造成过早IPC错误，scope内仍1.5秒不确定截止、无重试。
- report schema4新增后台nativeOnlyChecked与DOM nativePauseChecked，分别要求external.reserve+native.finish真实收据和native作为唯一source退役；两类探针DOM必须true、headless=false，原nativeSignalChecked保留。controller数据不进React state，数据仍单ordered binary channel。wrong future generation native cycle与duplicate调用被真实WebView拒绝。
- WM最小化与无WM hide/show各一次完整Linux运行通过（每次2headless+2DOM+7transport/cancel）。WM nativeMinimizeChecked=true/nativeOnlyChecked=true/nativePauseChecked=true，generation=12、3resize/1pause、104×12、parsedBytes=3,113,119、frames=792、peakCreditBytes=4115、renderEvents=790/frameFences=788；默认minimize=false而另外两项true、frames=786/peak4115/render783/fences782。二者nativeExit=0、digest=`00b088cbb609382c`、12行DOM匹配，隐藏期HIDDEN_SEED在new generation恢复，后续keyboard/repair/EOF仍全通过。
- 完整check/test、34native/39app/135TS、89core/16IPC/4xtask、native clippy -D warnings、bindings drift/真实新command DTO注册与UI机械检测通过；Openbox无泄漏，锁文件/CSP/vendor/依赖门禁不变。实际minimize/hide由runtime测试命令而非人工标题栏点击，所以证明native信号可独立驱动整条pause/resume，但不证明真实human事件、所有WM/Wayland/Windows/macOS、长期hidden/production ViewHost或跨屏DPI/IME验收；无hint/延迟超过采样窗的风险仍存在。

## 外部WM动作独立触发最小化/恢复（Linux X11）

- 新增mise run probe-window-external/xtask/CLI --probe-window-external与独立X11 controller脚本，复用新Xvfb/Openbox而非用户桌面。模式下应用正常cycle不调用window.minimize/unminimize/show，只在wrapper提供的umask077私有临时dir atomic rename固定动作:decimal generation marker。controller是独立进程，按自己直系app子进程PID寻找唯一可见XID、核对getwindowpid，执行timeout2s xdotool windowminimize/windowactivate --sync；不是CSS隐藏、失焦替代或由应用自身GUI API完成动作。
- 原runtime readback、native event hinted sampler、native-only controller pause/hidden output/恢复收据/精确新Begin链路保留。新增后台externalWmChecked且必须nativeOnlyChecked+nativeMinimizeChecked；server与xtask核对CLI mode/DOM true/headlessfalse，普通probe-channel/probe-window=false。独立window-external.json不覆盖另外两模式；controller动作日志window-external-actions.log纳入CI上传与xtask门禁：≤128B、恰好4行、两轮minimize→restore同gen、generation正整数规范/不同递增，拒绝缺失/错配/重复/倒序/非法文本。2项新增xtask单元回归验证这些规则，现6项通过；native fixture拒绝无nativeOnly收据却external=true。
- 一次完整外部WM实测成功（2headless+2DOM+7transport/cancel），动作日志minimize:6/restore:6/minimize:11/restore:11；DOM externalWmChecked/nativeOnly/nativeMinimize均true、generation=12、3resize/1pause、104×12、parsedBytes=3,113,119、frames=794/peakCreditBytes=8212、nativeExit=0、digest=`00b088cbb609382c`、12行DOM一致、832×192px、renderEvents=791/frameFences=790。controller/WM pgrep无遗留；xdotool显示两条_NET_ACTIVE_WINDOW初始属性警告，实际动作/状态回读/退出完整成功，未隐藏警告或放宽门禁。新增日志gate对应实际4条收据与确定性测试，不重复已通过外部WM正向运行。
- 4项直接shell negative probes确认非法generation/动作/未请求restore/歧义window均失败1、app停止且私有dir移除；第五项app退出7保留7。wrapper sh -n、git diff --check、native clippy -D warnings、34native、6xtask、bindings-check已通过。Rust/pnpm依赖锁、CSP、vendor及release gate未变；Linux CI新增xdotool系统前置和新task，注册不是远端执行证据。
- 最后普通probe-channel（无Openbox/xdotool依赖的路径）完整运行也通过，DOM externalWmChecked=false、headlessfalse、nativeOnly/pause证据仍true、frames=792/renderEvents=789/nativeExit=0；之后全项目check/test通过，39app/89core/16IPC/6xtask/135TS保持。已有external JSON动作log未被覆盖，分别核对mode false/true互斥；本轮不重复已通过的外部WM正向样例。
- 该测试证明独立外部WM动作能驱动现有native-only pause/resume，仍由有限测试marker协调时间，不声称人工鼠标点击titlebar、真实长时间外部最小化、其它WM/Wayland/Windows/macOS或production ViewHost验收。失败路径恢复guard可尝试同窗口GUI恢复，成功external收据必须由外部controller日志和实际runtime状态共同证明。

## Generation ViewHost作用域与资源生命周期基础（Linux）

- 新增features/terminal/view-host.ts的GenerationViewHost/ViewScope，按pane管理后端ordered Begin提供的u64 generation；拒绝零/负数/旧/重复/越界，不损伤当前scope。begin先退役旧scope再安装新scope，retire只匹配current generation；retirement后last仍单调，父Abort/卸载dispose同步撤销。transition内abort callback重入begin拒绝，防覆盖并泄漏中间scope；resource按身份去重、WeakSet disposeOnce、倒序release，一个异常不跳过剩余，当前cleanup失败闭锁并用稳定错误报告。
- create提供scope自己的AbortSignal，只在current scope dispatch factory；取消立即reject caller，不等factory，仍观察尾部。晚到成功/注册立即清理一次，晚到失败/cleanup不能杀新scope；already-aborted factory不派发，当前factory失败清理已经登记资源。partial分配尚未返回仍须factory作者清理，不能保证未知factory内部副作用自动回滚。19项TS regressions覆盖完整创建/替换/退休/父abort/late success+failure/cleanup异常、去重/倒序/U64精度/重入及在途创建即时取消与尾部释放，154项TS全部通过，无unhandled rejection。
- finite AttachmentConsumer实际每Begin建立scope，DOM/headless异步import/创建经scope.create，renderer/headless/query mask/onData/consumer/input queue/idle登记统一cleanup。onData与idle repair旧任务错误按捕获view.isCurrent检查，禁止cancel后来新consumer；frame callback同时比较consumer identity与scope current。native visibility/controller listener跨surface退役仍活到resume并归pane lifetime，不能提前dispose导致丢失restore。renderer dispose只移除自己的element，不再host.replaceChildren误清新renderer；重复dispose幂等。
- schema4新增DomEvidence.viewHostChecked及server/xtask门禁，真实每DOM3次resize+1native pause/resume共4次scope重建。在new root完成后再次调用priorRenderer.dispose，要求当前root identity和scope current不变，否则失败；末次scope必须current且重建数4才填checked。这不是只验证bool存在；两轮DOM及headless仍保持PTY、old generation拒绝、精确history/repair、input、EOF/12行DOM全部原门禁。
- 初版scope接入默认hide/show与external WM最小化各一次完整运行通过（每次2headless+2DOM+7transport/cancel），viewHostChecked=true/generation=12/frames=787、parsedBytes=3,113,119/nativeExit0，external mode日志两轮4动作通过。最后补input/idle登记与迟到错误守卫后，默认组合再次通过：frames=785/peakCreditBytes4115、renderEvents=781/frameFences781、104×12/832×192px、digest=`00b088cbb609382c`/12行DOM匹配，viewHostChecked=true；未重复外部WM（新增部分用最新默认真实路径与154TS覆盖）。完整check/test在基础集成后通过39app/89core/16IPC/6xtask/154TS，最后JS资源扩展经tsc/Biome/154TS/默认GUI及bindings-check验证，native全套34/clippy -D warnings与新report fixture通过；Astrolabe依赖索引stale，仅参考，不是LSP clean。
- 未修改Rust parser/vendor/dependency locks/CSP/许可证审计；未开放user shell。当前是每pane generation资源作用域，不是完整按key复用的ViewHost pool，parking/LRU/time eviction、React ViewSlot/StrictMode双挂载、跨pane reparent、多window长期恢复/IME/DPI及跨平台仍待验收；生产attach精确恢复门禁不变。

## ViewHost稳定容器池、ViewSlot票据与park/淘汰基础（Linux）

- 新增view-pool.ts的ViewHost/PooledViewController/ViewLease与view-slot-ref.ts/view-slot.tsx。pool由main在React树外创建，稳定key复用controller和element；factory只同步构建controller、不open PTY，复杂async surface创建仍归GenerationViewHost。mount返回独立ticket/signal，React19 ref cleanup释放该ticket而非按key；直接same-key移到新slot使旧ticket失效，旧cleanup不能park新挂载。same-document/connected/非parking/cycle与slot独占先验证，非法key/slot不创建resource。
- park使mount signal同步abort、element进入hidden+inert parking，同步setVisible(false)，controller lifetime仍活着；重挂同key只setVisible(true)、复用element/controller。默认总32份（可配1–64）/parked最多6份/5分钟TTL，朴素有界Map+monotonic time/BigInt park次序，单次timer与sweep按TTL/LRU只淘汰parked，mounted永不受TTL/LRU淘汰。factory/visibility错误清理自己element与lifetime，父abort收尾全部controller；单disposer错误不跳过其它entry。
- dispose可返回Promise，1.5秒不确定截止只报告，不释放仍执行任务的额度。误用async setVisible adapter明确拒绝/观察尾部并计入同entry pending；两个visibility/dispose尾部均实际settled才从retiring移除。pendingDisposals也占maxViews，超时/错误不能无限重建堆积后台任务；旧entry失败或完成不remove同key的新entry，旧mount ticket abort/release不伤新挂载。factory未返回partial仍需自己清理。
- 25项池单测（minimal fake DOM，不当浏览器证据）覆盖key复用/同元素移动、StrictMode式mount-release-remount、hidden+inert/controller lifetime区分、TTL/LRU/mounted保护、容量、constructor/visibility/park/cleanup异常与parentabort、pending timeout额度、双尾部/误用async与slot占用、late cleanup、新旧ticket/ref callback。另3项view-deadline回归覆盖6秒明确到期/旧generation timer清理/parentabort与非法参数。182TS、全项目check/test（39app/89core/16IPC/6xtask）、tsc/Biome/Vite、34native/clippy-D warnings与bindings drift通过；mechanical UI检查通过，Astrolabe依赖索引stale仅参考。
- 实际App改为ViewSlot包稳定terminal-host（内层只有pool控制，不让React管理xterm子树），pagehide父lifetime回收pool/parking。两DOM samples结束要求主pool size=1/factoryCount=1。finite adapter park选择取消运行样例，不冒充生产session pause/resume；factory从未直接open backend session。
- 新EOF-only真实DOM测试用当前xterm root在两个slot间移动同entry、检查old lease.release无效、park到真实hidden/inert container且controller未dispose、reacquire相同element/root/factoryCount1、辅助parked LRU/TTL eviction而mounted root仍存活，dispose后root返回原host。注入monotonic测试clock只验证TTL逻辑，不等待真实5分钟。原backend PTY/native-only暂停/恢复在EOF之前走原门禁，停车DOM测试期间没有活跃输出，所以不算backend活跃session parked recovery。报告schema4新增viewPoolChecked、server/xtask硬要求true（native fixture补false拒绝）；scope的viewHostChecked仍保留。
- 初次组合失败：两headless完成后首DOM挂起，错误session.frontend_deadline，未取得DOM evidence。随后只加stable phase/r/b诊断的默认运行通过，原始停顿未复现且尚未根因定位，不能归因为CSS或称已修复。保留阶段/resize/rebind计数（不含terminal内容）诊断；finite expected fit→new Begin用6秒scope deadline，旧scope退役清timer，不把没有measurement的无限等待当成功。样例取消现在立即结束前端race并观察nativePromise尾部，70秒总截止不增加/不自动retry。以后再现可明确定位哪个stage，最初失败仍记为未解释的稳定性风险。
- 最新上述有界监督版默认hide/show与external WM最小化各一次完整运行通过（各2headless+2DOM+7transport/cancel），viewPoolChecked/viewHostChecked=true，generation12、3resize/1pause、104×12/832×192px、parsedBytes3,113,119/nativeExit0/digest=`00b088cbb609382c`/12行DOM一致。默认frames782/peak4115/render780/fences778/externalWm=false；external frames793/peak4115/render791/fences789/externalWm=true，四动作日志minimize6/restore6/minimize11/restore11且无Openbox/xdotool遗留。
- 本轮没有新增Rust/pnpm依赖、改锁文件/CSP/vendor或audit白名单。池和React ticket基础已实现，不标生产PTY停车/长时间hidden恢复、开发React StrictMode完整系统循环、多pane并发实际后端状态、cross-window DOM迁移/IME/DPI/Wayland/Windows/macOS或三平台性能完成；首次未解释DOM stall需持续跟踪。

## 检查结果

- Rust：89 项 pulse-term 单元/属性测试通过（新增2项bound resize ACK/no-op/取消/失败保持grid回归与1项subscribe commit gate回归）；pulse-ipc 16项通过（新增有序Repair、NoCredit稳定性和大整数边界）；pulse-app 39项测试通过（新增check_binding只读owner/generation/换绑/撤销无输入信用回归）；额外178项原上游测试通过；原生适配器现34项测试通过（新增4项external native reserve不等于pause、闭锁/wrong generation/失败revocation/early publication回归）；xtask现6项报告负向测试通过（新增2项外部WM动作收据完整/顺序/正整数规范/有界长度回归）；本轮未修改parser补丁或上游源码。
- TS：182 项测试通过（新增25项ViewHost pool/React ref ticket/异步cleanup信用回归和3项finite scope deadline回归）；tsc、Biome、Vite 构建通过；负向类型用例确认 ID、camelCase 配置和二进制 Channel 参数的错误会被 tsc 拒绝。
- Rustfmt、clippy `-D warnings`、初始依赖分层检查通过。
- 原来的默认特性 cargo-deny 通过不足以覆盖原生依赖。现已改为全特性审计：来源与 bans 通过，许可证及维护公告失败；没有放宽白名单或添加公告 ignore，详见 [待审查依赖门禁](./dependency-review.md)。
- pnpm audit 无已知漏洞；初始化时发现 Vitest 4.1.0 的公告，已升级到 4.1.11。
- Astrolabe 已用于骨架、Rust 符号精确引用、TS 文件依赖范围及 Rust 诊断。TS 语言服务器诊断超时，未取得安装授权，不能视为通过；TS 的类型证据来自 tsc。

## 尚未验证

S1 已取得 Linux 合成通道/背压的部分证据，但未完成三平台验收。ConPTY、窗口材质/IME、真实 SSH 互通、桌面显示与编解码、CLI IPC 仍未实现或验证。PTY 已有 Linux 真实交互与系统 Shell 冒烟证据，但交互/login shell 矩阵、Windows ConPTY、生产DOM GUI 接入与进程树回收仍待验证；Linux已有finite helper的真实WebView组合验证。类型生成已有 Linux 控制面证据，但三平台与 RC 依赖门禁未完成。已批准的非活动网格只读getter及双屏恢复已接入；隐藏光标分离/非行尾wrap等不能无损编码的状态明确延期，尚未接入生产attach；xterm.js 与 alacritty 的边缘序列分歧需要重新同步策略。Unicode 标量表已统一，但更多组合边界、字形与控制序列交互仍待验证。原生依赖门禁审查通过之前，不得打包分发验证应用。
