# 已实现范围与验证记录

本文记录可复现证据，不替代架构决策或验收清单。验证只在当前 Linux 环境执行；已添加三平台 CI，但尚无 CI 执行结果。

## 工程基础

- `.mise.toml` 固定 Rust 1.95.0、Node 22.23.3、pnpm 10.34.6、cargo-deny 0.20.2；任务统一使用 `mise run`。
- workspace 实例化 `pulse-core`、`pulse-ipc`、`pulse-term`、`pulse-pty`、`pulseterm-desktop` 与 `xtask`；原生 GUI 依赖受 `desktop` feature 控制，默认仍可做 headless 核心检查。
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

- 使用 alacritty_terminal 0.26.0 / vte 0.15.0 和 xterm headless 6.0.0；xterm 加载 Unicode11。这不代表已经统一全部 Unicode 宽度表。
- 通过 6 个基本样例：ASCII、颜色/样式、中文与基本 emoji/组合音符、备用屏、返回主屏、光标位置查询。比较内容、宽度、颜色、部分样式、光标位置和备用屏标志。
- 前端策略阻止常见 CSI/DCS/OSC 查询应答及 OSC 52；新增公共 DECRQM（`CSI $ p`）拦截，修复只拦截私有模式而遗漏公共模式的重复应答缺口。单独测试先观测 xterm 原有应答，再安装 hook 验证应答消失，并在释放 hook 后验证模式写入未被吞掉。
- `tests/fixtures/terminal/queries.json` 的 14 组共享用例覆盖 DA1/DA2、DSR、CJK 光标位置、公共/私有 DECRQM、字符尺寸、多查询应答顺序及 OSC 52 读写拒绝。Rust 检查每种固定块大小（1 到输入字节长度），逐块消费应答；前端检查 1/2/3/7/整流分块，必须没有协议应答且屏幕与 Rust 一致。查询矩阵写入 `terminal.json`，跨解析器结果写入 `xterm.json`，由现有 `mise run probe` 和 CI 执行。
- 这不是完整查询兼容性证明：颜色/像素尺寸应答、混合调色写入与查询等仍待实现；矩阵中 OSC 52 无应答仅证明探针的拒绝策略，不证明应用已实现完整的剪贴板权限流程。
- `explore-zwj` 保留为明确标记的探索性样例，当前失败，不阻塞基础样例检查；不能把该样例或总体 S2 宣称为通过。

### 已修复：UTF-8 分块时漏字符

全流解析和分块解析对 `e + U+0301 + 空格 + 🌍` 曾产生不同结果。vte 0.15.0 在补全前一块的部分码点时，可能把紧随其后的 ASCII 字节计入消费长度，却只输出第一个码点。

适配层在进入解析器前保留最多 3 字节的未完整 UTF-8 尾部，下一块补齐；不修改非尾部的非法字节或 C1 控制字节。新增所有固定块大小的状态一致性、组合音符后空格、分块中文和非法字节边界回归测试。

`TerminalProbe::finish_input()` 明确 EOF 策略：一个未完整的尾部 Unicode 标量替换为一个 U+FFFD；重复调用不重复输出，完整输入与未闭合的 CSI/OSC 不被强行补全。6 种不完整标量前缀及幂等性有回归测试。该方法不会恢复/序列化 ANSI 解析器状态，生产 transport 的关闭路径尚未接入。

新增 proptest 混合流检查：ASCII、CJK、emoji、组合音符、SGR、光标移动、备用屏切换和查询按随机长度分块，逐块收取应答后，最终屏幕和应答顺序必须与整流输入一致。范围限于这些合法 UTF-8/控制序列，不等于任意恶意字节流已完整验证。

### 视口重放不是生产快照

