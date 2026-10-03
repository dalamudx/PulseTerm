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
- `IdleComparison`以600ms空闲、350ms证据期限、最多3次尝试调度；新输出入队、隐藏、替换或销毁拒绝迟到结果。未完成IPC仍占single-flight，失败不变成分歧。8项TS受控时钟测试验证有限重试、限频、隐藏、Data边界失效和取消。只有Commit/Checkpoint能提供revision，普通Data后必须等待新的有序检查点，不能猜测revision。
- 新生成`probe_session_compare`命令使用canonical十进制u64 counter（允许0）和显式小DTO，数据仍Raw Channel；generation必须非0。真实WebKitGTK完整probe连续两轮：错误/旧generation拒绝，人工改变摘要返回Mismatch，紧接重复请求因限频拒绝，600ms后真实headless回调摘要Equal。`channel.json`要求`session.idleChecked=true`，原7组运输与EOF校验同时通过。
- 限制：仅受控Commit idle验证，不自动重绘，不检查任意Data后的idle。下一步为同通道checkpoint请求、同generation/boundary原子恢复授权及生产DOM生命周期；Equal不证明hidden/history/parser一致。未更改第三方依赖、alacritty补丁或审计门禁。

## 检查结果

- Rust：77 项 pulse-term 单元/属性测试通过（新增9项原子lease/背压、同步Fence、EOF/取消/native resize回归）；pulse-ipc 14项通过（加入End阶段/额度/边界回归）；pulse-app 31项测试通过（7项ACK driver、13项registry授权/撤销/资源边界、2项终态回收、2项native exit/stop判定、7项generation摘要比较）；额外178项原上游测试通过；原生适配器现11项测试通过（新增origin/整数/证据验证）；本轮未修改parser补丁或上游源码。
- TS：68 项测试通过（新增8项idle比较调度/有限重试/迟到证据/隐藏与取消回归）；tsc、Biome、Vite 构建通过；负向类型用例确认 ID、camelCase 配置和二进制 Channel 参数的错误会被 tsc 拒绝。
- Rustfmt、clippy `-D warnings`、初始依赖分层检查通过。
- 原来的默认特性 cargo-deny 通过不足以覆盖原生依赖。现已改为全特性审计：来源与 bans 通过，许可证及维护公告失败；没有放宽白名单或添加公告 ignore，详见 [待审查依赖门禁](./dependency-review.md)。
- pnpm audit 无已知漏洞；初始化时发现 Vitest 4.1.0 的公告，已升级到 4.1.11。
- Astrolabe 已用于骨架、Rust 符号精确引用、TS 文件依赖范围及 Rust 诊断。TS 语言服务器诊断超时，未取得安装授权，不能视为通过；TS 的类型证据来自 tsc。

## 尚未验证

S1 已取得 Linux 合成通道/背压的部分证据，但未完成三平台验收。ConPTY、窗口材质/IME、真实 SSH 互通、桌面显示与编解码、CLI IPC 仍未实现或验证。PTY 已有 Linux 真实交互与系统 Shell 冒烟证据，但交互/login shell 矩阵、Windows ConPTY、生产DOM GUI 接入与进程树回收仍待验证；Linux已有finite helper的真实WebView组合验证。类型生成已有 Linux 控制面证据，但三平台与 RC 依赖门禁未完成。已批准的非活动网格只读getter及双屏恢复已接入；隐藏光标分离/非行尾wrap等不能无损编码的状态明确延期，尚未接入生产attach；xterm.js 与 alacritty 的边缘序列分歧需要重新同步策略。Unicode 标量表已统一，但更多组合边界、字形与控制序列交互仍待验证。原生依赖门禁审查通过之前，不得打包分发验证应用。
