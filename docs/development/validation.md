# 已实现范围与验证记录

本文记录可复现证据，不替代架构决策或验收清单。验证只在当前 Linux 环境执行；已添加三平台 CI，但尚无 CI 执行结果。

## 工程基础

- `.mise.toml` 固定 Rust 1.95.0、Node 22.23.3、pnpm 10.34.6、cargo-deny 0.20.2；任务统一使用 `mise run`。
- workspace 实例化 `pulse-core`、`pulse-ipc`、`pulse-term`、`pulse-pty`、`pulse-route`、`pulse-app`、`pulseterm-desktop` 与 `xtask`；原生 GUI 依赖受 `desktop` feature 控制，默认仍可做 headless 核心检查。
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

## 活跃pool停车、fenced resume与真实后端长期detach（Linux）

- ViewLease新增resume(slot)，只保留该ticket自己park后的同entry claim；挂载被替换/TTL/evict/已用claim不能重新factory或窃取新view。ViewSlotOwner分开React mount scope与内部pool lease，imperative park/resume更新当前lease，最终React cleanup跟随最新lease；旧ref cleanup/旧claim不伤新owner，mount observer失败rollback park。8项新增pool/slot回归，现33项通过。
- PaneVisibility绑定当前attachment控制面并同步pool false/true通知；false先SurfaceVisibility.retire本地consumer/render/input/idle，当前pane lifetime及backend SessionRegistry不关闭。old stop/abort与已取消晚bind不解除新observer，snapshot/notification失败明确清理，重复来源不重复detach/resume。9项bridge回归；没有活跃control bind时park仍fail closed取消finite样例，不虚构可恢复状态。
- 实际主React pool在第三resize后park，检查真实hidden ancestor/PaneVisibility=false，等待owner/nonce/generation pause票据，旧input/ACK拒绝，后台native PTY接受hidden控制后parsed_bytes增加、100ms旧Channel无新帧；原slot.owner.resume经claim重挂相同controller，container true驱动controller授权准确new Begin，恢复HIDDEN_SEED且pane signal未abort。随后native hide/WM minimize作为唯一native来源独立第二pause/resume，window状态严格回读；之后原query/repair/keyboard/EOF全门禁继续。
- finite driver cap由1 pause→2，gate.wait_resume已消费并清空票据，未放宽registry owner/gen检查或rollback；新2项native回归验证pool receipt未消费不能再pause、消费后更大native generation/early resume/old重复拒绝、Destroyed后不得开始下一cycle。DOM report activeParkChecked/pauseCount2/five rebuilds硬要求，headless0。
- 默认及外部WM各一次完整Linux组合通过（每次2headless+2DOM+7transport/cancel），activeParkChecked/viewPoolChecked/viewHostChecked=true、generation14、3resize/2pause、104×12/832×192px、parsedBytes3,113,152/nativeExit0/digest=`00b088cbb609382c`、12行DOM一致、主pool factory仍1。默认frames791/peak8212/render788/fences787；external frames794/peak4115/render790/fences790/nativeMinimize+externalWm true。有限停车不是生产任意长时间隐藏、真实React开发StrictMode整链、多pane/跨window验收；前次未解释初始化stall继续保留phase/6秒fit诊断，未在这两次再现。
- probe-local-session schema4新增detached：独立固定Rust helper保持stdin待命，真实registry binding Commit后detach，旧input exit被拒绝，backend trusted控制注入DETACHED_FIRST Unicode marker，等待实测6000ms仍Running，再注入DETACHED_NEXT并准确新generation capture，Rust replay全部cells与actor inspect一致，之后current bound exit/有序End ACK/nativeExit0/直接子join成功。证明backend无view本身不受GUI5秒有限策略关闭，不是跨平台/长期GUI恢复通过。report.detached renderer明确Rust validator而非frontend callbacks，Windows注册不是ConPTY执行证据。
- 全项目check/test 199TS/39app/89core/16IPC/6xtask、36native/clippy-D warnings、bindings-check与GUI及长期native probe通过，机械UI检查随后通过；未新增依赖或修改locks/CSP/vendor/审计ignore。生产attach精确性门禁保持，未给不支持状态输出近似帧。

## S4初始隔离SSH协议探针（Linux）

- 新增mise probe-ssh与xtask optional ssh-probe feature，russh0.63.3 default-features=false/ring，rand0.10配匹ssh-key0.7RC的rand_core0.10（0.9示例混用不符合真实API，实际编译修正）；仅xtask test feature，不连接用户endpoint、不启desktop生产SSH。纯控制server只listen127.0.0.1:0，固定synthetic身份/密码/exec，不执行任何shell；临时Ed25519 keys，client pinned完整PublicKey且明确拒绝host certificate，changed key必须设置mismatch并在任何auth handler被调用前失败。
- 真实TCP验证wrong password拒绝、正确password/publickey认证、PTY成功回执、window-change91×37被server观察、Unicode+字面shell符号数据回显精确、exit_status7后继续drain至EOF/Close且尾部SSH_END完整；nonfixed exec请求failure回执，client断开。server正常shutdown/listener task join并等待active handlers归零，失败Drop也请求shutdown/abort listener，不能以仅listener stop当连接排空证据。
- 本机系统OpenSSH10.0p2作为独立client连接controlled russh server：-F空配置、BatchMode/IdentitiesOnly/IdentityAgent=none、StrictHostKeyChecking=yes、独立临时known_hosts、host key ssh-ed25519、清SSH_AUTH_SOCK、不读用户config/agent。private key仅在tempdir（Unix file0600）写盘，读取stdout上限1KiB、5秒不确定期限，kill_on_drop；fixed exec stdout SSH_EXEC_OK及nativeExit7、stdout排空/join通过。未生成已存在用户凭据或改用户~/.ssh。
- 成功报告schema1全部安全/阶段字段经validator再写，开始运行先删旧成功报告；3feature单测验证pinned key匹配、username+credential拒绝、completed单flag不足，xtask feature9 tests与-Dwarnings通过。默认check/test 199TS/39app/89core/16IPC/6xtask通过；最新真实probe成功，server/client/系统child都清理。CI Linux新增explicit feature运行/检查/单测；仅注册不算远端CI或跨平台验收。首次timeout构造放runtime外导致panic，已修正在async block内部；lint test模块搬到EOF，无忽略生产warnings。
- 新crypto包完整cargo-deny全特性审查仍fail：license原6拒绝、paste/proc-macro-error维护2；sources/bans通过，无新增拒绝或漏洞项，详dependency-review。没有扩大allow/ignore，SSH-key RC风险单列；桌面树不含russh。S4未通过：只取得受控russh双向和OpenSSH10client证据，不是真实sshd7–9/dropbear/旧设备、keyboard-interactive/证书、跳板/forward/Agent/FIDO2/GBK、真实remote PTY或完整transport actor集成。

## S4真实OpenSSH服务端/原生PTY尺寸验证（Linux）

- 新增probe-ssh-system，复用xtask可选ssh-probe依赖，未加生产crate或新Cargo包；Linux当前/usr/sbin/sshd10.0p2/stty/既有/run/sshd。临时config/Ed25519 host+authorized keys/随机loopback端口，固定当前测试account；不改/etc/sshd_config、~/.ssh、passwd或用户权限。UsePAM=yes只使用现有系统account/session策略，认证只publickey；临时StrictModes=no为私有temp授权路径，不是生产策略。ForceCommand仅固定Rust helper，remote原命令非空拒绝、stdin只3类固定test命令且字面shell符号从不执行；agent/tcp/X11/tunnel/用户env转发禁用。
- russh pinned host key连接真实sshd，错key明确拒绝；公钥认证成功、PTY与shell request回执成功。实际helper通过stty size读取内核slaveTTY，初始24行80列；SSH window-change91×37后再读37行91列，不能只凭send queued或本地payload标记通过。固定Unicode/字面shell符号SYSTEM_ECHO验证；exit命令helper输出SYSTEM_END然后native退出7，client必须继续drain EOF/Close及尾部marker后断开。
- 正向退出在仍运行的master /proc/pid/task/pid/children上最多3秒等所有owned session children归零，再停止并wait master，分别记录nativeSessionsDrained/nativeDaemonJoined；只操作创建的daemon，不kill现有系统daemon。所有标准请求/等待3–5秒，总25秒；失败child kill_on_drop，fixed helper12秒watchdog，但不把该兜底或master kill冒充正向session排空。临时目录结束清理，report没有实际username/host key/密码/端口。首次未知startup占端口竞争由pinned随机hostkey拒绝，不能连接非owned daemon并假成功。
- 两次真实运行通过（第二次加入native child drain/完整validator），报告serverVersion OpenSSH_10.0p2/publicKeyAuth/realNativePty/nativeSessionsDrained/nativeDaemonJoined/EOF均true、[80,24]→[91,37]、exit7。后续新增1项真实report负向fixture（false nativePTy/child/eof/master、错尺寸/exit、missing limitations拒绝），feature10tests/clippy-Dwarnings通过；默认全check/test199TS/39app/89core/16IPC/6xtask及原probe-ssh仍通过。
- 新mise/xtask CLI/CI entry与报告字段已注册；CI Linux系统依赖openssh-client/server及/run/sshd目录明确，但无远端执行证据。本轮Cargo.lock/deny/vendor/CSP不变，原全audit6license/2maintenance阻断继续。S4仍不全通过：实际只有OpenSSH10server与10client，7–9/dropbear/legacy设备、keyboard-interactive/证书、跳板/SFTP/forward/Agent/FIDO2/GBK及生产actor transport尚未验收。

## S4受控keyboard-interactive挑战/回答/取消（Linux）

- probe-ssh controlled server新增K-I handler，每连接challenge布尔状态，仅当前synthetic user的start发1个固定prompt/echo=false，回应必须exact单答且当前challenge有效；回应时先消费challenge，错误答拒绝、没有challenge不能接受。client校验name/instructions/数量/文本/echo都为受控值，unknown/额外prompt/echo真拒绝且不记录远程内容/回答。所有值是fake测试凭据，不访问实际MFA或用户secret。
- 真实encrypted loopback先挑战→错synthetic答返回Failure(partial_success=false)，重新start fresh challenge→固定正确答返回Success；另一独立connection start后不给答案直接disconnect/drop。handler Drop对尚未答复challenge计数，必须在全server shutdown之前3秒独立出现cancelled=1（而非用global shutdown冒充取消）；started3/denied1/accepted1硬验证，最终所有active connections仍归零。
- ssh.json新增keyboardInteractive五字段且validator硬要求，明确noRealMfaHardware=true；受控protocol已通过不代表真实MFA/PAM/前端Prompt ID生命周期或FIDO2验收。新增1项prompt负向单测，11 SSH-feature tests/clippy-Dwarnings通过；probe-ssh与真实probe-ssh-system都再运行成功（real PTY仍80×24→91×37/nativeExit7/session children drain）。未改Cargo依赖/locks/CSP/allow/ignore。
- OpenSSH7–9/dropbear/legacy设备、证书/jump/forward/Agent/FIDO2/GBK和生产SSH actor仍待后续验证，S4不标全通过。

## S4逐跳独立host-key校验与direct-tcpip relay（Linux）

- 新probe-ssh-jump使用两个controlled russh endpoints，仅127.0.0.1随机port。outer先pinned/authenticate，direct-tcpip white list仅owned inner exact host/port、不得DNS/外部IP/错port；单owned Semaphore permit从TCP connect准入到worker退出，不在timeout取消后提前释放，第二forward明确拒绝。6秒copy_bidirectional监督和8KiB test packet/output cap；这不是完整SDK缓冲/RSS/backpressure高压证明。
- inner KEX/client::connect_stream通过outer SSH channel.into_stream，使用不同expected host key，wrong inner key必须实际触发inner Pin hook并失败；cannot inherit trust from authenticated outer。首次实现outer Handler.data对forward ciphertext误Disconnect，错误早于key验证，未把它算成hostkey拒绝；修复为outer数据交channel stream、只有inner应用channel echo，后实际hook reached/negative gate通过。
- 先拒外部目的地，再wrong inner key/取消outer，要求outer relay+两边connections在listener still alive时独立归零；重新outer/inner authenticated，第二并行forward拒绝；内层应用回显4400 bytes UTF8及字面shell符号精确，EOF/Close均明确收到后，两SSH连接断开、relay+connections在server shutdown前归零，最后join两个listeners。不是发送exit或仅listener停止便声明所有任务已排空。
- 成功report schema1包含逐跳pins/inner mismatch/destination拒绝/singleForwardCredit1/secondForward拒绝/nestedBytesExact/innerEofAndClose/both connections+relays drain/listener join并validator逐项要求。新增2单测验证exact owned loopback/port与单completed不足，SSH feature13 tests/clippy-Dwarnings通过；全默认check/test199TS/39app/89core/16IPC/6xtask与原K-I/real OpenSSH10server probes再运行成功。CI Linux新增调用，未取得远端CI；Cargo依赖/locks/CSP/audit配置未变。
- S4仍未全验收：两个controlled endpoints不代表OpenSSH7–9/dropbear/真实多级跳板实现矩阵，生产Route连接池、任意目标/remote-forward、SFTP/Agent/FIDO2/证书/GBK、实际slow consumer SSH窗口压力与actor集成待验证。仍不开放用户endpoint或production credentials。

## S4限制性用户证书认证/CA与subject持钥（Linux）

- 新probe-ssh-certificate，临时Ed25519 CA/subject与独立pinned host，loopback controlled server仅certificate auth（其它方法不开放）、fixed exec。政策用Certificate.validate_at验证CA签名/可信SHA256指纹和时间，另补User类型、当前expected username、exact principal存在与critical_options空。仅verify_signature不能当信任，库validate也不自动验证principal/critical options；本test policy拒所有critical选项而非忽略force-command等限制。没有生产CA信任库/撤销政策/用户UI采用承诺。
- 真实加密SSH连接依次使用6invalid cert：unknown CA/expired/future/wrong principal/Host type/critical force-command，全部authenticate failure；随后wrong username/不同leaf私钥搭配valid cert也明确失败，确保CA签名不等于client私钥持有证明。所有negative后accepted=0，再1valid cert authenticated、固定exec CERT_OK、exit7+EOF/Close/output drain，所有connections在server stop前独立归零，最后join listener。CA/keys只在内存，不采用户credential或写CA secret。
- 2新增feature单测确定性timestamp100..200：100 inclusive/199 valid/99未来/200 expired、wrong CA/username/principal/type/options拒绝；显式Builder.all_principals_valid生成golden ticket也被此policy拒绝。report必须negativeCertificates6、wrongUsernameRejected/leafKeyOwnershipRequired及所有CA/time/type/principal/EOF/drain字段，单completed不足；15SSH-feature tests/clippy-Dwarnings、真实cert probe通过。默认check/test199TS/39app/89core/16IPC/6xtask及K-I/OpenSSH10server/jump probes再通过。
- 新mise/xtask/CI及ssh-certificate.json已注册，本轮无新依赖/locks/审计配置变动，原6license/2maintenance release gate保持。受控认证证明不代替OpenSSH7–9/dropbear/实际企业CA/撤销UX、主机证书信任、Agent/FIDO2/PKCS11/密钥加密、GBK或production SSH actor/SFTP/转发验收，S4仍未全通过。

## S4私有软件Agent签名/SSH signer认证/identity清理（Unix/Linux）

- 新probe-ssh-agent仅Unix可编译运行，ssh-agent -D使用私有temp sock、explicit AgentClient.connect_uds，不connect_env/读取用户identities，不改全局SSH_AUTH_SOCK；子ssh-add只override其私有socket/无SSH_ASKPASS/无stdin prompt。初始Agent列表为空，加载1份临时Ed25519软件key（0600/目录0700/30秒TTL），身份必须匹配generated pubkey。
- 真实Agent raw SSH Signature验证用signature::Verifier（与SSHSIG namespace文档签名不同）；真实SSH authenticate_publickey_with only publickey+Agent signer，不把subject私钥交给SSH client。controlled loopback pinned server固定exec AGENT_OK/exit7/EOF/Close/输出排空与connection-before-server-stop归零，listener正常join。
- remove_all_identities实测earlyEOF，上游0.63.3请求前4字节零header问题单列dependency-review，不忽略失败或修改upstream。改用scoped remove_identity同key，Tokio duplex test确认frame length/message18/key string长度；实际Agent identities为空、removed key签名明确AgentProtocolError而非任意IO失败、随后再次list成功且空证明Agent仍活着。最后ownedAgent kill/wait join，tempdir清除，用户Agent无操作。
- report ssh-agent.json/isolation/software-vs-hardware/签名/auth/移除拒签/EOF/server+Agent cleanup全字段validator硬要求，缺completed-only拒绝；2新增单测，17SSHfeaturetests/clippy-Dwarnings、实际Agent probe通过。全默认check/test199TS/39app/89core/16IPC/6xtask与此前K-I/real OpenSSH10server/jump/certificate4探针再次成功，CI Unix-Linux调用已注册。无新增依赖/locks/audit修改。
- 这只是软件Ed25519 Agent路径，不算sk-ssh-ed25519/FIDO2真实触摸/PIN、hardware失联/用户确认、remote agent forwarding/Pageant/windows pipe/macOS矩阵、production credential vault或actor SSH adapter完成。S4仍有真实设备/实现矩阵未验证，release gate6license/2maintenance不放宽。

## S4受控Agent forwarding显式channel grant（Linux）