`TerminalProbe::screen_replay` 仅用于可见格子重放：**不恢复**非活动 buffer、历史、saved cursor、pending wrap、scroll region、tab stops、输入中间态、部分样式、链接及完整模式集。不得用于真实 detach/attach。还需 vim/htop/tmux 录制流以及重放后继续输入的状态一致性验证。

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
- 8 个命令与 `ProbeEcho` 事件共用 `command_bindings()`：同一份注册表生成 `ui/src/ipc/gen/commands.ts` 并注册 invoke/event handler。DTO 包含嵌套对象、枚举、数组和 nullable 字段；前端不再手写重复的配置、统计、测量与报告字段。
- `RawFrame` 不实现 Serialize，仅通过 `IpcResponse` 返回 `InvokeResponseBody::Raw`；反射映射为 `ArrayBuffer`，生成参数为 `Channel<ArrayBuffer>`，不存在 number[] 或 JSON 字节序列的替换。Rust 单元测试和实际 WebView 的逐字节检查均通过。
- 增加带随机 nonce 的生成命令 → Rust 类型化事件 → 生成事件监听器回环。监听先注册，随后发命令，超时/成功都释放监听；原生报告版本改为 2，必须有 `typedEvents: true`、取消证据及 7 组测量才可标记成功。Linux 实测回环与全部传输组通过。
- `mise run bindings` 写入绑定；`mise run bindings-check` 比较重新生成的临时内容但不覆盖源文件。手动改变绑定注释的行为探针确认检查失败且源文件哈希不变；随后恢复原文。三平台 CI 已加检查，但尚无远端运行证据。
- **已验证的限制**：rc.25 的递归 `serde_json::Value` 反射会栈溢出，因此未启用 Specta 的 serde_json feature，报告改为显式 DTO；不依赖 generic JSON 反射。`usize` 被精度策略拒绝，控制 DTO 改为有界 u32、内部乘法用 u64 验证，没有放宽 BigInt 禁令。Serde 的浮点输出可能为 null，生成器如实导出 `number | null`；前端仅在有限数运行时检查后用于算术，不用手写接口掩盖 null。
- 新增 `paste 1.0.15` 维护公告来自 Specta 的函数反射，详见 [依赖审查](./dependency-review.md)。没有添加 ignore 或宏补丁。S21 功能链路的 Linux 证据不代表依赖、三平台或正式采用门禁已通过。

## 本地 PTY 原语与真实交互探针（S3 前置基础）

```sh
mise run probe-pty
# target/probes/pty.json
```

- 新增独立 `pulse-pty`：封装 portable-pty 0.9.0，使用显式 executable / literal argv、cwd、环境覆盖与默认 TERM；提供单 reader、完整写入、字符尺寸读写、非阻塞退出观察、等待、关闭输入、终止并回收直接子进程。不依赖 Tauri、解析器或前端。
- 原语是**阻塞接口**，不隐藏队列或线程，不能直接运行在 UI/async worker 上。PTY 生产桥接、可取消写入、进程树/Job Object 回收和平台 Shell 探测尚未实现；独立 actor 已有 EchoTransport 证据，但还未接入此阻塞原语。Drop 是直接子进程的尽力清理，不是取消任意阻塞 I/O 的保证；Windows 调用方必须在关闭 ConPTY 期间持续排空 reader。
- Linux 真实 PTY helper 检查 stdin/stdout 为 TTY、literal argv/cwd/env 未变、输入逐字节未变；resize 到 91×37 后既检查 master readback，也由子进程 `stty size` 观测。中文与光标查询进入 Rust `TerminalProbe`，应答 `CSI 1;7 R` 原样写回 PTY，并由子进程核验，最后检查 Rust 屏幕及尾部输出、退出码 7 和 EOF。
- 使用固定 `/bin/sh -c` 脚本作系统 Shell 冒烟：确认 TTY、输出 VT 流、Rust 解析结果和正常退出。不是交互/login Shell、OSC 133、vim/tmux 或 Shell 环境探测的兼容性验收。
- 慢消费者暂停读队列，观测 reader 的 send attempt 停止推进，子进程仍未退出；恢复消费后按顺序逐字节验证 **1 MiB** 确定性输出和结束标记，没有丢弃数据。探针限定 2 个排队块，每块 ≤4 KiB；计入消费者当前块、producer 待发送块与固定 reader scratch，分块暂存上界为 **20 KiB**。这是代码结构的缓冲预算，不是 RSS/整个进程树内存测量，排除 OS buffer、线程栈其余部分、握手捕获及解析器状态。
- 检查关闭输入幂等、单 reader、重复 terminate 的退出状态稳定、直接存活子进程 Drop 后 EOF、所有正常 reader join、不存在可执行文件时 spawn 失败。本机直接终止 + 排空约 50 ms，用 2 秒上限检查；不能外推到其他系统或子孙进程。
- helper 独立 10 秒截止时间，worker 外层 25 秒监督，失败不读取上一轮结果；报告要求完整布尔观察、版本、字节/队列预算、精确应答/退出与取消上限，负向单元测试拒绝仅有 completed 标志等不完整报告。
- 已接入 Linux/macOS CI 的 `probe-pty`，Windows CI 编译原语并运行纯逻辑测试；本轮仅取得 Linux 结果。**S3/S10 尚未通过**：随附 ConPTY 加载、win32-input-mode、Win10/11、PowerShell/WSL 及真实 UI 兼容性待验证。

