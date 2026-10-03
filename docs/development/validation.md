# 已实现范围与验证记录

本文记录可复现证据，不替代架构决策或验收清单。验证只在当前 Linux 环境执行；已添加三平台 CI，但尚无 CI 执行结果。

## 工程基础

- `.mise.toml` 固定 Rust 1.95.0、Node 22.23.3、pnpm 10.34.6、cargo-deny 0.20.2；任务统一使用 `mise run`。
- workspace 实例化 `pulse-core`、`pulse-ipc`、`pulse-term`、`pulseterm-desktop` 与 `xtask`；原生 GUI 依赖受 `desktop` feature 控制，默认仍可做 headless 核心检查。
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

## 检查结果

- Rust：17 项核心单元/属性测试通过；原生适配器的 4 项测试已在此前验证通过，本轮没有修改原生适配器。
- TS：23 项测试通过；tsc、Biome 通过；验证应用 Vite 资源构建此前通过，本轮没有修改验证界面。
- Rustfmt、clippy `-D warnings`、初始依赖分层检查通过。
- 原来的默认特性 cargo-deny 通过不足以覆盖原生依赖。现已改为全特性审计：来源与 bans 通过，许可证及维护公告失败；没有放宽白名单或添加公告 ignore，详见 [待审查依赖门禁](./dependency-review.md)。
- pnpm audit 无已知漏洞；初始化时发现 Vitest 4.1.0 的公告，已升级到 4.1.11。
- Astrolabe 已用于骨架、Rust 符号精确引用、TS 文件依赖范围及 Rust 诊断。TS 语言服务器诊断超时，未取得安装授权，不能视为通过；TS 的类型证据来自 tsc。

## 尚未验证

S1 已取得 Linux 合成通道/背压的部分证据，但未完成三平台验收。ConPTY、窗口材质/IME、真实 PTY/SSH 互通、桌面显示与编解码、CLI IPC、类型代码生成仍未实现或验证。完整终端状态恢复与 Unicode 宽度一致性仍未通过；后续应继续补这些证据，不能直接将视口重放用于生产 detach/attach。原生依赖门禁审查通过之前，不得打包分发验证应用。