- 扩展私有软件Agent场景，system OpenSSH client -a与-A分别连owned pinned russh server，仍-F空config/private known_hosts/private IdentityAgent/socket/key；不接用户Agent或其它identity。server按当前ChannelId记录agent-request授权，不把connection一次授权泛化到任意channel；未grant的forward-fixed-exec明确channel_failure并denied计数1，不能把客户端generic网络失败算作权限拒绝。
- -A明确开启请求后，server handle.channel_open_agent经SSH认证通道反向拿Agent流，request-identities必须仅唯一synthetic publickey、raw sign固定数据并本地Verifier验签。只签名和public身份通过隧道，不传private key给server。一个owned task permit贯穿3秒query/sign/reply直到实际task结束，Guard计active与completed，最终completed1/denied1/active0与全部SSH connections0在listener stop之前证明；系统client exact FORWARDED_OK/exit7/stdout drain，之后仍scoped移除synthetickey并拒签/ownedAgent daemon join。
- report.forwarding新增五字段（explicitChannelGrant/disabledRefused/signVerified/credit1/taskDrained）validator硬要求，新增permit/Guard生命周期单测，18 SSH-feature tests/clippy-Dwarnings通过；实际probe-ssh-agent、原4SSH probes和全默认check/test199TS/39app/89core/16IPC/6xtask再次成功。没有新dependencies/lock/audit/CSP/vendor修改，上游remove-all帧bug仍记录、没有忽略。
- 仅受控OpenSSH-client→russh-server/private软件Agent转发，尚未实现/验收Pulse客户端reverse-channel bridge或生产默认禁用/按host确认UX、realFIDO2触摸/PIN/断开/复杂Agent操作取消、Pageant/Windows pipes/macOS、任意remote forward或长期pressure。单task额度不承诺Agent硬件操作会随网络timeout取消；S4仍部分证据。

## S4旧KEX/MAC的显式test-profile opt-in（Linux）

- 新probe-ssh-legacy复用private real OpenSSH10 daemon，仅其临时config offer diffie-hellman-group14-sha1/aes128-ctr/hmac-sha1。client普通Preferred默认连接在host-key hook/认证前typed Error::NoCommonAlgo(kind=Kex)拒绝，不能用generic网络/timeout/主机key错误伪安全拒绝。
- 仅此显式CLI/profile构造Cow owned列表1旧KEX/1MAC/1cipher+Ed25519 hostkey，复用真实sshd正确pinned key/publickey/native PTY/stty80×24→91×37/UTF8/exit7/EOF及owned children/master join；wrong pinned host仍拒绝。之后再次ordinary default连接同legacy-only daemon仍typed拒绝，说明opt-in没有修改global defaults或auto retry/downgrade。现代profile完整再通过，flag分别false/true并存独立ssh-system.json/ssh-system-legacy.json，mode_matches禁止错模式、缺bool或string冒充。
- 2新增单测检查create legacy后默认KEX/MAC相等且不含SHA1、legacy只Ed25519，以及report互斥/缺flag拒绝；20 SSH-feature tests/clippy-Dwarnings与modern+legacy实际运行通过。其余K-I/jump/cert/Agent-forward4探针及default check再成功，CI追加明确legacy调用；没有新依赖/lock/audit/CSP/正常daemon配置改变。
- 注意russh Preferred默认safe KEX/CIPHER/MAC排除旧SHA1/CBC，但key列表仍含RSA{hash:None}（ssh-rsa）；本profile RSA feature关闭且只有Ed25519，不能据此承诺启用RSA后的host-key列表也完全现代。生产legacy-per-host政策需另行过滤旧host-key算法并明确用户opt-in，不照搬全DEFAULT key列表。未启group1/DSA/3DES/RSA-SHA1，不验证真实旧设备及用户警告UX；S4矩阵未全完成。

## S4嵌套SSH二进制GBK回显与严格分块转码（Linux）

- 新test-only gbk_probe复用owned双SSH跳板通道，输出固定中文/ANSI/DSR fixture编码为GBK实际33 bytes（UTF8则43 bytes）；真实channel传输、回显原始bytes完全相同，不将SSH Data强行from_utf8。返回gbk报告并纳入jump成功validator，server应用bytes计数同时包含UTF8 4400和GBK33。
- 编码strict检查GBK.encode had_errors，unrepresentable emoji明确拒绝而非numeric entity替换；decode使用有状态new_decoder_without_bom_handling/decode_to_string_without_replacement，bytes≤4096/out-cap16KiB/完整consumed&InputEmpty，最后empty last=true排空。所有chunk sizes1..33（含leadbyte边界）均得到exact43 UTF8 bytes，与reference backend TerminalProbe cells/DSR replies一致；非法0xff/半个lead0x81EOF均fail而非U+FFFD隐藏损坏。
- 新3单测覆盖all splits、malformed EOF/unsupported input、bound/缺report；23SSHfeaturetests/clippy-Dwarnings及真实jump+GBK通过。全默认check/test199TS/39app/89core/16IPC/6xtask与其余5SSH模式再次成功；CI既有jump调用即强制新GBK报告。新增encoding_rs0.8.42与3transitive包，完整audit仍原6license/2maintenance失败，sources/bans通过无新增拒绝，原锁定包版本未升级。
- 严格边界：只是controlled GBK echo+后端解析归一化测试，不是实际GBK交换机/自动侦测/所有GBK扩展/生产codec。raw source bytes33与logical UTF8 bytes43不同，不能直接把现有attachment parsedBytes/raw-offset定义当已经适配；actor original-byte/reconnect/history/input query transcoding及Zmodem在转码前分类仍未接。原协议精确性/flow-control门禁不放宽，生产charset支持未标完成；S4仍部分矩阵。

## ADR-029双坐标v2与validation-only GBK actor恢复

- 用户明确选择版本化双坐标，保留原始SessionState.parsed_bytes并新增utf8_bytes/source_is_utf8；协议v2独立于原v1，未改旧parsedBytes公开语义。Rust attach_v2 + TS AttachmentV2Receiver/Consumer以28-byte envelope/36-byte Begin分别追踪source与display。part IDs/ordered channel/credit共用既有FrameKind8，互相reject mixed versions/unknown flags，bigint保持大于2^53精度。
- source消费无显示（半GBK lead）发布pure Marker，decoder在backend actor且跨lease存活。v2 Data按实际body长度推进UTF8，source独立monotonic；Checkpoint/EOF/Repair保持两个cursor一致，sourceRegression和source正确/display错误均failclosed。renderer摘要SessionState比较包括两个计数及mode；当前desktop仍UTF8-v1-only，未增加GBK窗口命令。
- pulse-app attach_v2显式准入，复用同一Driver的ACK/timeout/cancel/EOF/repair监督，driver livecursor改为state.utf8_bytes；Fragment末记录原source终点，不推算一对一raw映射。v2limit在atomiccapture前校验；v1先拒source_is_utf8=false，哪怕source/display均0，保留旧lease。3app回归证明invalidcredit保旧lease、wronggeneration只退视图新view继续EOF、noACK实际停actor且timeout不关session。
- validation feature才可调用spawn_gbk_text_session，未接productionbinary/Zmodem分类。rawchunk≤64KiB，单次最大3×64KiB+4 scratch并按≤64KiBrecords有界分发，source在第一份decoded record消费一次，后续source不增加；满lease阻止pending解析/继续读transport。4core tests覆盖半GBK原始1/UTF80跨capture→trail原始2/UTF83，64KiBsource→98304UTF8两记录，input中文反编码/emoji拒写/query反编码，malformed及halfEOF=Failed不当normalEOF或replacement。
- 共享attachment-v2.hex由Rust sender byte-for-byte校验，包含独立bigintsource/display、source-onlyMarker、中文展开Data、samecursorRepair与EOF；TS8tests检验mixedversion/duplicatedsource/sourceRegression/repair+EOF mismatch/oldgen/queuedcallbackACK/cancel+latewrite/unsupportedmarker/overflow。Rust额外pre-copybody bound与Commit释放bootstrapVec容量回归；v1原全部用例保持。
- 新mise/xtask/CI probe-encoding-attachment两份actualactor→v2→headless样例（含history），准确半GBK捕获与新lease重建，EOF source7/UTF89和source82/UTF884均与Rustscreen一致，无frontendquery泄漏。生成阶段ACK先保存wire以运行actor，TS独立callbackgated消费再ACK，不称同进程roundtrip或realWebViewGBK证据。默认prod-feature独立consumer禁止test-onlyGBK入口，existingreplay/Echo门禁保持。
- 验证记录：核心组合check/test207TS、93core/42app/22IPC（后续pre-copy回归增至23）、v1五headlesscase与v2两case通过，默认API隔离通过。原生UTF8-v1真实Channel/DOM复验completed=true（2sessionRuns/2domRuns/7measurements），bindings-check/native clippy-Dwarnings/36native tests通过；encoding_rs没有新版本，仅lock增加pulse-term可选依赖边，审计许可/公告门禁没有豁免。首次运行在2headless之后/首DOM期间native_query_failed，root cause未定位；补static分类日志但500ms/failclosed不放宽，后续通过不宣称根因已修。
- 未完成：productionbinarypreclassifier/临时encoding切换、多charset/真实GBK设备、Tauri窗口v2适配/DOM/三平台编码acceptance，S2/S4仍不标全过。UTF8原有invalid字节/EOF replacement行为不改，strictGBK拒绝政策目前只对已分类测试文本有效。

## v2注册层授权与实际GBK bound input/单坐标摘要拒绝

- 新SessionRegistry.attach_v2显式选codec，复用相同attach_version实现，不新增授权token或generationcounter。先owner/session校验，预留monitor/capture锁，再mint generation，captured后再次核对owner才install，oldbinding detach和latecompletion只清当前matching generation。默认attach仍v1；只有可信backendcaller选择v2，未增加Tauri前端参数/任意会话入口。
- 3新增registry tests：foreign v2capture/ACK/input拒绝、windowclose撤view（appshutdown才关actor）、v1→v2 replacement后oldACK/oldDetach/lateCompletion不能影响newbinding、invalidv2credit保旧v1。45app tests、wholeworkspaceclippy/check与真实probe-local-session native ACK/2MiB pressure/detach/EOF链通过。
- 两份actualGBK headlessprobe由裸attach迁移为registry-owned，外国window capture/ACK明确Forbidden，owner旧generation ACK Stale；boundInput中文真实传输driver收到GBK D6D0CEC4，raw相同而UTF8cursor错误的compare必须Stale，不能清risk或误判Equal。报告三个新增必需bool和TS validator强制检查，headlesshistory/屏幕/EOF source7/utf89与source82/utf884仍精确；没有把producerstaging ACK说成liveJScallback roundtrip。
- no new deps/locks/capabilities/audit变化；此前v1真实WebView复验属于1214679检查点，当前registrywrapper变更采用45unit+realPTY与GBKprobe targeted checks。Tauri窗口v2选择、实际DOM和生产charsetbinarypreclassifier仍未验收，源码不能因backendregistry API存在就开放未批准的credentials/shell。

## v2固定UTF8样例的真实Linux WebView/DOM验收

- 新mise/xtask probe-channel-v2、backend --probe-v2明确选择codec2，仅owned fixed UTF8 PTY helper，没有GBK/任意shell/remotecredential入口。readonly generated probeSessionProtocol只在local origin可调用，frontend选择必须1或2，不从浏览器参数替换后端codec；allcapture/resize/resume用同registry.attach_v2。默认probe-channel/window/external仍codec1，reports独立channel-v2.json。
- SessionEvidence attachmentVersion由backend产出，xtask/headless/DOM全部必须匹配请求mode；新增模式wrong/mixed/缺version unit，v1完整report不当v2。信用精确按fragment8192+envelope20/28，因此v1≤8212/v2≤8220，而不是全局放宽。native unit覆盖8213-v1拒、8220-v2允许/8221拒、unknown3拒，native报告复用sessionvalidator消除重复8212常量。
- 新ProbeConsumer UTF8-only适配器底层为真实AttachmentV2Consumer，私有legacy renderer只在控制边界source==utf8时投影，source-onlyGBK marker/不等Begin明确在render/ACK前拒绝；不让shim冒充GBK显示支持。一般GBK客户端仍直接用双坐标sink。3TS回归mixedversion/flags/unequalbootstrap不reset不ACK/equal边界renderACK/unequalmarker拒，合计210TS，native36tests通过。
- 实测channel-v2 completed=true：2headless/2DOM/7原测量，session attachmentVersion2/dom2，headlesspeak8220/dom8101，DOM每轮resize3/paused2/input+repair+idle+native visibility与EOF一致，实际Raw Channel仍ArrayBuffer。不是性能三平台或GBK DOM声明。第一次因budget误用8212被capture前拒绝；修到8220后两DOM完成但总report重复v1常量拒，改复用版本化SessionEvidence后最终成功。两处都保留hardgate，未扩超时或skipchecks。
- 最终default-v1真实probe-channel completed、typed bindings-check、workspacecheck/tsc/Biome/nativeclippy-Dwarnings再次成功，先前v2有36native tests/210TS，xtask新mode单测通过；CI新增Linuxv2但保留v1/WM测试。无newdependencies/locks/audit/CSP改变。既有native_visibility采样一次失败仍rootcause未定位，不能因最终通过宣称全解决。
- 当前完成v2固定UTF8真实窗口组合，不是productioncharset；GBK DOM、binary/Zmodem预分类、temporary charset切换、真实设备、Windows/macOS依然pending。release6license/2maintenance门禁不放宽。

## 显式验证特性的GBK双坐标真实Channel/DOM样例

- 新desktop encoding-probe feature显式启用pulse-term/validation，default/desktop不自动启用。默认compiled命令只返回encoding.validation_feature_required，--probe-encoding不带feature直接exit1、不启动GUI；terminal-api-check仍确认默认外部consumer无法访问GBK/replay/Echo。generated command surface在两feature中一致，避免drift，但生产textclassification尚未实现。
- 新mise desktop-encoding-build/probe-encoding-window与xtask独立encoding-window.json，only固定已分类文本，不创建用户shell/远端连接/任意charset入口。复用WindowSessions可信owner/nonce/独占slot/RunGuard和registry.attach_v2，fixedinput只中文/确认，旧generation ACK不能影响新binding，任何error走guard撤销/关闭并清理。CI显式build模式与原默认v1/v2/WM验证分别保留。
- 实测两轮GBK actor→Raw Channel ArrayBuffer→AttachmentV2Consumer→可见DOM→callback ACK成功。history15row+半lead初始capture source76/display75，两generation准确恢复；Commit渲染后才输入握手，后端driver收到D6D0CEC4/C8B7C8CF反编码。续流lead产生pureMarker source78/display78，只render fence不写VT，cursor/baseY不变；trail续流中文/GBK_OK，最终source89/UTF891/revision5/digest78682c0fbbb6bc30与backend一致。report replacement1、145frames、peak64、DOM12行、history/sourceOnlyMarker/reverseInput/callbackAck全true；原2headless/2UTF8DOM/7测量也完成。
- 不走等坐标UTF8 shim，renderer只接columns/rows用于几何，所有V2控制事件保留完整两计数；双坐标final/digest及history实际检查，不能把headlessproducer预ACK称为这轮真实ACK。2新native证据回归raw==display伪成功/缺DOM/无ACK/信用65/旧replacement/非法counter拒绝；xtask新增缺字段/无renderACK/同坐标负向。第2case失败保留第1成功和原error，但不能completedtrue；额外native报告回归证明该边界。
- 最终普通v2重建（encodingRuns0、encodingnull）/check/fulltests/GBKheadless/encoding-probe严格clippy全通过；核心93term/45app/23IPC、39native、210TS、8xtask。defaultAPI隔离/GBKmode明确拒绝及bindings-check通过，Astrolabe Rust新模块无error诊断；TS证据来自tsc。无lock/newdependency版本/CSP/audit豁免；only featureedge启用已有encoding_rs。
- 未完成：真实GBK SSH/PTY传输与设备端画面、binary/Zmodem预分类、临时charset切换、生产入口、三平台/IME。已批准strict策略仅对分类fixture有效，不能标S2/S4全通过。

## S4受控双跳SSH到GBK actor/registry/v2桥接