## 独立会话 actor / EchoTransport（S11 前置基础）

- `pulse-term::session` 由一个 Tokio task 独占解析器/网格，外部只有有界命令和小型 watch 元数据；没有 `Mutex<Term>`、Tauri、窗口或 DOM 依赖。`SessionTask` 提供可监督的 join，终态屏幕仅供诊断，不是恢复快照。
- Echo 驱动的输入/输出队列各 8 块，块上限 64 KiB；命令队列 8 条；协议应答积压载荷上限 256 KiB，另有字符串/队列开销。超量消息拒绝；满队列明确返回 Busy（未接受），不伪装写入成功。调用方已构造的消息、无界并发调用数量、内核缓存及网格不在此预算内。
- 后端查询回复单独写入 upstream，不再当作程序输出回送。输出解析每轮有界，选择循环公平处理控制命令；取消在每轮先检查且走带外 watch，不排在数据后面。正常 EOF 完成 UTF-8 尾部；取消/协议溢出与正常 Ended 分开标记。
- 11 个本地测试覆盖无视图输出、观察者释放、最后控制 handle 放弃、字节 roundtrip、尺寸变化、协议应答、输入/输出超限、满队列关闭、持续输出下 inspect/close、公平调度和最终 EOF 替换字符。合成驱动的结束 join 包括 driver 回收；2 秒测试 deadline 不等于 OS I/O 的退出保证。
- 当前 **ChannelTransport 仅有测试驱动**，不是已完成的生产 Transport/PTY async bridge。输入成功只说明队列接受，不代表物理写完；Echo resize 只变更 parser，不代表系统 PTY 已 resize。Registry、多视图关联、额度/落后快照、生产生命周期事件节流和 OS 进程树回收仍待实施。
- S11 仍未整体验收：没有实际窗口迁移或 UI 销毁/恢复结果，视口投影禁止充当 attach 快照。此实现只建立与视图无关的基础，不宣称已经实现完整会话恢复。

## 检查结果

- Rust：33 项 headless 单元/属性测试通过（原有 22 项 + actor/有界驱动 11 项）；原生适配器 8 项测试已在此前通过，本轮未修改桌面代码。
- TS：24 项测试通过；tsc、Biome、Vite 构建通过；负向类型用例确认 ID、camelCase 配置和二进制 Channel 参数的错误会被 tsc 拒绝。
- Rustfmt、clippy `-D warnings`、初始依赖分层检查通过。
- 原来的默认特性 cargo-deny 通过不足以覆盖原生依赖。现已改为全特性审计：来源与 bans 通过，许可证及维护公告失败；没有放宽白名单或添加公告 ignore，详见 [待审查依赖门禁](./dependency-review.md)。
- pnpm audit 无已知漏洞；初始化时发现 Vitest 4.1.0 的公告，已升级到 4.1.11。
- Astrolabe 已用于骨架、Rust 符号精确引用、TS 文件依赖范围及 Rust 诊断。TS 语言服务器诊断超时，未取得安装授权，不能视为通过；TS 的类型证据来自 tsc。

## 尚未验证

S1 已取得 Linux 合成通道/背压的部分证据，但未完成三平台验收。ConPTY、窗口材质/IME、真实 SSH 互通、桌面显示与编解码、CLI IPC 仍未实现或验证。PTY 已有 Linux 真实交互与系统 Shell 冒烟证据，但交互/login shell 矩阵、Windows ConPTY、GUI 接入与生产进程树回收仍待验证。类型生成已有 Linux 控制面证据，但三平台与 RC 依赖门禁未完成。完整终端状态恢复与 Unicode 宽度一致性仍未通过；后续应继续补这些证据，不能直接将视口重放用于生产 detach/attach。原生依赖门禁审查通过之前，不得打包分发验证应用。
