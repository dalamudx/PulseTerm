# 已实现范围与验证记录

本文记录可复现证据，不替代架构决策或验收清单。验证只在当前 Linux 环境执行；已添加三平台 CI，但尚无 CI 执行结果。

## 工程基础

- `.mise.toml` 固定 Rust 1.95.0、Node 22.23.3、pnpm 10.34.6、cargo-deny 0.20.2；任务统一使用 `mise run`。
- workspace 实例化 `pulse-core`、`pulse-ipc`、`pulse-term` 与 `xtask`，没有提前创建其余空 crate。
- `cargo xtask check-deps` 检查已实例化 crate 的直接依赖方向、Tauri 隔离和领域层已知 I/O 依赖；这是初始规则，不是完整的传递依赖/源码安全证明。
- Rust 禁止 unsafe；生产代码禁止 unwrap/expect；TS 开启 strict 和 noUncheckedIndexedAccess，使用 Biome。
- GitHub Actions 配置 Linux/Windows/macOS 检查、测试和探针；VT 样例禁止换行归一化。

## 二进制 IPC 编解码

`pulse-ipc` 编解码与 `ui/src/ipc/frames.ts` 解码共同使用 `tests/fixtures/frames/data.hex`。

- 6 字节头：kind、保留 flags、little-endian u32 sequence；payload 不超过 1 MiB。
- 拒绝短头、未知 kind、非零 flags、超长数据；TS 正确处理非零 byteOffset，payload 引用原 buffer。
- Rust 属性测试覆盖随机载荷往返、任意输入不 panic；尚未实现 Channel 传输、信用额度、顺序恢复或桌面载荷分片。

## 终端双解析器探针（S2、ADR-026 的部分证据）

```sh
mise run probe
# target/probes/terminal.json：Rust 观测、VT 重放、应答和内存下限
# target/probes/xterm.json：跨解析器比较结果，探索性失败同样记录
```

- 使用 alacritty_terminal 0.26.0 / vte 0.15.0 和 xterm headless 6.0.0；xterm 加载 Unicode11。这不代表已经统一全部 Unicode 宽度表。
- 通过 6 个基本样例：ASCII、颜色/样式、中文与基本 emoji/组合音符、备用屏、返回主屏、光标位置查询。比较内容、宽度、颜色、部分样式、光标位置和备用屏标志。
- 前端策略阻止常见 CSI/DCS/OSC 查询应答及 OSC 52；后端光标位置查询产生一份应答。调色板写入可继续交给后续处理器。完整查询覆盖、颜色/尺寸应答、混合调色写入与查询仍待实现。
- `explore-zwj` 保留为明确标记的探索性样例，当前失败，不阻塞基础样例检查；不能把该样例或总体 S2 宣称为通过。

### 已修复：UTF-8 分块时漏字符

全流解析和分块解析对 `e + U+0301 + 空格 + 🌍` 曾产生不同结果。vte 0.15.0 在补全前一块的部分码点时，可能把紧随其后的 ASCII 字节计入消费长度，却只输出第一个码点。

适配层在进入解析器前保留最多 3 字节的未完整 UTF-8 尾部，下一块补齐；不修改非法字节或 C1 控制字节。新增所有固定块大小的状态一致性、组合音符后空格、分块中文和非法字节边界回归测试。未完整 EOF 的策略尚未实现。

### 视口重放不是生产快照

`TerminalProbe::screen_replay` 仅用于可见格子重放：**不恢复**非活动 buffer、历史、saved cursor、pending wrap、scroll region、tab stops、输入中间态、部分样式、链接及完整模式集。不得用于真实 detach/attach。还需 vim/htop/tmux 录制流以及重放后继续输入的状态一致性验证。

### 历史内存预算仍未满足

120 列、40 行视口、保留 10,000 行历史时，按 `size_of::<Cell>()` 计算的格子存储下限为 **28,915,200 字节（约 27.6 MiB）**，不含 extra、分配开销或进程内存。

当前是有上限的热历史，不是冷热分层/压缩归档。10 万行历史目标需要独立压缩存储，不能直接沿用 alacritty 原始格子。此数值也不是整棵进程树的内存测试。

## 检查结果

- Rust：11 项单元/属性测试通过。
- TS：5 项测试通过；tsc、Biome 通过。
- Rustfmt、clippy `-D warnings`、初始依赖分层检查通过。
- cargo-deny：公告、来源、许可证及 bans 通过，保留 windows-sys 双版本和未使用白名单项的警告。
- pnpm audit 无已知漏洞；初始化时发现 Vitest 4.1.0 的公告，已升级到 4.1.11。
- Astrolabe 已用于骨架、Rust 符号精确引用、TS 文件依赖范围及 Rust 诊断。TS 语言服务器诊断超时，未取得安装授权，不能视为通过；TS 的类型证据来自 tsc。

## 尚未验证

真实 Tauri Channel 吞吐/回压（S1）、ConPTY、窗口材质/IME、真实 PTY/SSH 互通、桌面显示与编解码、CLI IPC、类型代码生成均未实现或验证。下一步首先补充完整状态恢复与 Unicode 宽度一致性证据，并建立真实 WebView 数据通道测量，避免把 headless 结果误当性能验收。