- 既有probe-ssh-jump强制新增独立authenticated session，reuse两hop单独pinned hostkey和temporary synthcredentials。realSSH收到leadD6才喂boundedDriverEndpoint，source1/display0时capturev2，v1拒绝且旧lease被commit替换。trailD0在重建后续接，boundInput中文经actor反编码D6D0CEC4再发送SSH，原样echo回actor，不做from_utf8强解。
- 远端echo DSR触发backend专有TransportWrite::Reply，明确与用户Input区分；收到ESC[1;7R并实际回写SSH，再按raw字节echo解析，backend仅答一次。remoteEOF/Close两收据都成功后才drop driver，actor正常EOF排空，wire.Data UTF8offset按bodylen连续/source单调，唯一End source16/UTF819，与SessionState及UTF8 reference全Screen相同；PeakCredit64。SSHserver总应用bytes增16，原4433变4449，双连接/relay仍drained/listeners joined。
- actualSSH probe通过；2新增metadata negative tests拒同raw/display、假pendingUTF8、65credit和缺query/input/EOFstage证明。27 SSHfeature tests、SSHfeatureclippy-Dwarnings、全workspacecheck/tsc/Biome通过。无newdependencies/lock/desktoppermissions或audit变化，既有CI jump现在强制gbkActor证据。MCP首先找出新增testmod与innerdoc顺序错误，已修复并经编译/clippy确认；一次随后诊断仍返回旧line5错误，缓存不能当最终通过。
- 限制：每阶段有限echo并施加timeout/bytes cap，尚非SSH slow-consumer端到端窗口/ACK压力验收；没有生产SSH transport、GBK远端设备、同条网络到DOM组合、binary/Zmodem预分类或charset切换，不标S4完整通过。先前GBKDOM来自固定本地driver，此处是另一独立真实网络检查。

## 真实远端SSH发送窗口背压与共享连接HOL限制

- 新mise/xtask/CI probe-ssh-pressure，仅私有回环russh端点/合成credential。remote fixedexec产生4193280 GBK source（8190×512），通过真实ChannelWriteHalf.data按SSH窗口reserve发送，utf8显示4892160。client receivewindow32768/channel_buffer2/packet8192，actor transport queue8和lease/wire有界，frames1/credit8220。初版同步Session.data echo将4MiB排到server内部pending，即使actor/reader停也不阻producer，失败已确认，改为window-aware生产者重测，未放宽停止断言或修改第三方源码。
- ACKhold后700ms与后续窗口稳定：actor source57338、bridge读114660、remote生产155610，三者均停止，remote producer未完成；actor diagnostic_digest300ms回应，UI无ACK不阻actor控制。恢复ACK后逐UTF8字节pattern/offset/source monotonic和唯一EOF检查通过，backend全Screen同reference、normalEnded、远端EOF/Close完整，private producer/连接/listener全drain/join。原jump（UTF8+GBK+actor16bytes）再次通过。
- 共享connectioncontrol350ms不能完成，独立connectioncontrol500ms回应，恢复ACK后原control3s内成功。源码client/encrypted.rs的boundedchan.send.await确认HOL；handler callbacks在这之后执行，不能仅改应用Callback消掉阻塞。该失败语义在报告显式oldConnectionControlBlocked=true/limits中，意味着生产SSH复用/混合SFTP+shell+forward仍未验收，不是复用通过。
- 另两条real SSH压力路径Detach及AckTimeout精确匹配理由，均仅撤view，remote继续完整4193280/4892160，actor normalEnded全Screen同reference，同连接control恢复，producer/连接/listener清理。超时3s只用于该受控样例；未改变生产超时或命令队列。禁止把正常cancel当完整有序viewEOF，但底层SSH EOF/Close与actor EOF均完成。
- 3新增负向用例拒remote全部提前完成、bridge超1MiB、假HOL/缺独立连接证明、错releasekind/误关actor/信用8221。30SSHfeaturetests、SSHfeaturestrictclippy、jump兼容与workspacecheck/tsc/Biome通过，Astrolabe新pressure文件无error诊断。all-featureaudit重跑仍原6license/2unmaintained阻塞，sources/bans通过，未ignore/变白名单/升级依赖或锁版本。
- 未完成：生产SSH adapter/每通道credit调度与复用隔离、真实设备/多版本压力矩阵、GBK同条SSH到DOM组合、binary preclassifier/charset切换。此处已确认的同连接HOL必须进入后续设计门禁，不能用headless/受控回环推断真实网络性能。

## S5本地CLI协议与独立Unix socket子进程（非完整生产CLI）

- 新pulse-ipc::cli公共模块：4-byteLE长度+64KiB JSON body，header0/过大在bodyallocation之前拒；Decoder每push最多一frame/返回usedbytecount，任何截断EOF明确失败。boundedwriter防serialize巨大中间Vec，Zeroizing<Vec>所有wire/暂存body在drop清理。JSONRPC2严格object/stringid≤64/method≤96/objectparams，不接受batch/notification/numericID/未知字段，serde拒重复字段/非法UTF8。
- SessionAuth第一消息hello params={protocolVersion:1,token}，固定64hex token用subtle恒定时间比较，expectedtoken/消费hello字符串零化，无Debug/Display或请求内容错误。failure锁死gate，重复hello/id或128requestbudget拒，auth本身不等于Capability/窗格权限，不将method当shell。10新单测/property覆盖所有分块/header/最大body/EOF/JSON subset/版本/密钥/重放/有限额度。
- 新optional xtask cli-probe/mise probe-cli，用既有Tokio nativeUDS作有限可重复验证（不是规划interprocess productionadapter）。随机私有runtime0700、token+socket0600；启动token是两次OS随机UUIDv4组成64hex，不在argv/env/report；客户端和服务端均核验peer UID，secret只经私有token文件读取。实际10个新子进程证明合法hello/readonlyping（含中文emoji）/错误token+版本/握手前调用/oversizeheader/partialEOF/idle500ms总截止/unknown shell.exec拒绝；symlinktoken/文件0640拒，临时目录/连接/listener/子进程全部清理。
- CLIreport强制8失败/成功精确观察序列、10process及所有安全bool，不接受flags-only。2新probe单测验证目录/文件mode与symlink拒；cli-feature10xtask tests、strictclippy、真实probe两次通过，MCP cli.rs/cli_probe.rs无error诊断；workspacecheck/33IPC/93term/45app/210TS、v1五headless/v2两GBKheadless/API isolation通过。
- fulltests发现旧GBK expansion test在close()之后立刻drop driver，让EOF和cancel同时ready，公平select偶发normalEnded而非预期Closed。独立签名72a2bb7先await有限1s close终态后drop，Closed断言不放宽，生产actor不改；再跑完整suite通过。serde/subtle2.6.1/zeroize1.9.0/uuid只新增已锁定依赖边，没有版本升级/newcrate；all-featureaudit仍原6license/2maintenance拒，sources/bans通过未豁免。
- 未完成：生产interprocess端点/WindowsNamedPipeACL与真实跨用户拒绝、clap独立pulseterm入口、共享Capability授权/风险确认、单实例/冷启动--startup-actions/50ms窗口创建、S5三平台验收。这里的ping RTT/原生UDS和Linuxpeer证据不能当上述能力已完成。

## S5真实CLI到Tauri native窗口收据与加载时序（Linux）

- 新显式desktop cli-window-probe/mise desktop-cli-build/probe-cli-window，仅固定probe.window且emptyparams，nativeUDS/token/peerUID准入后在Tauri mainthread创建480×320本地页面，无shell/任意URL/窗口label输入。main page Finished后才启动private服务/三个独立helper（wrongtoken+两次good）。wrongtoken返回拒、窗口数不增加；两次真实窗口visible+runtime注册+pageload+Destroyed全部确认后windowcount回初始，client/listener/private runtime清理。
- 第一版JSONRPCresult等待load和destroy，CLI完整回执346–396ms而servernative9–11ms。改唯一result在真正native注册/visible检查后发，不仅回accepted；page-load与destroy仍独立await。最终sample1 servernative7004us、clientrequest/native8022us、load330244us；sample2 native10680us/client11872us/load333854us。前两个是受控native状态收据，不是compositor首像素/完整terminal可用，不能声称S5生产显示50ms全通过。未运行的客户端拉起/冷启动竞争还没接。
- main-thread回调先检查oneshot接收端是否已取消，build后投递失败主动关闭latewindow；正常/错误都有WindowGuard关闭，page/load/build/destroy各有限deadline。runnerLinuxGNUtimeout40秒监督整个Xvfb/dbus/GUI进程组，parent50秒观察上限，既有DISPLAY也有超时。CLIhelper仅good/denied，mode缺report或与自动benchmark混用拒。defaultdesktop二进制直接--probe-cli-window exit1/cliwindow.validation_feature_required，不启动GUI。
- 两次真实probe（最后修改只报告/negative/监督）通过。2新增xtask negative tests拒假visible/倒序native或load时间/缺清理/单sample，并允许nativeACK早于pageload（不混两个phase）；CLIfeature12xtask/native39tests、两featurestrictclippy、bindings drift与workspacecheck/tsc/Biome通过，MCP cli_window_probe Rust无error诊断。CI新增该native入口，保留oldchannel/v2/GBK/WM。锁文件只多desktop tempfile/zeroize已锁包边，无newversions/CSP放宽；allfeatureaudit仍6license/2maintenance门禁失败。
- 限制：仅Linux/privatefixture nativeUDS，不是productioninterprocess/clap/Capability或跨用户/WindowsACL；当前重建WebView页面约330ms，完整显示与50ms目标仍需独立评估；未测试冷启动/single-instance/真实user窗口/IME/跨平台。主进程配置窗口与spawn同runtime并不等于productionCLI启动流程。

## S5真实进程启动竞争/存活端点保护/崩溃恢复（只读daemon）

- 新mise/xtask/CI probe-cli-startup，复用cli-probe/ownedUnixUDS/token/peerUID和只读ping，nativeGUI仍是独立检查。5个candidate加3个正常认证CLI和1个旧token拒绝CLI，共9个实际新进程。两个候选tokio并发launch，std1.95 File.try_lock原子竞选保证只有一个primary；stdin空/stdout仅PID+role/status，secret不在argv/env/report。
- 额外live-secondary持相同election.lock尝试失败，在退出前后socketinode/token字节完全不变。primary完成2授权ping后受控退出，锁及endpoint清理。随后真正SIGKILL primary，OS自动释放锁而磁盘留下socket+token；recovery新实例拿锁，只有root0700/sockettype+UID+0600/tokenfile+UID+0600/nlink1/64hex检查全部完成才unlink旧endpoint，不误删普通文件/symlink/hardlink。
- 恢复token真实轮换；父进程将旧token保管在另0600文件，由独立子进程实际提交hello，新服务认证拒（denials1），然后新token正常ping成功，证明坏认证只关闭该连接、不关闭恢复服务。server最多4个连接尝试、每个有限accept/serve deadline；临时listener/client/process/runtime均清理。未把uuid/token bytes写入报告。
- 3新增单测拒flags-only、unknownregular/symlinksocket被误删、unsafe/hardlinkedtoken触发前确保旧socket仍在。CLIfeature15xtask tests/strictclippy，真实startup+原CLI回归与workspacecheck/tsc/Biome通过，Astrolabe startup模块无error诊断。无newcrate/lock/version/audit策略变化，既有release6license/2maintenance仍阻断。
- 限制：只读daemon的冷启动election与IPC安全，不是Tauri single-instance插件、GUI未运行时拉起/--startup-actions转交、生产runtime路径/owner文件防TOCTOU、WindowsNamedPipeACL或真实跨用户威胁验收。文件检查依赖当前用户受信任0700目录，不能抵御同UID恶意进程；生产bootstrap仍须单独验收，不暗中改变已有Tauri插件架构。

## S18通用Direct/HTTP CONNECT/SOCKS5组合字节流基础

- 新独立pulse-route库，默认workspace及层级检查纳入。公共Destination/Credentials/Hop/Route/Dialer/NetworkDialer/BoxStream均已实现，返回protocolneutral TokioAsyncRead+AsyncWrite流；只trusted backend配置，不开放desktop/CLI/frontend endpoint。host/port安全验证、IPv6authority括号、ASCII/IDNAupstream要求、8hop深度/8KiBheader、≤30s总deadline，无每byte重置deadline/自动直连fallback。已锁base64.22/zeroize/Tokio边，无newthirdparty版本。
- HTTPBasic显式凭据/ownedscratch零化，HTTP全2xx成功/407auth/其它refused，status与field/control/obsfold严格检查。SOCKS noauth或RFC1929只offer对应method，server改noauth拒且不发credential；IPv4/IPv6/domainCONNECT与boundreply精确消费。header逐byte有界读不overreadapplication首bytes，SOCKS响应相同，UTF8解码不接触tunnelpayload。
- 实际probe-route 5场景：Direct、HTTP→SOCKS、SOCKS→HTTP各256KiB arbitrarybinary echo及8byte binaryprefix逐byte一致，writer半关闭后仍读完整剩余bytes并收到EOF，全部listeners/relay tasks已join。另错误HTTP认证时inner/targetaccept0；错误SOCKS认证时outer已forward1但targetaccept0。报告强制各mode/顺序/bytecounts/prefix/halfclose/acceptforward数值，不接受旗标替代。
- 12librarytests覆盖CRLF配置/secret/depth、同packetheader+binary/IPv4IPv6domainreplyprefix、真实TCP超时/取消未来drop后remoteEOF、DNSname原样透传代理、协议badstatus/header/2xx/authdowngrade/rep/reserved/emptydomain/truncatedEOF。2新增xtaskmetadata回归缺payload/双顺序复用/认证后冒出targetaccept/缺EOF与cleanup明确拒。真实probe+workspacecheck/fulltests/sharedprobe通过（93term/45app/33IPC/12route/10xtask/210TS），库及probeMCP无error诊断；CI注册probe-route跨平台运行但本机只有Linux证据。
- relay任务deadline须借用JoinHandle直到完成，不take后timeout丢后台任务；错误时EndpointDrop abort，成功明确awaitJoin。credential/error不写report；allfeatureaudit仍6license/2maintenance失败，sources/bans通过，无豁免。
- 未完成：真实企业代理/NTLM/HTTPSproxyTLS、SSH与该Dialer同条链协议复用（后续验证）、ProxyCommand/SSHJump hop、route pool/配置secretref/UI/productionCapability入口、三平台及网络慢消费性能。fixtureRFB样式prefix仅任意字节，不声称VNC/RDP互通或S18全通过，SSH共享连接HOL门禁仍在。

## S18同通用代理字节流接真实pinned SSH协议

- probe-ssh-jump复用owned HTTP→SOCKS双认证fixture（两个独立listen/relay），调用pulse-route NetworkDialer返回同publicBoxStream，再直接交russh connect_stream完成协议。proxy不知道SSH，不另造SSH特有代理实现或frontend入口；finaltarget仍新建隔离SSH服务，不访问外部网络/用户凭据。
- 第一次通过两个proxy对SSH目标提交错误expectedhostkey，Pin回调实际拒绝，connection失败且两relay正常join；target连接drained，不能因代理认证成功就信任SSH。第二次正确key与password后session传512任意binary逐byteexact，remoteEOF+Close都收到、clientdisconnect、两proxy/listener/tasks join和SSH连接drained成功。server应用bytes原4449追加512，共4961，原UTF8/GBK/actor/两个SSHhop拒绝矩阵继续通过。
- 1新增routed report negative regression拒缺keymismatch/错误hop或bytecounts/flags-only；33SSHfeaturetests/strictclippy以及真实组合jump通过。已有CIjump现在强制routedSsh，不增加新crate/lock/audit/CSP修改。
- 限制：已说明S18协议-neutral复用在受控SSH上成立，但不是企业HTTP/SOCKS服务器矩阵、SSHJumpHop池/ProxyCommand、proxy链端到端ACK背压或productionroute配置；原共享SSH连接HOL门禁保持。下一步将原有4MiB慢ACK样例显式经此代理链验证，不将512bytes正常echo当压力验收。

## S18真实HTTP→SOCKS→SSH→GBK actor的4MiB背压与三种视图释放

- 新mise/xtask/CI probe-route-ssh-pressure，保留原direct probe-ssh-pressure独立报告。复用同NetworkDialer BoxStream与HTTP/SOCKSowned双认证，每条ACK/detach/timeout压力SSH连接都经过该proxychain，而对照独立control连接仍直连。proxy不理解SSH/VT/GBK也不拥有无限缓存，Tokio copy_bidirectional有界buffer及TCP/SSHwindow保证流控，不改变锁定russh。
- actualwindow-aware远端producer固定4193280 sourcebytes（512×8190），归一化4892160 UTF8，trueactor/registry/v2/credit8220。暂停ACK稳定时actor/reader/remoteProduced三个计数不动，本轮remoteProduced155610<TOTAL，独立连接control响应、同连接control阻塞。恢复ACK后全部raw+UTF8逐byteexact/最终backend画面匹配/SSHEOF+Close/v2EOFACK成功；撤视图或3sACKtimeout两条路径只退binding，会话存活/远端与sameconnectioncontrol恢复，最后完整producer排空与actorEnded。每条disconnect后两proxy/listeners/tasks正常join。
- report强制请求proxyOrder同时匹配mainACK/detached/timedOut且proxyRelaysJoined=true，不能用direct成功报告替代某一proxyphase。1新mode错配回归、34SSHfeaturetests/strictclippy、原direct三路径回归/workspacecheck/tsc/Biome通过，MCP pressure模块无error诊断；新route三路径actualprobe通过。无依赖/lock/CSP/audit变动。
- 仍限制：proxy链加入未改善russh同连接HOL，不能准许生产SSH共享池/SFTP/转发复用；受控回环不能当企业代理/网络设备/三平台性能。用显式routeJSON区分多bufferTCP链也不代表复杂真实网络永不积压；没有生产SSH adapter/ProxyCommand/HTTPS TLS/二进制Zmodem预分类。

## S11可信活窗口之间的会话ownership事务（尚无迁移DOM验收）

- 新SessionRegistry::transfer(source,target,id)可信backendAPI，无frontend/CLI命令，不序列化WindowToken。source须authorized+open、target须open、session Running；samewindow是no-op，capture在途try-lock Busy，不等待/不改healthybinding。迁移仅换Entry owner并撤原binding，保留actor/sessionId/native工作，不发送input/ACK/resize/close。目标重新capture/attach才有新generation。
- compare/marker/repair/capture gate的Arc及历史时间沿迁移共享，不重置限频。capture准入及提交时除owner外还检验Entry Arc身份，A→B→A不会使旧Entry的迟到请求复活；跨迁移捕获已持gate则迁移Busy，旧monitor只按generation清理。原owner已native准入的write/resize不能回滚，接入adapter需先retire本地input/ACK/UI，再明确调用事务与准确重绑。
- 4新增actor-registry回归：有输出时迁移/原window随后close仍继续解析/目标newgeneration bootstrap+输入；旧bindinghandle迟到input不写driver，旧ACK/detach/foreign/closedtarget拒；sameowner/Busy无副作用；ABA/rate锁保留；EOF后拒迁移且原owner不变。最初3项及完整workspacecheck/fulltests（48app/93term/33IPC/12route/210TS）/真实PTY2MiB/v1五headless/v2两GBKheadless回归通过；第4项新增后targeted tests通过。生产Mutex不跨await，测试guard改词法scope通过strictclippy；MCP registry无error诊断。
- 真实native v2WebView、typedbindings drift、nativeclippy-Dwarnings与39nativeunit回归最终通过，但这些是既有窗口兼容验证，不是新跨窗迁移DOM证据。后续还需固定真实跨窗口Channel/DOM迁移、原视图retire/目标容器接收/尺寸协调/用户拖动确认和关闭后orphan授权恢复；目前不能标S11完整通过。不改变audit/第三方deps或恢复serializer门禁。

## S11单个真实PTY子进程跨可信owner迁移（非GUI窗口）

- probe-local-session新增transferred证据/schema5：只调用一次spawn_local启动既有portable interactive fixed Rust helper。源owner capture真实READY屏幕（19bytes），transfer到target并close源owner仍Running；新generation恢复与backend inspect逐cell完全一致，旧completion观察完仍check新binding成功。原ownerinput/ACK拒，新ownerboundinput中文在同helper实际read_line校验，最终PORTABLE_OK中/exit7，rawData连续19→63/EOF帧ACK/native read63及final state/digest/screen一致，所有native workers/directchildjoin。
- 2报告negative回归拒missing/false receipts、twochildspawn、无generation递增/错字节/native退出码/非整数；报告验证在落盘前强制执行。fullcheck/真实PTYprobe与4core迁移tests通过。不是runtime WebView窗口，更不证明跨窗口DOM拖动；窗口owner仅trusted registry token，未声称不存在的公共childPID查询API或process-tree验收。默认任务/Unix CI已有probe-local-session即覆盖新增fixed分支；不新建生产迁移RPC/用户shell。

## S11实际Tauri peer窗口→main窗口的PTY归属与Destroyed隔离

- 扩展原固定peer验证，不新增frontendtransfer命令，不信任JSowner。main与peer均实际WebviewWindowBuilder/runtime install mint token，foreigninput/ACK/repair拒后，将peer既有native SessionId从peer转main；source及target使用请求的v1/v2有序恢复，Rust TerminalProbe逐cell/cursor screen与backend inspect一致后Commit ACK，旧completion不撤销新binding。每次run只transfer既有peer child，不另spawn替代child。
- source.close后等runtime Destroyed revoke caller token AND get_webview_window消失（不能用因transfer已被拒的source state假称销毁）。main新owner的state仍Running；boundinput hidden产生真实child HIDDEN_SEED，无view时解析推进；target再次capture全屏精确，然后显式close/nativejoin/reap。主样例RawChannel/DOM不接peer数据，generation全registry单调跨两run。typed SessionEvidence/TS/总报告s.validate/xtask强制peerTransferChecked；native fixture false拒，xtask新负向拒两证据之一缺失/false/truthy值，旧报告不得冒充此scope。
- 实测Linux v2及默认v1各两headless+两DOM run（每个run都有实际peer迁移分支），两最终报告transfer receipts都true；v2 final gen10/30（新增peer capture造成分配递增，不是跨版本合流）。typed bindings漂移/严格nativeclippy/39nativeunit/fullcheck/fulltests（49app/93term/33IPC/12route/210TS）通过；MCP新probe无error诊断。newxtask报告negative在其后targeted通过。无deps/lock/CSP/audit changes。
- 首次v2运行未生成报告，进程已退出/no lingering children，但旧runner未保存native exitstatus/阶段，根因未知；不是已修复。新增bounded固定阶段diagnostic与runner错误的exitstatus context，无凭据/输出内容，保持原期限不延长；随后v2/v1复验通过但不宣称消除偶发故障。
- 仍未完成目标WebView RawChannel与DOM消费peer恢复、生产多窗口拖动/输入退役/尺寸协调、WebView reload及关闭后orphan授权恢复，S11整体不通过。此处backend消费wire验证只是runtime authorization/lifecycle加真实PTY延续，不能将它当crosswindow视觉/IME验收。

## S11迁移末次capture实际目标WebView Channel与DOM回调

- peer源码窗口的首轮capture仍由Rust消费，但source Destroyed后含HIDDEN_SEED的末次capture改由独立Channel<ArrayBuffer>送目标main WebView。主session/channel未Begin时临时创建独立xterm surface，用SharedUnicode17/query suppression/UTF8 stream与现有v1/v2Consumer；主通道和迁移通道各自queue/ACK/nonce/gen，禁止frame混流。目标DOM每次write真实callback后settle检查实际绘制rows再ACK，不把headless视为DOM。ready回执后计算visible digest，backend exactsameboundary compare Equal才消费一次确认票据；清理只自己的root，不误删随后主surface。
- 新typed probe_transfer_ack/confirm开发命令不收sessionId/owner/目的windowlabel，由current主run的runtime caller+nonce定位固定已迁移peer；permit最多一份。old/foreign/confirmed请求拒绝，guarddrop仅清理自己的generation且不关闭actor，新guard不被迟到olddrop清掉。确认还须backend已送Commit并收到该seq的ACK准入标记；这仍是consumer提交ACK而不是用户看见画面的通用安全证明。driver保持原protocol/ACKtimeout监督，不改credits。明确只接受预期Detached收尾，旧source撤销允许Detached/Invalidated(Replaced)竞态，AckTimeout/Protocol/Failed不能冒充成功迁移。
- mandatory peerDisplayChecked 与 peerTransferChecked分别记录真实targetdisplay/runtime归属。TSfinish必须一次readyconfirm且无query泄漏，reject live/repair/end/mixedcodec/额外Begin/Commit或cancel/confirm失败。native报告false拒，xtask两份报告都须严格bool true，null/string/数字皆拒。实际endpoint负向：wrongnonce transfer ACK、wronggen确认、将peer ACK送主session命令、重复确认均被拒而currentpeer仍正确完成。
- strictCommit门收紧之前的最终v1/v2 actual报告均completed=true、两headless+两DOM、两组transfer/display收据true；全workspacecheck/test（49app/93term/33IPC/12route/13xtask/215TS）/typedbindings/40native/strictclippy/基础probe通过。MCP新native模块无error诊断。新5项TS回归使用actual headless xterm中文屏幕及可控CommitACKpromise，确认不会先于callback ACK，取消/确认失败不当成功；1nativegate回归测试foreign/old/consumed及oldguard不伤replacement。
- 最新strictCommitseq/ACK前置与具体Detach收据收紧后的v2→v1真实复验最终通过（两版各两headless+两DOM的runtime/display收据均true），native40/typedbindings/strictclippy通过；gate回归还直接验证无Commit、无ACK、foreign、old以及consumed确认拒绝。原首次无report的未知风险仍保留，不声称修复。
- 本轮mise run audit仍明确FAILED（6license拒、paste RUSTSEC-2024-0436/proc-macro-error RUSTSEC-2024-0370两项unmaintained；bans/sources过），无新dependency/lock/allowlist/CSP变化，不得打包。后续生产多窗口drag/目标实际输入与尺寸协调、source端DOM退役、WebView reload/关闭后orphan权限恢复、跨平台/IME/用户按钮仍待验证；S11整体未通过。

## S11刷新前可信registry批量view退役（真实reload仍待接）

- 新SessionRegistry::retire_window_views(owner)只允许live window token，短metadata锁原子撤该owner全部bindings；返回实际retired view数（无binding也更换Entry身份）。actor/sessionId/windowowner保持，不sendinput/resize/close，不持锁await；该window关闭后Forbidden，其他window view不受影响。Entry::with_owner共享clone逻辑用于transfer及retire，保留capturegate/marker/repair/comparison时间，不因refresh重置额度。
- 批量retire不try-lock等旧capture完成，而是换Entry Arc身份使已准入oldcapture在提交处被current_entry复核拒绝；sharedcapture gate仍占住额度，所以new capture可明确Busy直到旧工作结束。这是旧Entry身份与绑定准入基础，不是JS navigation认证：runtime adapter还须先更换/废弃旧page nonce/epoch，防尚未准入的old请求在retire之后新开capture。已native准入的输入不能回滚；不添加frontend批量撤销RPC或隐式retry。
- 3新增regression：同owner两个liveview批量撤销/0重复count、旧input/ACK拒，foreign binding保持；无view仍解析output，newgen准确bootstrap，oldcompletion不影响new；持有旧capturegate时retire立刻成功/oldEntry身份复核拒/newcaptureBusy，共享marker/repair/comparison锁与时间保持；closedtoken不能退foreign。fullcheck/fulltest52app/93term/33IPC/12route/13xtask/215TS、真实PTY迁移/2MiBbackpressure与两GBK headless恢复通过，strictclippy/MCPregistry无error。
- 尚未接on_page_load Start/真实window.reload/旧IPC epoch，不能视为S11刷新验收。releaseaudit仍原6license+2unmaintained失败，未改依赖/lock/gate；下一步有限runtime reload样例应把native task与page命令future分离，以免页面消失误关PTY，并等待新page准确Begin/ACK。

## S11真实fixed peer WebView reload与同native actor延续（不是reload页面DOM恢复）

- 既有peer WebviewWindowBuilder的on_page_load记录最多两次Started→Finished，watch只存一份bounded Pages metadata；finished-before-start、重复Started/Finished或第三次导航永久invalid，不能靠重复回调制造fresh page。初次foreign-control脚本结束后，Rust准确capture peer会话，retire_window_views要求撤1个view、oldACK/input拒，然后调用实际WebviewWindow::reload（Tauri2.12.1 public SDK）。5s固定截止等待真实第二次页面加载完成。
- fresh page仍同runtime window token/registered native window，backendSessionState和同SessionHandle仍Running且完整等于刷新前；Rust v1/v2 recapture每cell/cursor与backend inspect exact，newgen>oldgen，oldreceipt必须Detached、晚完成不清newbinding，freshview显式detach只回Detached。随后同既有peer child继续跨窗transfer、sourceDestroyed和main独立Channel实际DOM恢复，child由main native command supervisor持有，peer pagehide不会drop native task。
- 注意这个peer JS一直只执行对main会话的foreign拒绝探针，从未获自己peer sessionId/input/ACK/attach权限；oldpage无capability可复用。这是runtime reload+actor/旧binding失效证据，不是已授权production页面的nonce/epoch隔离，也不是新peer页面的DOM/Channel渲染。本页recapture仍Rust验证器消费，之前已验的目标main DOM证据只能算后续迁移，不可冒充refresh DOM。
- 新mandatory peerReloadChecked加入typed SessionEvidence、TS与xtask两报告收据检查；native false拒、xtask缺/false/truthy值拒。2新增PageTracker负向tests，native42/strictclippy/bindingsdrift/tsc/Biome通过。实测v2和默认v1各两headless+两DOM run（各4真实reload），最终reports completed/peerReloadChecked及后续display receipts true；fullcheck/fulltests52app/93term/33IPC/12route/13xtask/215TS通过，MCP新probe无error诊断。
- 之前目标迁移和Entry retirement也补做explicit encoding-probe实际GBK Window兼容：两GBK DOM run/双坐标/reverse input/新runtime-display receipts均过、40nativefeature/strictclippy/绑定漂移过，普通desktop仍不启用GBK，未新增deps/lock/CSP/audit豁免。当前新增reload的GBK/外部WM兼容尚未重跑。下一步需新页面权限mint/revoke与其真实renderer消费，不开放用户shell/任意导航；S11总体仍未完整验收。首次未知无report风险不宣称修复，releaseaudit仍blocked。

## S11固定peer每页nonce与精确runtime窗口身份

- PageTracker保留最多2次导航和一份watch投影，nonce由backend uuid mint；刷新retire前显式revoke，Started同样清current、保留previous仅给negative probe，Finished才生成新值。注入只在可信builder Finished、local origin和runtime身份检查之后用当前document eval，UUID为规范ASCII；不提供前端nonce getter RPC，不记录nonce明文。当前PageLoaded但未auth/旧nonce未拒都不能标第二页ready。
- probe_session_peer现在需要run nonce+page nonce：Active只能由trustedbuilder一次绑定精确peer WindowToken，重复同token允许refresh，新/foreign/main token或wrongrun都在confirm/取oneshot前拒绝，不能将同label重建窗口当原peer。页面确认single-use。新document真实IPC脚本先尝试previousnonce并要求拒绝，再currentnonce确认；watch还强制previous拒绝标记+当前auth2，不能只看Finished2。旧nonce不复活、不消费newpage许可。
- 2新增nonce单测覆盖explicitrevoke/no-greeting-getter、duplicate/previous/unknown、已加载未认证；1新增实际actor+Active测试覆盖未绑定/foreign/main/newtoken/wrongrun不会耗valid nonce或oneshot，正确peer随后成功，bind不可覆盖。native45/strictclippy/typeddrift/tsc/Biome与真实v1/v2各两headless+两DOM（各4reload）通过；fullcheck/fulltests52app/93term/33IPC/12route/13xtask/215TS通过。
- 精确owner变更后的MCP window_sessions诊断超时，不算通过；此前将长shell与MCP放Promise.all导致fabric取消两条shell（status killed/无有效完成），未据此标通过。确认取消后顺序复验任务完整通过，source/controller注册和types以实际编译/tests为准，不安装语言服务器。首次无report未知风险依旧未声称修复。
- 本scope只授予fixed peer的只读foreign-control拒绝探针，不授予peer自身input/ACK/attach：真实刷新后的画面仍Rust validator，不是授权新页面自己的DOM恢复。下一步把同页面nonce及owner门用于fixed peer只读Channel，native task继续应用所有，旧页面回调不得关PTY。不改变deps/lock/CSP/audit/release门禁，不标S11完成。

## S11刷新新页面自己的只读DOM恢复（固定样例）

- 固定peer窗口改为实际visible 600×400，新doc通过已验的UUIDnonce与精确runtimepeerToken确认身份后，lazy初始化只读480×208 host；rootReact独立，probehost放body且finally恢复root.hidden/移除host与pagehide listener。同原child RESIZE_SEED中文/emoji非空画面，从backend固定peer_id（不接前端session选择）经独立v1/v2Raw Channel输送。普通桌面和生产shell权限未开放。
- probe_peer_view/ack/confirm/done四个typed端点均要求localcaller+run nonce+当前page2nonce+精确peer owner+backendsession owner，no input/resize/close。peer semaphore拒重复捕获（不替换当前view），render共用已验的ordered事务/3sACK/一帧/8212或8220credit、实际DOM write/settle后ACK；只有同CommitSeqACK与sameboundary摘要Equal才触发native单次确认。await后再次检查nonce，detach收据严格Detached，不把timeout当完成。原main迁移端点授权逻辑独立，sharehelper仅处理已授权的owner/id/gen。
- positive render完成只markCaptured，不提前markRendered：新页还必须完成首次ACK前错误nonce/错generation拒、confirm前错误nonce拒以及positive后重复confirm拒，view.finish和清理流程均无失败才调用probe_peer_done。nonce锁内captured/rendered验证避免revoke竞态；Done single-use，Root5sreload wait要求auth2+rendered。新增mandatory peerReloadDisplayChecked贯穿nativeDTO/TS检查/xtask报告严格bool拒绝，既有peerDisplayChecked仍只标后续目标mainDOM，不能混用。
- 首次新增sourceDOM→targetDOM紧邻触发Registry.compare_visible 500ms/session Busy：分类确认session.transfer_compare_busy，而不是改限频/重置owner额度。仅固定probe在source成功后按既有quota sleep500ms，再进行下一次目标compare；原3sACK/5sreload/40schilddeadline不变，不做隐式retry。这再次证明transfer保存comparisonquota。native错误日志只记录数字退出码/cancelled/resize，不记录终端、凭据或nonce。
- 最新v2与默认v1各两headless+两DOM run（每codec4个刷新sourceDOM及4个后续targetDOM）完整报告，实际oldnonce拒和newdocDone完成。首次默认v1末轮native_exit_failed未保留分类，补无敏感退出收据诊断后复验通过；尚未复现/定位，不声称已修复这项偶发风险。46native/strictclippy/typeddrift/7报告负向与fullcheck/fulltests52app/93term/33IPC/12route/13xtask/219TS、baseprobe通过，MCP新peer模块无error。
- 4新增TS是wrapper生命周期mock测试，不算绘制验收；覆盖scope参数/duplicate、pagehide取消在途native和latecompletion、rendererfailure、混版/7s截止清理且不能调用Done。真实绘制证据单独来自WebKitGTK实际probe。first page仍无自己的DOMChannel，live旧页面订阅跨导航、生产任意导航/IME/拖动/多平台、GBK及外部WM最新组合尚待验证；S11总体不标完成，deps/lock/CSP/audit/release门禁不变。

## S11刷新DOM链路的GBK/外部WM兼容与在途许可撤销

- 显式encoding-probe构建发现Active新增peer字段未初始化（默认desktop编译不覆盖此cfg分支），不能视为GBK新组合通过。改为Active::new统一无peer default：owner/nonce/id/visibility与peerNone/peerOwnerNone/peerIdNone/newPageTracker一致；普通固定run显式增加自己的tx/pages，encoding样例不继承peer页面权限。此前已通过的旧encoding版本不当作当前源码证据。
- 修正后实际probe-encoding-window完整GBK两DOM、双坐标与UTF8固定peer新docreadonly恢复通过；probe-window-external真实WM-only最小化/恢复与两headless+两DOM完整reports通过，freshsourceDOM/targetDOM均独立收据。feature测试最初46项，随后最终源码新增2项回归并补default/encoding-probe各48项tests、严格clippy、bindingsdrift/fullcheck通过，MCP peer模块无error。普通desktop仍未开放GBK入口。
- 将Active::peer_view同nonce/window/id门复用于四个readonly端点，compare异步返回后仍调用owned重新授权。2新增真实actor metadata回归：foreign/main/wrongrun/oldnonce拒，page2 admission后导航Started使pendingPageTrackerclone不能markCaptured/Done，registry退役后actor仍Running、正常close/join；no-peer默认run绝无page capability。这是受控授权yield回归，不冒充GUI中用户重复刷新操作。
- CI现有LinuxGBK实际probe后补显式encoding-probe单元测试与strictclippy，防止cfg分支今后只编译默认desktop而漏检查。真实GBK设备、firstpage liveChannel跨导航、生产ViewHost权限、连续任意导航与跨平台仍待验收；最新defaultcore219TS已过，无deps/lock/audit门禁变化。先前偶发native非零退出/无report继续记录风险，未新增豁免或扩大截止。

## S11共享恢复sender的无副作用准入

- 源freshpeer和目标main复用的sendlane现在以application单额度semaphore在attach/capture前try-acquire，metadata slot也在capture前检查；Busy返回不发送frame、不替换当前actorlease、不消费新generation。permit贯穿capture/CommitACK/摘要确认及detach完成，失败按RAII退本次view；capture成功即创建Guard，避免previousgeneration不合法等早期return遗留已创建view。不会close actor/session。
- 2新增native回归各覆盖v1/v2：occupy lane后render Busy且zero frames、oldbinding保持、释放后下一次准确attach generation仅+1；badprevious失败detach自己的lease、permit恢复1、actorRunning/metadataNone、nextgeneration2能正常attach/close/join。default和encoding-probe各50项native/strictclippy通过，实际v2两headless+两DOM sourcefreshDOM/targetDOM及7合成transport测量完整，typeddrift/check通过、MCPsharedhelper无error。前一轮v1/GBK/外部WM实际证据保持，当前新增sender顺序尚未重新跑这三种实际模式。
- 发送准入是元数据保护，不是复用SSH控制通道/提高native窗口吞吐或放宽恢复精度；所有deadline/credit/500ms比较限频与releaseaudit保持。下一步Started可信callback需自动retire已绑定peer窗口的view，以覆盖不经root显式retire的实际reload；生产任意导航仍未开放。

## S11实际导航Started驱动退旧binding（非调用方提前撤销）

- fixed builder每次PageLoadStarted先PageTracker.record撤当前nonce，再按后端当前run nonce与windows索引中的精确peerToken调用retire_window_views。首次Started未授权peer仅允许退0；第二次必须在Started2/Finished1阶段退1个旧lease，缺/重复/0或2count/错误/非Started回执标invalid，不允许仅加载新页/新页DOM成功冒充自动退役。mandatory peerNavigationRetired贯穿nativeDTO/TS/xtask严格bool检查。
- reload验证不再在root显式revoke或retire：capture的旧lease在调用window.reload前仍checkBinding成功，真实Started驱动撤nonce及view，Root等待新页认证+sourceDOM+Done+automaticretired，旧ACK/input拒/完成Detached与新capture精度单独核对，actor和窗口owner始终保留。用于撤销的runtime callback不要求新URL仍local（导航到远端也应撤旧许可），但授予新nonce/权限仍只允许可信本地Finished；no frontend revoke/RPC label。
- 2新增native回归：真实actorlease下oldrun、同label更换token不能退原binding；正确peer退1且重复退0、actorRunning/closejoin正常；PageTracker actualStarted阶段/count/失败严格负向。default/encoding-probe各52native、strictclippy/typeddrift/fullcheck/fulltests52app/93term/33IPC/12route/13xtask/219TS通过。actualv1/v2各两headless+两DOM run（各4真实Started自动退役及freshsourceDOM）完整JSON独立retired/display receipts true，原七组合成测量仍完整。
- 旧页自己的renderer先前仍未授权（它只有foreign-control拒绝probe），旧activelease来自Rust验证器；证明runtime Started对真actor订阅退役，不等于旧页正在绘制liveData时的完整恢复。下一步先补firstpage本身readonlyDOM许可，再真实验证其旧nonce对ownreadonly端点被拒。当前不处理生产任意导航/连续三次reload、跨平台或用户IME，既有偶发退出风险保留，GBK/外部WM最新Started版本尚未实测复跑；release门禁不改。

## S11首次页面曾获自己只读许可后的真实刷新撤权

- firstpage已用当前pageNonce+精确peer token完成自己的只读Raw Channel→DOM→ACK→摘要→Done，mandatory peerInitialDisplayChecked覆盖首次实际绘制。nativeSupervisor在peer注册和greeting后才主动eval首次renderer，不依赖首次Finished与peer_id注册的竞态。当前doc只允许单次view，无session选择、输入、resize/close权。
- readonly_ready仅认证完成的page1或page2，Started清currentcaptured/rendered（不复用旧DOM收据）但保留initialDisplay历史证据；freshdoc依然要求旧nonce拒/新nonce认证、真实新capture/新Done。旧页面确实曾通过ownreadonly gate；freshwrapper除unknownnonce/错generation/duplicate之外，明确用previousPageNonce调用自己ACK/confirm端点并要求拒绝。Root500ms节奏分别保留first→fresh与fresh→target共享比较额度，不增加deadline。
- 新native回归firstpage真实准入与currentcapture/render收据清零、oldnonce拒/新doc须重新capture后Done；新TSwrapper回归previousnonce确实调用ownACK/confirm负向，mock仅测试调用约束。nativeDTO/TS/xtask独立initialDisplay bool收据拒缺/false/truthy，不能把freshDisplay或targetDisplay当首次画面。
- latestv1/v2各两headless+两DOM run（三阶段各4次firstDOM/freshDOM/targetDOM）完整，actualStarted retired及initial/fresh receipts true；default/encoding-probe各53native、strictclippy/typeddrift/fullcheck/fulltests52app/93term/33IPC/12route/13xtask/220TS通过，MCP reload模块无error。
- firstpage是只读snapshot，完成后该Channel/surface被释放；reload瞬间Open lease仍由Rust validator持有，并非旧DOMliveData正在持续接收。下一步需要将firstpage订阅保留至navigation，让RuntimeStarted撤销真实renderer lease；不能提前宣称liveData跨reload完整验收。最新GBK/外部WM此firstpage扩展未重跑，生产/IME/跨平台和偶发非零退出风险仍保持，deps/CSP/audit不变。

## S11保留readonly renderer的前端续流与就绪握手（native live relay待接）

- TransferViewProbe增加显式validation-only live配置，bootstrap ACK/摘要后保留surface、Unicode provider、查询屏蔽与UTF8Stream，允许有界Data及纯Marker；snapshot默认仍拒Commit后的任意帧。Marker sink只等DOM绘制，不送VT、不finish pendingUTF8或flush同步更新；同seq ACK完成之后才调用带revision/source边界的marker摘要确认。取消/确认失败丢弃本代，不重用surface或清理新代。
- 6新增v1/v2实际headless回归：半emoji跨Data/Marker保持（含已写空格），暂停MarkerACK时不能确认，续字节完成🌍，DSR查询不会产生重复onData响应，lateData/marker确认失败不能被当成功；默认snapshot续流继续拒绝。6项是headless数据面证据，不是native live订阅跨reload证明，尚未启用第一页面live模式。
- 默认snapshot真实回归首次报peer_initial_display_deadline，未通过。实现审查确认Finished/greeting可能早于React effect安装__PULSE_PEER_VIEW__；新增单document resolver握手：Finished script先等已有hook或一个resolver，installPeerBootstrap先发布hook、清resolver再唤醒，无轮询、重试、nonce getter或deadline扩大。3单测直接读取/执行actualRust静态脚本，覆盖提前/迟到装hook、跨doc隔离和重复安装拒绝。复验过并不证明那次timeout的全部根因已定位。
- latestdefault v1/v2snapshot实际first/fresh/targetDOM及53native、fullcheck/typeddrift/tsc/Biome与229TS通过，CI原有actualprobe含新握手。前次firstpage构建GBK和外部WM实际组合也已过完整独立initial/fresh/retired报告；本次新的前端live/握手后这两模式未复验。无deps/lock/CSP/audit改变。
- 下一步把显式live接到native application-owned订阅监督：bootstrap确认、Data/Marker确认、Started撤销与oldviewjoin须分开；不能在窗口卸载时取消PTY。snapshot或headless通过不等于oldDOM仍在消费输出时的真实刷新验收。S11整体/生产/跨平台仍未完成，既有偶发退出风险保持。

## S11实际旧DOM持续订阅→Data/Marker确认→Started退役→新页恢复

- firstdoc用render_live保留自己的真实lease与DOM，不再以bootstrap结束后创建的Rust验证器lease代替旧renderer。Root仍持有同一个nativePTY task/actor；peer页面命令只持有view。bootstrap实际DOM行投影/write/fence完成后ACK/摘要Equal，peer confirm标captured、ready调用single-use Done，frontend不等native订阅结束才显示，而是保留surface/查询屏蔽/UTF8Stream直到pagehide或native完成。
- Root向本renderer generation发固定hidden输入，同一child输出HIDDEN_SEED中文/emoji。native受一帧/8212或8220credit转发Data，只有metadata本seqACK游标达到backend parsed边界才请求pure Marker；Marker必须已有Data本seqACK、自己的seqACK、sameboundary摘要Equal与本页nonce/owner，独立peerLiveDisplayChecked不可用initial/fresh/targetDisplay冒充。snapshot默认仍拒continuation。每次Data/Marker ACK前还检查实际DOM所有行text与buffer projection，不仅依赖模型摘要/RAF机会。
- metadata锁贯穿同步ACK准入与ACK状态发布：credit释放可能在其他runtime线程唤醒nextFrame，避免Marker误见lastDataAck=false。live Data credit允许后续ACK，不授予重复bootstrap/未ACK Marker确认；Marker单次sender独立重新arm。依旧保留原500ms/session比较quota，first→live、live→fresh、fresh→target仅样例显式按quota节奏安排。
- nonce锁内每页一次claim（两doc最多两在途请求）限制等待数量，Started重置当前claim/capture/render而保留旧实测收据。native live监督等actualStarted、registry退役后严格Detached receipt，先释放Guard/sender/peer permit再mark_navigation_joined；freshcapture有500ms固定join上限，await后重验nonce，不能抢旧lane或复活旧请求。Rootreload之前仍check旧renderer gen绑定Open；newdoc以自己的Raw Channel重捕获后继续可信跨窗迁移、输入与有序EOF。oldnonce/gen仍拒，view消失不close actor。
- 新3native回归覆盖claim per-epoch/重复与oldnonce、join真正完成前阻塞、live DataACK不等于Marker确认权限；新3TSwrapper回归native订阅未结束时已ready/Marker、pagehide只清oldsurface与latecompletion、native提前结束或后续失败不能伪成功。此前首次actualrun四次旧live/刷新/迁移已走通但main terminal.render_timeout，整组未通过；保留1sdeadline，只补安全frames/visible/connected分类，之后v2和默认v1完整复验通过，未声称根因已定位。
- 最新actualv1/v2各两headless+两DOM run（每codec4次oldDOM Data/Marker/Started/fresh/target），mandatory live/retired收据true、seven运输测量完整。default/encoding-probe各56native/strictclippy、fullcheck/fulltests52app/93term/33IPC/12route/13xtask/232TS、baseprobe/typeddrift通过，MCPshared native sender无error。随后actualGBK与外部WM此live版本均完整通过，独立live/retired/fresh收据true、typeddrift保持；next验证未ACK导航场景。
- 这份controlled sample在Data全部ACK且Marker完成后发起reload，不证明未ACK洪流/持续网络输出期间的bootstrap摘要及重绑策略，live relay只支持Data和single Marker，不把缺少Checkpoint/End支持计完整。firstdoc/currentnonce只读权限仍非production ViewHost，生产/跨平台/IME与S11整体未完成；所有deps/lock/CSP/1srender/原helper deadline/audit门禁不改。

## S11有限未ACK Data期间的真实导航与恢复

- 旧页已确认一次live Marker后，只允许当前run/page nonce及live generation设置ACK hold；真实hold_ready端点先检查runtime owner、固定session和binding。第二条固定hidden输入产生Data，本seq/end连续100ms保持未ACK才记peerPendingAckRetired。PeerAckHold在pagehide仅reject，绝不靠假ACK释放credit；旧DOM的预期取消不误伤新页，setter清理按函数身份保护新代。
- actualStarted撤旧lease，native严格Detached并释放sender/peer额度后join，新页不能抢占。后端无视图继续解析第二条有限输出；两条HIDDEN_SEED可见且SessionState连续20ms稳定后才允许freshcapture（1500ms固定上限）。必须比较排空后的expected而非误假定pending前状态不变。该排空门是有限样例监督，不解决连续网络输出时bootstrap/latest摘要策略，也不算洪流压力验收。
- 新3TS回归：hold只拒绝退出、不返回credit，新旧代独立，真实wrapper挂起ACK及错误nonce拒/oldcancel清理。原snapshot参数fixture改在native调用进行期间测试，禁止清理后再发ACK。新native回归验证live→hold→pending→Started→oldjoin→refresh许可顺序；DTO、生成types、TS与xtask严格bool收据均强制。
- 后续移窗样例原contains已有HIDDEN_SEED会提前判新输入完成；改为seed count增加且parsedBytes推进，不放宽成功条件。新增静态relay失败分类及old supervisor/driver receipt/permit/join阶段诊断，不记录nonce、终端或内部错误字符串。
- 多次整组失败（旧join、main relay/可见性查询）保持风险记录。期间工具ENOSPC：数据盘31GiB空闲，但共享/tmp 16GiB tmpfs已满，主要为其他项目；未删除其他项目文件，仅cargo clean核实的PulseTerm临时upstream target缓存约319MiB，保留src/tests/manifests/licenses与测量报告。后续使用0700项目target/probe-tmp.*运行时TMPDIR避共享压力，Sandbox/CSP不改。不能由空间恢复与复验成功推断所有先前失败根因已修复。
- 最新Openbox实际最小化整组及默认v1/v2整组各完成2headless+2DOM、pending/live/retired独立收据true，oldrelay→Detached→释放→join阶段齐全。default/encoding-probe各57native、strictclippy/fullcheck/fulltests52app/93term/33IPC/12route/13xtask/235TS/typeddrift通过，MCP reload模块无error。随后pending版本GBK与外部WM实际兼容均完整通过，独立pending/live/retired收据true，绑定漂移保持；所有原1srender/query、500msoldjoin、helper/ACK期限及deps/lock/audit门禁不变，S11整体仍未验收。

## S11捕获摘要与最新状态校验分离（受控输出推进）

- 最小双codec回归证明：Begin/Commit未ACK时actor从a推进为ab，captured VT仍精确还原a；原compare_visible必须Stale，而不是图像损坏。按offset1续流b后才匹配latest。不能靠冻结无限网络流或放宽latest检查解决bootstrap确认。
- SessionBootstrap新增digest，由session_stream::prepare在同一actor turn计算；AttachmentHandle仅保存boundary/digest和失效status的只读sidecar，不进v1/v2帧、JSON或source/display信用。SessionRegistry::compare_bootstrap在当前owner/gen/status下确认captured projection，格式与共享500ms/session额度保持（foreign/错gen/坏格式不耗额度）。Equal不代表queued Data已绘制，不清任何risk；latest compare/repair保持原语义。4新增app oracle回归覆盖授权/格式/限频共享/篡改/撤销、capture后已出现非零LINES/REP风险保持、换绑/移窗缓存不可跨代复活，以及恢复加载期间连续输出有界。
- 桌面固定端点仅在本Commit seq ACK与can_confirm通过后选择captured比较；Marker选择latest比较。新增双codec native分派回归证明capture后actor已推进时Commit可Equal，同旧边界Marker必拒。native bootstrap总3s内再等本页Done/initial_display，才转发queued Data：避免confirm RPC已在后端唤醒sender，而前端仍comitting时收到Data。新native屏障回归capture本身不能放行、Done后放行、导航旧nonce不能复活。
- 实测把原第一条hidden输出移到已捕获但Begin尚未发送时，等source parsed cursor确实超过captured边界后才发送Begin；不增加种子数量。firstdoc实际DOM还原旧图像并确认cached digest，本页Done后消费queued Data，随后Marker校验最新状态；原latest-only bootstrap实现会拒。新增peerBootstrapAdvanceChecked贯穿nativeDTO/generated TS/report严格bool检查，不用initial/live标志冒充。
- v2与默认v1/显式GBK/外部WM四种实际组合均完成2headless+2DOM，独立advanced/pending收据true，查询屏蔽、旧nonce/ACK、未ACK退役、fresh/targetDOM、EOF及运输测量保持。59native两feature/strictclippy、54app及原binary headless/GBK恢复、fullcheck/tests93term/33IPC/12route/13xtask/235TS/typeddrift已过；随后仅新增风险保持测试，最终app57与check/API隔离补验。MCP comparison/stream模块无error诊断。
- 新双codec慢bootstrap压力回归：真实有界Channel producer发送64×4096字节（共256KiB，中文/emoji）。Begin未ACK时parsed暂停且少于总量、producer未完成；在backend已推进时captured确认仍Equal。恢复ACK后每Data offset连续、256KiB逐字节一致，最终模型digest与actor相同。不算GUI/SSH洪流性能验收。
- 显式GBK headless探针追加缓存oracle：源计数/UTF8计数不同且半字符待解码时exactcaptured可Equal；source相同而UTF8游标错误必须Stale，共享500ms比较额度保持。bootstrapOracleChecked在TS报告消费者要求严格true，两份真实actor→v2→headless恢复仍通过。普通pulse-app与desktop未添加GBK生产feature或新依赖。
- 真实SSH压力追加“bootstrap未ACK且远端已经生产”阶段：先启动受SSH发送窗口约束的4,193,280字节GBK producer与读桥，不先完成bootstrap；700ms后采样再等待300ms，actor/source、bridge read和remote produced三计数必须完全不动且未达总量。此时cache Empty捕获确认仍Equal，随后bootstrap ACK及原Data暂停/恢复、逐字节UTF8、EOF、detach/timeout清理继续执行。
- 直连与HTTP CONNECT→SOCKS5→SSH实际报告均完成：bootstrap paused actor 57,330、bridge 106,478、remote 155,610字节，最终source4,193,280、credit≤8220；独立bootstrapWhileRemotePausedChecked与3个bounded counter为mandatory，缺值/false/truthy/0/总量负向拒绝。38项ssh-probe单测/strictclippy、两实际pressure probe、fullcheck/fullcore235TS通过。一次错误test filter得到0tests未计通过，已完整38项补验。
- 该网络结果仍是隔离回环/测试CA凭据/固定producer，不是任意网络设备或GUI持续洪流；同connection控制通道HOL仍必现，独立连接响应、各条route/relay/EOF/清理门保持。未添加依赖/allowlist豁免，未ACK导航检查点已独立签名提交 `e39c5fa`，捕获摘要/网络验证保留独立提交边界，未擅自无签名提交。
- 当前only fixed UTF8 peer开始前有限一条输出推进，不是连续洪流/跨平台生产验收；oracle为FNV可见投影而非安全认证或history/hidden/fullstate校验。上游5行getter、所有wire版本/信用/超时/CSP/依赖与发布审计门禁均未改。未ACK导航检查点已签名提交 `e39c5fa`；本节新代码/记录与它分开提交，签名要求不绕过。

## S11刷新后的只读页面持续输出与迁移交接

- fixed peer第二个document不再只用snapshot render；同readonly nonce/owner/fixed session权限下render_live保留renderer、UTF8流和查询屏蔽，等待本页Commit绘制/ACK/捕获摘要与single-use Done后才转发Data。前端keepLive同时支持初始/刷新页，刷新页不能调用仅用于initial场景的ACK hold。
- 固定Root在refresh后输入额外hidden，必须parsedBytes推进、seed count增加且本seq Data ACK达到当前边界，才请求pure Marker。fresh自己的Marker ACK/latest摘要才设置refreshed_live_display，旧initial live_display不能替代；随后Root显式detach，native确认同binding Detached、释放shared sender及peer额度后才mark_refreshed_joined，新capture/移窗必须等待此收据。
- 初始document仍由真实Started驱动退役；fresh不能因为started已为2误用旧navigation事件而立即退出。live监督器进入时固定document role，fresh在wire关闭时重新检验当前nonce，再要求真实Detached receipt；会话一直Running。等待Done/3s bootstrap、5s live、500ms freshjoin以及1s渲染/Marker/数据检查截止不变。
- peerReloadLiveChecked为独立mandatory bool贯穿RustDTO/generated TS/总报告/xtask两runtime检查，missing/false/truthy拒。新的native负向门证明fresh必须自己captured/Done/Marker才join，oldnonce与重复Marker/join拒；3项TS回归包含fresh保留DOM、旧nonce ACK拒绝/initial hold阻断、pagehide与迟到失败隔离、Marker失败不能冒充native退休成功。
- v2/default v1/显式GBK/外部WM真实组合均完成两headless+两DOM，fresh/pending/advanced独立收据true；60native两feature/strictlint、57app、238TS、fullcheck/fulltests/drift已通过，MCP reload模块无error。依然固定有限输出，并非任意第三次导航、洪流GUI、跨平台或生产ViewHost验收。
- 后续准入审查发现page claim早于peer/shared lane额度，Busy请求可能消耗恢复机会。已调整为等待oldjoin/refresh许可后，先授权nonce、取得两个额度，再消费single-use claim；lane贯穿捕获/发送/退役，不再次竞争。native回归覆盖initial与fresh坏nonce、peerbusy、sharedbusy后合法claim仍成功且permit全部释放，已获claim仍不允许重复。最新61encoding native/strictlint/check已过，最新双版本实际endpoint组合已通过两headless/两DOM与61default native/strictlint/drift；没有以此前GUI通过替代最新源码验收。
- 最新移除了fresh捕获前的“第二条输出排空且20ms状态稳定”等待：只等old relay Detached/join与页面授权，记录source下界不要求latest停住；fresh也在已捕获但Begin未发送时启动固定hidden输出，必须parsed游标推进后才标refreshed_bootstrap_advanced。fresh Marker成功还要求该独立内部收据，不能用initial bootstrap_advanced替代；nonce/claim/重复推进拒绝已纳入native回归。新页旧图像可见投影摘要确认后queued Data继续，再额外输入后以latest Marker确认并detach/join。
- 最新无稳态源码v2/default v1/显式GBK/外部WM四种实际组合均完成两headless+两DOM和fresh/pending独立收据；61native两feature、strictclippy、57app、238TS、fullcheck/fulltests/drift通过。仍固定有限输出与最多两页，不能称为无间断网络/GUI洪流或任意重复刷新验收；1srender/Marker、3sbootstrap/ACK、5slive及500msjoin截止未增加。

- 未ACK导航检查点已签名提交 `e39c5fa`；后续已验收变更保留独立签名提交边界。没有新增shell入口/输入权限、wire opcode、依赖或审计豁免。

## LOCAL-01/02 Unix自动Shell/Profile与启动字段基础

- 新公开`pulse-pty::profiles::{ShellKind,ShellCandidate,AutoProfile,AutoProfileCatalog,discover_unix_shells}`。Unix读取 supplied PATH/SHELL快照与常见目录的文件元数据，不执行候选或读取用户Shell配置。preferred已识别absolute shell优先，PATH顺序每kind选择第一份；目录/无执行位/相对及空PATH跳过，最多64组件/64KiB PATH/4KiB程序/5kind。执行位只是候选检查，不等于有效用户access/ACL/noexec判定或程序认证。
- 自动身份按kind稳定，refresh换路径保留hidden；缺失shell保持profile但unavailable，不准构造launch；remove记录deleted且重装、PATH变动及tombstone恢复后不自动重生。reconcile先验预算/duplicate，再原子改变metadata。此为in-memory backend模型，不是持久数据库/完整用户Profile/默认选择或窗口RPC。`sh`调用名保留，不能canonicalize到bash/dash改变Shell语义。
- `PtyCommand`新增pure validate，所有spawn在native分配前检查NUL、空env键/含equals的键、空cwd，最大128argv/128override/128KiB显式字段（含终止符）。保留空argv及最后env覆盖，Debug全部程序/参数/cwd/环境键值脱敏。继承环境、实际目录存在及OS argv上限仍由native层处理，底层native错误仍须adapter脱敏。
- 新9项profile回归覆盖private0700元数据fixture没有executed副作用、PATH/preferred优先、目录/执行位/relative过滤、symlink调用名、NUL/预算与隐藏/删除/恢复/atomic规则；4项command回归覆盖分配前拒绝、边界一字节、空参数/duplicate环境与无敏感Debug。pulse-pty共16项单测及严格clippy通过；xtask report新增missing/false/truthy/预算/exit/platform/schema负向门。
- 新`mise run probe-profiles`已注册xtask及Unix CI，不把登记当跨平台证据。实际Linux由/bin:/usr/bin元数据发现sh、以固定`exec "$@"`转发字面argv到自有Rust有限helper，核对TTY/cwd/env/中文emoji输出、exit7/EOF/reader join/direct-child reap，以及真实snapshot下hidden与deleted/tombstone恢复。候选发现本身不执行`--version`或其它命令，报告不含本机路径/环境值/输入内容。输出预算4096、helper3秒自退、worker12秒监督；固定脚本不接受任意CLI参数或远端输入，也不声称一般Shell树清理。
- 首次12profile/native单测与实际probe后严格lint发现let-chain/from_ref/print-newline问题，均修正且没有豁免；最新16PTYtests/strictlint已过，完整check/test/真实ProfileProbe/UnixPTY/portablePTY/actor/probe组合已通过（16PTY、57app、93term、33IPC、14xtask与238TS）；实际profiles.json所有独立收据true、shellCount=2、exit7/output23bytes。无manifest/lock/第三方依赖改变，audit仍原6拒license及paste/proc-macro-error两维护公告失败，bans/sources通过，发布禁令不变。MCP profiles模块Exact无error；首次引用缺path的Unknown未算通过，补path后Exact引用。
- LOCAL-01/02未全验收：Windows PowerShell/cmd/Git Bash/每发行版WSL、用户自定义命令参数/外观/默认profile、存储/UI、macOS登录及Windows注册表环境刷新均未实现或实测。锁定portable-pty只在默认program路径内部设置login argv0，显式profile未提供独立argv0覆盖；不能用`-bash`当executable或默认builder冒充自定义登录支持，本轮未patch依赖。

## LOCAL-01 WSL UTF-16LE列表纯解析基础

- 新公开`pulse-pty::wsl::{decode_wsl_list,WslDistribution,WslListError}`，接受可选LE BOM、LF/CRLF及空分隔行，不提供UTF8猜测回退或损坏surrogate替代。输出64KiB、最多64发行版、每名256 UTF16 units；坏BOM/奇数字节/无配对surrogate、NUL/control/仅whitespace及exactduplicate拒整份输入，错误不带stdout/name。名字保留case、空格、Unicode与出现顺序；不伪造registry GUID或作删除稳定键。
- `command`只将`--distribution`和完整名字作为两份OsString参数，不拼接shellsource、执行程序或加入其它选项，复用通用PtyCommand预算；Debug脱敏名字。option-like名字的纯argv回归并不证明wsl.exe的实际参数解析行为，这一点仍需Windows实测。
- 6项新回归覆盖BOM/Unicode/各发行版、损坏编码、全snapshot拒绝、字面选项/标点与Debug、边界及2000个seeded随机UTF16 snapshot。22项PTY、57app、238TS与完整check/test、LinuxProfileProbe、严格clippy通过；MCP模块Exact无error，未新增依赖/锁/审计豁免。没有运行Windows/WSL，CI只是原有三平台unit登记；native discovery必须先验证owned wsl process的成功退出、stdout预算与时限，纯解析不能认证数据来源。

## LOCAL-02 本地启动环境defaults与CLI路由元数据隔离

- 新公开`pulse-pty::launch::{prepare_local_command,LocalSessionEnvironment,PaneEnvironment,LocalLaunchError}`纯准备层；字面args/cwd保留，提供TERM/COLORTERM/TERM_PROGRAM/版本defaults，普通profile env及TERM/COLORTERM显式值优先。runtime身份4键的所有ASCII case别名覆盖/删除拒；backend version/id/socket限制格式/长度/NUL，错误与Debug不暴露值。context不是authorization token，不创建CLI服务或会话窗口绑定。
- PtyCommand新增env_remove，在继承环境/defaultTERM之后、显式env之前应用；清旧PULSE_PANE_ID/PULSE_SOCKET，只有backend当前pair才重新设置，不设置空字符串冒充删除。显式env覆盖/删除键合计最多128项、encoded byte均计128KiB。原通用PtyCommand不强制runtime政策；只有调用prepare的local-profile adapter走此行为，不能把它当生产IPC已受保护。
- Pwsh按trusted ShellKind前置-NoLogo（已有leading大小写同flag不重复）并默认更新检查Off，不解析/拆分script参数，profile普通env仍可明确改更新检查。此为纯argv/env回归，不是pwsh/PSReadLine/Windowsnative验收，更未实现自定义login argv0。
- 7项新环境回归覆盖defaults/字面argv/cwd/profile覆盖、无CLI字段、backendpair与Debug脱敏、reservedcasealias阻断、坏context、pwsh与预算、继承删除键校验/先删后设；29PTY/57app/238TS与完整check/test、strictclippy/真实ProfileProbe和actor2MiB授权/恢复兼容通过，MCP launch模块Exact无error。
- ProfileProbe schema2要求launchCount2、terminalDefaults/staleCliCleared/backendCliPair等独立strictbool收据；父worker通过process.env注入固定旧pair，不修改进程global环境，两个真实sh→finite Rust child分别断言旧pair不存在或被backendpair替换、全部终端字段与literal env/cwd/argv一致，exit7/EOF/readjoin/directreap均通过。字段值不入报告。新schema负向测试首次仍用schema2作为错误值失败，已修为旧schema1与单launch明确拒绝，没有放宽校验。
- 前一版本profile/WSL已补actualv2两headless+两DOM与61native/bindings/strictclippy兼容，不能当本轮env_remove的新GUI实测。最新本轮native编译/61tests/strictclippy已通过；无新依赖/锁/底层补丁或audit豁免。runtime metadata继承政策尚未接生产Profile服务/GUI/持久存储，Windows/macOS与一般进程树仍未验。

## LOCAL-02 Unix公开登录builder可行性实测

- 锁定portable-pty0.9的public default builder通过自身env SHELL选择程序，Unix as_command设置`-basename` argv0。新`mise run probe-login`在0700的owned project目录hardlink自有xtask finite binary，显式SHELL指向该文件，真实slave执行后helper断言精确`-pulseterm-login-probe`且argc1/TTY，exit7/EOF/readerjoin/direct-child reap全过。helper3秒自退、worker12秒监督、输出≤4096；不读取用户或系统shell rc，也不把该实验当startupfile/macOS/交互Shell验收。
- public args默认builder会panic，新增expectedpanic回归验证确实不能附带profile参数。missing-SHELL仅调用公开get_shell，观察选出的程序不同于requested path，不执行回退shell；report必须missingProgramFallbackObserved=true和customArgumentsSupported=false，避免success代表完整login。两项报告/公开API负向测试拒missing/truthy/wrong平台/非零argc/假定支持customargs。
- 先前“没有独立argv0覆盖API”的限制仍适用于完整显式program+args，但不能据此否认公开builder的无参数SHELL选择可行。生产完整选项：最小Unix argv0设置API补丁（保留显式program原路径/fail-closed，独立参数）、已知Shell登录参数（改变$0语义，generic POSIX shell需要单独支持矩阵），或暂缓完整登录。后续用户已选择 ADR-030 登录参数方案，不引入补丁；此段保留公开 API 实验背景，不以无参数例外降低 LOCAL-02 范围。
- 新xtask→已锁定portable-pty内部依赖边，Cargo.lock只多一dependency字符串，没有新包/版本。任务/CLI帮助/Unix CI已注册，Linux实际login.json selectedProgram/loginArgv0/exit/EOF/join收据齐全；完整check/test29PTY/57app/238TS、严格clippy、原ProfileProbe均过。最新两项xtask回归全量16项补验；MCP新模块诊断请求60秒超时，未计通过；未安装或重启语言服务器。全featuresaudit仍原6license/两维护公告失败，无新豁免。
- 本地环境prepare提交后actualv2 WebView也已通过两headless/两DOM、fresh恢复/输入resize/旧ACK/EOF与bindings漂移兼容；它不是生产ProfileGUI验收，Windows/macOS未实测。

## LOCAL-02 显式登录参数（ADR-030）

- 用户明确选择登录参数方案，不 patch portable-pty。公开 `pulse-pty::launch::prepare_local_login_command` 只为可信 Unix Bash/Zsh/Fish 分类请求登录；Bash 前置 `--login`，Zsh/Fish 前置 `-l`。已有 leading `-l`/`--login` 不重复，后续参数可能是脚本数据，不扫描、改写或拆分。选定程序/cwd 保留，复用 defaults、CLI 身份防伪和最终参数/环境/字节预算。generic sh、pwsh、未知类型和非 Unix 自动登录请求稳定拒绝。没有使用默认 builder 或程序选择回退。
- 首次35项PTY回归通过，但真实参数登录探针失败；复现 `bash -l --noprofile --norc` 的长选项顺序错误，改用 `--login` 后真实探针通过，输出断言、时限和启动文件隔离均未放宽。用户显式提供的无效选项顺序不自动重排；用户选项还可能改变登录状态，纯构造不作任意 profile 登录成功保证。
- 新 `mise run probe-login-arguments` 及 Unix CI 独立登记，报告 `login-arguments.json` 不得以原 argv0 实验代替。在0700 owned目录，实际 Bash 两次原生PTY分别断言普通非登录/参数登录状态、TTY、script `$0` 和中文/emoji/标点字面参数、终端环境、exit7、输出EOF、reader join、直接子进程回收；missing 显式程序真实spawn失败，未执行其他 shell。Bash 使用 `--noprofile --norc`，清除 BASH_ENV/ENV/SHELLOPTS/BASHOPTS 继承键，不读取用户启动文件；4096输出预算、worker12秒监督。此证据不等于系统/用户登录文件正确加载。
- 六项新纯回归覆盖三种known shell构造、leading选项复用、未知/非Unix拒绝、预算与身份防伪、非UTF8参数保留、坏命令先拒；报告负向回归拒绝 truthy/缺原生收据/argv0方法替代/错误模式和计数。Linux真实 Bash 已过，Zsh/Fish仅有纯构造证据；macOS、Windows及生产Profile默认选择/持久存储/UI尚未验收。`$0` 不保证 `-` 前缀，与 Terminal.app/iTerm2 不声称完全等价。完整 check/test、35PTY/57app/238TS/17xtask、严格clippy及原ProfileProbe、公开argv0实验、2MiB actor授权恢复兼容通过；MCP launch模块 Exact 无error。依赖/锁文件/审计规则未变。

## LOCAL-02 Profile平台默认与显式模式

- 新公开 `LocalShellMode::{PlatformDefault,Plain,Login}` 和 `prepare_local_profile_command`，平台默认仅在macOS为可信Bash/Zsh/Fish请求参数登录，其余分类和平台保持普通启动，不猜未知命令的能力。显式Login复用unsupported拒绝；Plain不添加登录参数，也不剥离用户已有flags。三种模式共用runtime身份与最终预算，不变化选定程序、cwd或字面参数，不创建会话授权或默认profile持久化。
- 原真实Sh ProfileProbe已改用统一profile入口。参数登录探针schema2要求三个原生launch（Plain/PlatformDefault/Login）、platformDefaultChecked严格true、platformDefaultLogin与当前OS一致；Linux实际参数计数6/6/7，Bash的非登录/默认非登录/显式登录状态、literal argv/终端环境/EOF/回收通过。旧schema1/两launch、缺default收据和truthy值拒绝，仍保持12秒监督与4096输出预算。
- 首次新增default负向测试暴露其收据未强制校验，已补strictbool和缺失/truthy/旧版本拒绝，不放宽测试。新增三项纯模式回归及完整38PTY/57app/17xtask/238TS、check/test、strictclippy、实际ProfileProbe/三launch/2MiB actor兼容通过；MCP launch模块Exact无error。macOS默认只取得policy纯构造/CI注册证据，不声称本机跨平台实测；Zsh/Fish实际启动、系统/用户启动文件与生产Profile/UI/存储尚待完成。依赖与发布审计规则不变。

## LOCAL-02 参数登录Profile到真实Actor与注册层

- `mise run probe-local-profile-session`及Unix CI注册：可信Bash+显式Login，`--noprofile --norc`和清除BASH_ENV/ENV/SHELLOPTS/BASHOPTS；固定bootstrap先断言参数登录和终端defaults，再仅exec自有finite Rust helper，绝不插值用户程序/脚本，不读取用户启动文件，不新增生产IPC。literal helper argv/cwd/env/TTY沿用原portable helper强断言。
- 抽取原 `wire_streaming_command` 正向注册路径，旧probe仍用原direct helper。两条实际路径均验证有序capture/data/end、正确owner/generation的输入及ACK、单帧信用9000内、暂停ACK解析不推进、恢复后EOF在ACK后完成、原生exit0和join。追加Unix应用固定X burst精确2097152计数，缺失或重复一字节也失败；ConPTY可能经console重绘，Windows仅记录观测值不扩大未实测断言，Unix profile收据要求burstExactChecked严格true。
- Linux profile实际source解析2097221（含控制序列和TTY echo），credit peak8212/limit9000，固定burst2097152，Rust重放最终PORTABLE_STREAM_DONE。它不是目标WebView DOM、真实用户启动文件或持续生产shell验收；仅直接子进程不作进程树保证。
- 原Profile/旧actor兼容、新native 2MiB路径及完整check/test/probe/bindings-check通过；38PTY/57app/18xtask/238TS/61native、strictclippy和MCP新probe模块Exact无error。新报告负向测试拒绝truthy/缺native收据、错误renderer、超信用和burst差1字节。审计实际exit5：advisories与licenses仍FAILED，bans/sources通过，仍为RUSTSEC-2024-0436/0370；未添加豁免或修改依赖。

## LOCAL-03 固定参数登录Profile到真实WebView

- 新增显式 `login-probe` 构建特性、`mise run probe-login-window` 和 Linux CI 入口；普通desktop不启用，任务要求Unix且已知Bash。只接受当前exe与单一`--session-probe-child`字面参数，无cwd/env自定义；拒绝额外参数、任意程序及环境覆盖/删除。工厂使用统一profile准备接口，禁用启动文件并清除BASH_ENV/ENV/SHELLOPTS/BASHOPTS，不提供用户shell IPC。
- 固定bootstrap检查`shopt login_shell`与TERM_PROGRAM后才exec helper，父进程继承的测试标记被覆盖为0，bootstrap检查成功后写1；helper检查精确标记再输出。该标记是验证收据，不是授权凭据。现有nonce/window/generation与ACK授权保持不变；报告新增`profileLoginChecked`，native验证要求与实际特性一致，xtask要求headless/DOM两份收据均与请求模式相同。生成DTO保持Raw Channel<ArrayBuffer>与字符串大整数，二进制帧不变。
- 首次编译暴露原生适配使用了不存在的builder API，已按当前字段式PtyCommand/LocalSessionEnvironment修正；strictclippy拒绝测试unwrap后改用Result传播，未添加allow或放宽断言。三项固定工厂/精确收据回归、普通/登录报告错配、缺失/数字/字符串/真假混合否定通过，login特性64项native/strictclippy、绑定漂移与MCP新模块Exact无诊断通过。
- Linux真实login-window报告已完成两轮headless/两轮DOM、刷新页续流/未ACK退役、目标迁移、输入/resize/修复/有序EOF以及七组传输测量，实际退出码0；单测不替代该结果。完整check/test/probe、普通v1/v2真实窗口兼容与绑定漂移通过；38PTY/57app/19xtask/238TS、default/encoding各61native通过，三份实际报告的login字段严格true/false/false且版本2/2/1。最终check额外发现否定fixture的unwrap，已改为无unwrap的空对象fixture后全链通过，不放宽lint。Zsh/Fish实际窗口、用户/系统启动文件、macOS/ConPTY、生产Profile/UI/存储及进程树清理未验收。依赖补丁、审计及发布门禁未改。

## LOCAL-04 当前自动Profile目录到统一启动准备

- 新增公开`pulse-pty::profiles::{AutoProfileLaunch, ProfileLaunchError}`与`AutoProfileCatalog::prepare_launch`；options无程序或shell分类字段，只携带字面argv/cwd/env/remove/mode。目录当前记录给出程序与分类，再调用已有统一环境/模式/预算策略。隐藏可显式准备；删除/缺失/失效拒绝，不从PATH或默认程序回退。该API不授予启动权限，不执行候选文件，不持久化配置，也不消除metadata到spawn的竞态；环境与cwd可敏感，Debug只暴露数量/是否设置。
- 五项新增回归：字面值及覆盖顺序/继承地址删除；隐藏、失效、删除再刷新和缺失fail-closed；目录路径更新及Bash/Sh登录分类；reserved identity/空cwd/NUL/参数额度/无效context；Debug与错误隐私。首轮测试函数名遮蔽和DoubleEndedIterator lint已修正，未增加allow或放宽断言。
- 真实`probe-local-profile-session`已改由目录直接准备options，不再由adapter构造shell程序或传分类；2MiB暂停ACK背压、精确续流、注册层输入与有序EOF通过。完整check/test、43PTY/57app/19xtask/238TS，strictlint和profile/login-arguments真实探针通过，新模块MCP Exact无诊断。当前消费者仍是Rust，不用新目录probe替代先前独立login-window DOM验收；生产Profile/UI/持久化、跨平台启动文件和任意shell入口仍未开放。无依赖或审计策略改变。

## SHELL-03 Linux Bash 配置与提示钩子隔离验证

- 新增`mise run probe-shell-startup`/Linux CI注册及固定xtask脚本fixture，不注入普通Profile。两次真实交互Bash5通过当前目录统一准备，以临时0700 HOME、--noprofile和--rcfile加载自有用户配置后加载集成，源码路径走环境字面值，不拼接用户路径为shell代码。系统bashrc可能仍由平台Bash加载，不能据此声称系统配置隔离或真实用户配置兼容；非Linux命令明确拒绝，纯收据tests保持可编译。
- 用户配置提供字面中文env/PS1/PS0，以及标量和数组PROMPT_COMMAND；fixture在最前捕获退出码并返回原值，原hook仍执行；PS1围绕原prompt输出OSC133 A/B，PS0保留原precommand并输出C，下一prompt输出D与状态。wrapper重复source集成仍只有一个前钩子，用户配置只加载一次。实际输入false/true产生精确D;1/D;0，原hook看到同状态，最终交互标志、env、hook数量及native exit7均验证。输出≤16KiB、stage读3秒/退出1秒/worker25秒，EOF、reader join、直接child回收通过；日志/JSON不包含输出或env值。
- 首次编译不支持所用Result便利方法，已换为现有is_ok_and，不升级工具链。新增严格报告否定：缺失/数字/字符串/false、模式/平台/单次launch/错误exit，以及realUserConfigRead/loginShell/productionIntegration三项scope必须精确false。完整check/test/probe、43PTY/57app/20xtask/238TS和原2MiB目录登录Actor通过，strictlint/MCP Exact无诊断通过。固定script不是生产hook实现，Bash<5、login rc顺序/实际用户脚本/readonly hooks、多shell、actor/IPC Shell事件和DOM标记还未验收；任何远端OSC均不可信，不授予控制权限。依赖与发布门禁未改。

## SHELL-03 有界 OSC 133 后端提示（低层）

- 新增`pulse_term::shell_markers::{ShellMarker, ShellMarkerBatch, MAX_SHELL_MARKERS}`与`TerminalProbe::take_shell_markers`。已有ground scanner开放只读callback复用64KiB unfinished OSC，不增加std vte Parser的无界Vec，Alacritty仍解析原字节。修正scanner在OSC内保留DEL以匹配vte（此前粗边界扫描忽略DEL）；其他C0仍按既有语义处理。只识别exact133;A/B/C/D以及D;0..255，允许缺省未知状态；只认BEL/完整ESC-backslash，CAN/SUB、单ESC及其他结束形式保守拒绝提示。无命令文本、cwd、权限字段或内容执行。
- 队列最多128条，超额丢弃新提示并累积饱和u32 dropped，drain不丢跨片段ST候选。6项新增回归覆盖每个chunk切分/BEL/ST/可选状态、取消/其他OSC/畸形或secret字段、C0/DEL、64KiB超长与队列上限、屏幕/DSR回复/nonzero风险不变，以及对real vte Perform回调的随机组合差分。首次测试错误符号与collapsible-if lint均修正，未增加allow。提示仅是输入流side-channel，无行坐标/parsed边界/完整command log，不随snapshot恢复，不清除risk；同步刷新时可早于grid应用。actor/注册层/IPC/DOM消费还未接入，不将prompt当可信证明。
- `probe-shell-startup`升级schema2，actual每个read片段增量送入TerminalProbe，两次真实交互Bash得到A/B/C/D;1/D;0且dropped=0；EOF不伪造native exit7的D标记，exit7单独验收。backendHintsChecked必须exacttrue；原schema1/raw-only报告拒绝。
- 完整check/test、99term/43PTY/57app/20xtask/238TS、realBash/2MiB目录Profile、五份v1实际actor恢复和两份GBK v2恢复、source drift与真实v2窗口/漂移均通过。标准snapshot600随机流条件matrix311/311；token matrix仍10类初始差异/18个base案例、11可修与7显式不可表示，输出含非法序列的xterm诊断，不视为无条件全部匹配。额外`PULSE_RESTORE_CASES=30000 cargo test -p pulse-term --features validation --locked restore_roundtrip_and_continuation_property -- --nocapture`通过，延期/近似类别不放宽。新模块strictlint/MCP Exact无诊断通过；上游parser补丁、二进制协议、授权、依赖和audit豁免均未改。

## Shell提示的actor与注册层隔离验证

- 新增`SessionHandle::diagnostic_shell_markers`、`AttachmentHandle::take_shell_markers`与`SessionRegistry::take_shell_markers`，不改帧协议或生成命令。actor同turn验证lease/cancel/receiver，最多128枚举提示及显式drop数；reply关闭时恢复原顺序/drop数。采样不写VT、不ACK、不清非零REP/LINES风险；`sampled_at`不代表每个提示的输出/行坐标或DOM消费。
- 注册层校验当前活窗口owner/generation，单session独立500ms准入、250ms响应截止，quota跨换绑/移窗保留；错误owner/stale不扣当前quota，返回后重新授权。成功采样后被撤销的迟到批次允许丢失，非exactly-once日志；与恢复/比较/Marker额度、协议及会话生命期隔离。
- 6项actor回归涵盖跨ST、旧lease、取消/放弃、满输出队列、溢出、reply失败还原与非零风险保持；3项注册回归涵盖外窗/stale、换绑迟到、取消保留提示、迁移限频与detach。真实Bash固定bootstrap输出A/B/C后exec原helper，schema2报告强制3条；原direct helper必须0条，两条2MiB ACK背压/输入/EOF精确排空均通过。该固定样例不是生产脚本注入或命令日志。
- 完整check/test、105term/60app/43PTY/20xtask/238TS、默认61native及strictlint、真实v2 WebView与binding drift通过；MCP对注册新模块Exact无诊断。marker尚未经IPC/DOM消费，也不能推导执行权限/实际子进程退出码。依赖、源码补丁、audit allow/ignore不变。

## Shell提示的EOF监督收据

- 新增`SessionReceipt`与`LocalSessionReceipt`、两层`join_with_receipt`，保持旧`join`和`join_with_screen`签名/监督关闭语义。收据保留最终actor状态、可见投影和最多128剩余提示/drop数；原生收据仍独立持有真实exit/cancel/read/write/resize。live已drain提示不重复，EOF/显式关闭不会推导或伪造D；不完整ST标记不因EOF补全。
- 2项新增actor回归验证EOF剩余D、不完整C、live读取Closed、显式close剩余128/9drop与不造D；关闭测试保持driver活着直到join，避免关闭和真实EOF同时投递造成错误测试断言。真实Bash profile样例改为等待固定helper后发送刻意D7再保留实际helper退出码0：schema3强制首批3、终批1、提示状态7与原生0分开；普通直接helper始终0提示。两条原2MiB背压、精确字节、EOF ACK与native join通过，不扩大生产Shell入口。
- 完整check/test/probe、107term/60app/43PTY/20xtask/238TS、五份v1与两份GBK v2恢复、真实Bash低层、v2 DOM窗口/绑定漂移、默认61native/strictlint通过。MCP对应用监督模块Exact无诊断。二进制协议和依赖门禁不变，尚未提供持久命令日志、每提示坐标或IPC/DOM消费。

## 固定页面Shell提示的只读IPC

- 新增`probe_peer_shell_hints`与生成`ShellHintReceipt`：限定localcaller、当前run、精确peer WindowToken、当前页面nonce、当前binding generation；页面必须已完成捕获确认且尚未Done。重复/旧页面在registry采样前拒绝，返回后重新检查owner/generation/nonce并单次标记提示回执，不新增输入/resize/close或任意会话选择权限。
- DTO最多128枚举提示及drop数；revision/sourceBytes/utf8Bytes为保留64位精度的十进制字符串，sampled-at不代表逐提示坐标或已经绘制的边界。固定登录bootstrap输出A/B/C，首次页必须收到三提示；普通模式与刷新页必须空。page Done必须已有提示回执；`peerShellHintsChecked`取实际首次及刷新页状态并贯穿native验证、JS、xtask严格bool门，不以root单次完成flag代替。
- 2项native页面/DTO回归及4项TS负向回归覆盖跨导航撤销、重复、未确认页、无内容枚举、状态/坐标非法和模式错配；实际IPC另外检查旧nonce、错误generation与重复采样拒绝。二进制帧、ACK、风险清理、比较额度、原生退出码语义不变。
- 完整check/test通过：107term/60app/43PTY/20xtask/242TS；default与encoding-probe各63native、login-probe66native及strictlint/绑定漂移通过。五种真实Linux模式(login/defaultv1/UTF8v2/GBK/externalWM)各2headless+2DOM及7测量完成，实际JSON提示回执均为true，只有login模式的profileLoginChecked为true。MCP对新端点Exact无诊断。
- 外部mode首次窗口匹配不唯一并超时：当时未保存XID集合，具体根因未证实。控制器现收紧为自有子进程PID+固定main标题，跨两个操作周期保持同一XID；拒绝歧义/缺窗/外PID/替换，不选第一匹配，不放宽2s操作截止。新增`mise run probe-window-controller`的5项确定性隔离fixture及Linux CI，不能当真实WM验收。
- 后续复验出现既有helper exit2（源码明确40s watchdog），耗时原因尚未定位；补无内容peer完成/EOF阶段计时，保留40s截止和exit0断言。再隔离复验完整通过、进程清理正常，但不宣称该截止风险已修复。生产脚本注入、持续命令日志、命令坐标DOM渲染、真实用户配置及跨平台仍未完成；依赖审计门禁未变。

## 自动Profile的typed SQLite app_state

- 新建`pulse-store`，按既定rusqlite/rusqlite_migration方案固定0.40.2/2.6.0，native bundled/backup、关闭默认Wasm特性；worker独占Connection，16待处理请求try_send准入，不在UI/async执行SQLite。WAL/FK/FULL实际验证；当前读写均串行，读池、Vault、密钥和应用目录平台适配仍未实现。
- 非秘密`AutomaticShell`偏好仅Bash/Zsh/Fish/Sh/Pwsh的hidden/deleted；严格版本1、4096字节行预算、未知字段/身份、重复和冲突拒绝，读坏记录不回退默认，保存事务不覆盖坏行。SQL固定且parameterized，无任意SQL、路径、程序、命令、环境或凭据载荷接口。
- 原10项store测试覆盖真实文件重开、worker clone/drop/join、16额度/Closed、取消执行前不写、坏/过大行、未来schema/无版本既有表保持原数据、私有权限/symlink、迁移前SQLite Backup保存未checkpoint WAL、existing backup不覆盖、失败迁移rollback并保留原schema及backup。新增9项通过：同worker及独立worker原子编辑合并、CAS旧快照拒绝/新快照写入、显式恢复deleted、坏行/trigger失败保持原值、真实SQLite锁竞争不改行、idle/fullqueue cancellation/drop owner、保留client后关闭、已提交写入lost reply不回滚；合计19项。升级测试为内部版本1→2fixture，不等于生产已有schema2；当前production schema仍1。此前tempdir默认权限导致Location失败，已只修fixture到0700，不降低门禁。
- app增加2项固定枚举映射/目录恢复回归；PTY保存独立hidden身份集合，重启初次缺shell而后再发现仍hidden。`mise run probe-profile-store`新版schema2报告在5个独立子进程真实保存、并发2editor、旧快照Conflict和重开，5s每child截止、无重试，最终join关闭worker，确认3个hidden与1个deleted全部保留；新增2项严格PID/模式/worker join/字面true负向回归并纳入同一mise task/CI，旧两进程报告不能替代；metadata candidate不执行，不计作Shell或真实用户配置验收。task/usage/跨平台CI注册，跨平台CI尚无执行证据。
- 初始检查点最终check/test/probe、实际Unix发现与PTY启动、2MiB登录Profile→actor ACK/EOF、bindings-check、63native tests/strictlint及实际v2两headless/twoDOM/7测量通过：10store/62app/43PTY/107term/242TS。本轮19store/strictlint、真实5process及2strictreceipt回归、完整check/test/probe、2MiB登录Profile→actor、bindings-check、63native strictlint/tests及v2 twoheadless/twoDOM/7测量均通过；62app/43PTY/107term/242TS保持通过。MCP store诊断Exact无错误，引用查询出现stale未当精确影响结论；实际调用者经有界读取/全编译检查核验。tokio-util沿用已锁定0.7.19，只新增直接引用与rt feature，没有升级包。
- Unix后续补齐有效UID、精确模式、单链接及预存侧文件检查（见下）；仍不宣称完整目录链/ACL/TOCTOU安全；WindowsACL/备份恢复UI/持久化Profile权限IPC/读池性能/Vault仍待实施。新6个库未新增cargo-deny拒绝或公告，原license6/maintenance2仍失败，不加白名单/ignore，详见dependency-review。

## SQLite进程强制终止与WAL恢复

- `mise run probe-store-crash`新增2个父用例，每个只执行自有integration-test子进程，在私有临时库、固定typed偏好上操作。直接SQL仅存在测试fixture，不扩大store或生产IPC权限。ignored子用例是被父用例明确调用并终止的helper，不是跳过的产品验收；正常workspace test也会执行两个父用例。
- 固定phase+实际child PID的完整行收据，5s就绪截止、32行/每行1024字节读取预算；已COMMIT用例还要求live WAL文件>32字节。父进程实际kill/wait：Linux SIGKILL=9；Windows代码路径要求Child::kill的退出码1，尚无Windows/macOS实测证据。12s helper watchdog退出不能冒充强制终止，RAII在失败时也kill/wait并join读取线程。
- 未COMMIT时终止后，重开保留此前Bash hidden和Zsh deleted；已COMMIT且两条Connection仍存活时终止后，重开恢复WAL中的Fish hidden，保留完整logical状态。首次超时来自libtest的同行名称前缀，改helper独立换行分帧后两个精确握手通过，不放宽字符串匹配或截止。
- 2个父用例、19项store unit、完整check/test、strictlint和5process偏好回归通过，MCP测试文件诊断Exact无错误。上个检查点原生v2DOM/63native回归已通过，本次仅新增测试/任务/文档与Unix限定import，未重复未变的原生测量。
- 不模拟物理断电、损坏磁盘/SQLite、用户已有库或备份恢复UI。schema仍1、frame协议不变。tokio-util rt直接引用后重跑all-feature audit仍仅原6项license拒绝与2项维护公告，bans/sources通过；audit不是通过，未新增ignore或放宽许可。

## Unix私有存储身份与侧文件边界

- DB打开前校验叶级parent的有效UID与权限700；主DB及预存-wal/-shm/-journal要求权限600、同有效UID、regular且nlink1，拒绝hardlink/symlink/目录、宽权限及外来owner。Owner只经safe Rustix geteuid读取，无unsafe或权限提升。caller仍必须提供可信目录，不验证或锚定完整祖先链，metadata检查与SQLite open之间TOCTOU尚未消除；WindowsACL/所有权/硬链接保证仍未验收。
- 22项普通store unit通过，新增硬链接保留两名原数据、12种sidecar别名/类型/权限组合不修改主DB或目标、只读父目录500/000及文件400保持权限与内容，Root也不得绕过只读意图。fixture-only模式恢复用于非Root测试清理，production不chmod/chown。
- `mise run probe-store-owner`在本机现有有效UID0显式执行1项真实chown负向测试，只改变专用临时目录/DB/3种侧文件的owner，证明拒绝且无自动修复；没有提权或触及用户文件。此用例在普通CI ignored并明确要求额外授权环境，不作为Windows/macOS实测证据。Node入口拒绝非Linux/macOS、非数值UID0与0tests假成功，必须有精确testname、1passed/0failed/0ignored、正常exit0收据；3项纯gate/收据否定测试纳入普通CI，不需Root。
- 完整check/test/probe、22普通store+1专用Root、2真实进程终止、5独立进程并发/重开、2MiB PTY→actor、bindings-check、default63native strictlint/tests均通过；62app/43PTY/107term/242TS保持通过，MCP database诊断Exact无错误。Rustix沿用已锁定1.1.5，只增加target依赖与测试fs feature，无版本升级；audit仍原license6/maintenance2失败且无豁免。
- 既有数据不会为满足这些条件自动修复；存储平台目录适配器、完整所有权链/ACL/TOCTOU保证、生产Profile IPC与Vault仍未实现，schema仍1。

## 设置分层与内存Vault密码学基础

- SettingsResolver 8项回归通过：default→user→group→host逐字段优先级与来源、显式false/同默认值、不可信字体CSS语法/Unicode名字、未知/重复/未来schema、文档预算和错误不泄露输入。schema由实际Rust类型生成，任务与CI注册，缺失/字节漂移/范围篡改负向均拒绝；不读取或写入用户settings文件，不计热加载或设置UI。
- AEAD 8项回归通过，含公开draft-irtf-cfrg-xchacha附录A.1已知ciphertext/tag答案、Unicode/NUL/空值、独立随机nonce、错误key/id/kind、密文/nonce/tag篡改与截断、64KiB双边预算、幂等lock与zeroize早清。固定key/nonce仅用于公开向量测试；正常seal只用可失败OS CSPRNG。额外3个compile_fail证明密钥/明文不能Debug、明文不能Serialize，严格lint与MCP两模块Exact无诊断。
- 本轮store最小实测38常规unit/1额外Root用例ignored、2真实崩溃回归与3compile_fail通过；最终完整check/test、schema只读漂移、45项SSH feature、5独立进程、2MiB actor、bindings与default63native严格检查均通过，62app/43PTY/107term/242TS通过；实际v2二轮headless/DOM与七组测量报告通过。无实际钥匙串/密码读取、主密码、KEK包装、秘密落库、完整应用锁或前端凭据IPC；schema与binary帧格式不变。
- 最终原生日志在EOF阶段后出现XIO fatal IO诊断，随后验证器接受完整报告且任务退出0；输出未标注来源PID，未定位、未宣称已修复，保留风险。不把该诊断本身作为通过证据。
- 下载先出现Cargo/官方HTTPS超时，同一地址复验HTTP200后Cargo正常校验归档并成功构建；早期超时原因未定位，不以成功复验认定修复。全特性audit仍原6项许可证与2项维护公告失败，bans/sources通过，未添加ignore。

## 配置文本的精确标量编辑

- 纯`set_terminal_setting`只接受font_size/font_family/cursor_blink类型化值，未提供任意key/路径/原始TOML片段。先验证原文和新值，不重置坏记录，生成后验证整个模型与预期相等且仍≤64KiB；不产生其他默认项、不写文件。
- 8回归通过，覆盖完整原文注释/排序、inline/dotted键、Unicode/转义token、显式false、新值稀疏、非法值/未知/未来schema及输入/结果预算。首轮发现toml_edit整体串行化会统一重复dotted parent的空格，改用不可变Document的真实值span只替换目标token，保留原失败断言；dotted新增值追加根级赋值保留旧字节。
- 直接引用已锁定toml_edit 0.25.15（parse/display），未升级版本；本轮46项普通store、2实际崩溃、3compile_fail及完整check/test、CLI strictlint、五独立进程验证通过，62app/43PTY/107term/242TS保持，MCP编辑器Exact无诊断。无Native/帧协议变更，未重复上一检查点已过的真实WebView。删除覆盖、原子落盘、外部并发编辑冲突、notify/设置UI仍未验收。

## 固定成本的主密码KEK验证原语

- 3项回归通过：RFC9106 §5.3 Argon2id v1.3已知答案（32KiB/p4/secret/AAD仅用于规范测试）；实际固定64MiB/t3/p1路径中，同密码/salt可恢复固定32-byte密文，换密码或salt认证失败，随机salt独立；忙时不排队、不开始第二次分配，空/超4096byte输入在准入前拒绝。生产入口不接受任意成本/版本/secret/AAD参数。
- 显式try_reserve工作区，Zeroizing<Vec<Block>>及32-byte key包裹所有正常/错误离开路径；单进程Mutex try_lock最多1job。密码是借用输入，由调用者负责寿命和清理；CPU工作不可取消，尚无异步调度、授权epoch提交、取消/迟到结果、限速/错误次数应用策略。派生成功不是密码认证成功或Vault解锁。
- 4次固定64MiB debug派生合计约16.6s，仅测试观察，不是production或跨平台基准、生产参数批准。49项普通store/1额外Root用例ignored、2真实崩溃、3compile_fail及完整check/test/45SSH/CLI严格lint/五独立进程/default63native均通过，62app/43PTY/107term/242TS保持，MCP Exact无诊断。无Native/帧协议改变，未重复已过DOM。此次audit实际core_exit0/audit_exit5：原六项license和两項maintenance保持失败。
- 仍无KEK包装DEK、版本化持久信封、秘密落库、OS钥匙串、主密码UI或完整应用锁，不读取用户密码/凭据。

## 内存KEK包装DEK与认证恢复

- `ValidationWrappedDek`只含private加密metadata，无持久serde/文件读取/任意成本。新建测试Vault随机生成独立32-byte DEK，KEK以Vault UUID+固定wrapped_dek kind的AAD加密；恢复严格48byte密文和可信预期ID，认证后检查32byte明文并通过Zeroizing源导入opaque cipher，未返回raw DEK、未生成替代key。
- 2回归通过：原DEK lock后正确密码恢复同一已有credential，错密码/tag篡改/连metadata一起换ID均认证失败；错目标ID、截断、nil ID在昂贵派生之前拒绝，原加密blob不变。对公开输入零化责任仍属调用者，派生后的key、工作区和解密DEK均有零化所有权。
- 增加cfg(test)串行场景锁仅协调两个真实高成本密码学夹具，生产1job try_lock及Busy语义不变；完整check/test、CLI严格lint、五独立进程通过，51普通store/1Rootignored/2真实崩溃/3compile_fail，62app/43PTY/107term/242TS保持；MCP Exact无诊断。无新依赖/参数/Native/帧变化，不重复上一阶段已过SSH/DOM/审计。
- 仍无持久信封格式、真实凭据/钥匙串、异步epoch与迟到key发布、应用锁或UI；这不是成功解锁/用户授权，后续控制层只能在当前授权与epoch确认后启用key。

## 后台内存解锁epoch与两阶段key确认

- `VaultKeyLifecycle`仅模型化backend key可用性，不等同用户/窗口授权。`AuthenticatedDek`由成功的wrapped_dek认证构造并带可信预期VaultID，普通KEK/随机cipher不能当证明。RAII permit→单次worker→candidate receipt→显式confirm；owner是不可串行化Weak pointer+不回绕epoch，没有caller字符串generation。scope clone共享状态、pending handle不保活owner。
- 初稿将候选key放在结果对象里，review发现lock只能撤权而不能同步清掉外部候选；已改为scope持有candidate key、结果是无key回执。lock/close/permitDrop/receiptDrop同步drop对应key，已锁或失败结果不会替换新scope。confirm借用回执，foreign拒绝不消耗当前合法确认机会。Mutex poison和epoch耗尽清key并fail closed。
- 9生命周期回归通过：候选未确认不可用、单worker/单pending、旧worker和已有candidate经过lock拒绝、取消后重绑、外来scope/同ID重建、owner销毁/错误VaultID、真实线程延后结果、重复confirm、耗尽时清active key、poison及lock/receiptDrop即时清candidate。真实64MiB KDF/tag认证成功结果接入当前permit确认并读取同一旧credential，未改断言或生产KDF Busy。
- 5 compile_fail通过（新增证明不能Debug、permit不能Clone）；全check/test/CLI严格lint/五进程/default63native通过，60普通store/1Rootignored/2崩溃回归，62app/43PTY/107term/242TS保持。MCP新模块诊断Exact无错误；改返回类型的引用工具带stale警告未算精确通过，实际workspace编译确认无漏改调用。无依赖版本/成本/帧变更，不重复已过DOM/SSH/审计；audit仍保持之前实际exit5。
- 尚无spawn_blocking/有界入队、CPU取消配额与result投递、窗口/页面授权整合、生产应用锁或idle锁、持久信封/钥匙串/用户凭据。控制器只能清自身持有的key，不能保证清除工作线程尚未投递的临时结果、早先合法transport副本、swap/core dump；后续监督层仍需处理它们。

## 有界后台主密码任务与取消交接

- `vault_unlock::begin_validation_unlock`只接受后端Rust scope、拥有密码和固定验证加密元数据。新PasswordInput 1..4096bytes、Zeroizing且禁止Debug/Clone/serde；借用副本仍归原调用者。KDF私有AtomicBool RAII令牌在CPU排队前取得，裸KDF与后台任务共用单quota，不能直接构造或复制令牌；没有新依赖/成本参数、前端命令或数据库schema。
- 一个spawn_blocking闭包持有密码与CPU令牌直至实际退出。未启动即接收方关闭/epoch失效跳过计算；运行中取消或ready未来timeout只撤销scope，CPU额度不提前释放。消费ready等待投递和join再给出无key回执，仍需当前permit确认；外来确认借用回执不消耗机会。显式cancel返回只可join、不能abort/清quota的观察收据。panic/认证失败/丢失或迟到结果不会自动重试、换DEK或启用key。
- 10项回归通过：joined仍Pending、foreign确认拒绝后合法确认；running取消/timeout仍Busy直到真实Drop；queued取消跳过compute；lock后的迟到线程不触碰新epoch；已投递未观察receiptDrop清key；worker panic释放额度且scope Locked；无Runtime/坏输入/错误ID不取得许可；Busy不消费任何scope的机会。额度实际退出采用cfg(test) oneshot见证，不轮询或凭sleep推断。真实64MiB KDF在阻塞线程完成、tag认证后当前epoch确认读取同一旧credential，错误密码拒绝。
- store严格lint及MCP Exact无诊断；完整check/test/schema漂移/五进程/2MiB会话/绑定漂移/默认原生严格lint与63tests通过。70普通store/1Root-only ignored/2真实进程崩溃/6 compile_fail，62app/43PTY/107term/242TS保持。改动不影响协议、Native工厂或算法/包features，未重复已过DOM/SSH/原有audit exit5。
- 仍是验证profile和内存scope，不是生产解锁/窗口授权/idle应用锁、系统钥匙串或持久信封；不能撤回已发给授权transport的secret副本、保证swap/core dump擦除或中断运行中的Argon2。生产参数及Win/mac解锁性能、窗口权限与应用退出时scope关闭须单独接入验收。

## 检查结果

- Rust：43项pulse-pty测试通过（profile目录、启动字段、WSL UTF16解析与本地环境）；107 项 pulse-term 单元/属性测试通过（新增2项bound resize ACK/no-op/取消/失败保持grid回归与1项subscribe commit gate回归）；pulse-ipc 33项通过（新增有序Repair、NoCredit稳定性和大整数边界）；pulse-app 62项测试通过（新增2项存储偏好到目录恢复回归；含4项可信活窗口transfer与3項刷新前批量view退役/Entry身份/捕获限流保持回归）；额外178项原上游测试通过；原生适配器default/encoding-probe各63项、login-probe66项测试通过（fresh生命周期与双额度先准入再claim回归）（新增导航期间在途页面许可撤销与无peer默认scope隔离回归；保留只读page2/两阶段收据回归）；xtask默认20项负向测试通过（含真实PTY迁移2项报告、目标runtime/display收据和codec/外部WM/GBK模式错配）；本轮未修改parser补丁或上游源码。
- TS：242 项测试通过（新增4项只读新页wrapper nonce参数、pagehide/迟到任务、渲染失败、混版/截止清理回归）（新增5项目标迁移capture的真实headless中文渲染/ACK确认顺序/取消/混版/确认失败回归；原park/PaneVisibility回归保留）；tsc、Biome、Vite 构建通过；负向类型用例确认 ID、camelCase 配置和二进制 Channel 参数的错误会被 tsc 拒绝。
- Rustfmt、clippy `-D warnings`、初始依赖分层检查通过。
- 原来的默认特性 cargo-deny 通过不足以覆盖原生依赖。现已改为全特性审计：来源与 bans 通过，许可证及维护公告失败；没有放宽白名单或添加公告 ignore，详见 [待审查依赖门禁](./dependency-review.md)。
- pnpm audit 无已知漏洞；初始化时发现 Vitest 4.1.0 的公告，已升级到 4.1.11。
- Astrolabe 已用于骨架、Rust 符号精确引用、TS 文件依赖范围及 Rust 诊断。TS 语言服务器诊断超时，未取得安装授权，不能视为通过；TS 的类型证据来自 tsc。

## 尚未验证

S1 已取得 Linux 合成通道/背压的部分证据，但未完成三平台验收。ConPTY、窗口材质/IME、真实协议/桌面/CLI已有部分受控证据，但真实设备与生产接口、Windows管道ACL及CLI窗口启动仍未完成。PTY 已有 Linux 真实交互与系统 Shell 冒烟证据，但交互/login shell 矩阵、Windows ConPTY、生产DOM GUI 接入与进程树回收仍待验证；Linux已有finite helper的真实WebView组合验证。类型生成已有 Linux 控制面证据，但三平台与 RC 依赖门禁未完成。已批准的非活动网格只读getter及双屏恢复已接入；隐藏光标分离/非行尾wrap等不能无损编码的状态明确延期，尚未接入生产attach；xterm.js 与 alacritty 的边缘序列分歧需要重新同步策略。Unicode 标量表已统一，但更多组合边界、字形与控制序列交互仍待验证。原生依赖门禁审查通过之前，不得打包分发验证应用。
