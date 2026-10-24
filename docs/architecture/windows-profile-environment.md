# Windows Profile 环境刷新

Windows 自动 Shell 发现与实际 PTY 启动均使用锁定 `portable-pty 0.9` 的环境刷新规则。发现阶段在现有受监督阻塞工作线程中构造只读 `CommandBuilder` 快照，取其中的 PATH 交给有界元数据发现；不执行 builder、不启动候选、不修改进程或注册表环境。页面仍只选择后端发现并保存的 Profile 身份。

依赖先继承进程环境，再覆盖可读取的系统／用户环境，并合并系统与用户 PATH。因此，PulseTerm 启动后改变的注册表 PATH 可以在下次发现时使用，不再仅依赖应用启动时的进程 PATH。注册表不可读时，依赖保留继承值或已读取的部分；本接口与启动保持相同的尽力刷新语义，不宣称原子环境快照或刷新必定成功。发现与随后启动分开读取，不能保证二者之间环境不发生变化。

固定系统 Shell 位置仍从 OS 元数据获取，ProgramFiles 后备位置不变。Windows PATH 发现最多接受256段和64KiB编码字节，超限或含NUL时拒绝整个发现，不截断为部分结果；Unix 的64段限制不变。相对／空分量仍忽略，候选数量及单个可执行路径预算不变，PATH 中 PowerShell 7 的第一个可用候选保持优先。隐藏和删除墓碑按稳定 Shell 身份保持，目录迁移不恢复已删除 Profile。已有会话不会因为发现环境变化而重启。

## 环境值与日志

锁定依赖在 Windows 刷新环境时使用 `log::trace!` 记录环境名称和值。Windows `pulse-pty` 依赖启用已锁定 `log 0.4.34` 的 `max_level_debug`，并以编译期常量断言要求静态上限不超过 Debug，环境 trace 即使被运行期 logger 请求也不能输出。

这会静态禁用该 Windows 依赖图中所有使用同一 `log` crate 的 Trace 宏；Debug／Info／Warn／Error 保留。Unix 构建不启用该限制。此措施不保证所有其他日志源安全，也不处理用户 Shell 自己输出的环境。代码不打印或持久化 PATH 或环境快照。

## 验证范围

纯元数据用例覆盖系统 Shell 身份、WSL 固定发行版、路径预算，以及 PATH 迁移后 PowerShell 7 的实际候选更新、隐藏状态和删除墓碑。它们可以在 Linux 执行，但不算 Windows 注册表／ConPTY 验收。Windows PTY 构建会实际求值静态日志断言；Windows 目标完整桌面检查已通过，保留既有平台功能警告。

Windows 原生验收入口、组包与采集器逐项要求 22 个测试名称／结果，其中包含 4 项发现用例。旧 21 项清单与旧回传保持原范围，不能用旧成功收据覆盖新范围。Linux真实执行49项PTY回归（含4项纯发现／迁移用例）、11项执行器回归通过；Windows PTY全部目标严格检查、Linux正式生产组合全部目标严格Clippy、基础check及39项归属回归与材料复查通过，结果与清理记录归档到 `target/probes/windows-profile-environment/verification-v1.json`。

待真实 Windows 普通用户验证：保持 PulseTerm 运行时更新可识别 Shell 的用户 PATH，重新发现并明确批准启动，核对新会话 PATH、旧会话不变、隐藏／删除保持，以及环境注册表读取失败的行为。该批不变更系统策略或用户注册表来伪造原生通过；MSVC／普通用户实机及发行许可门禁仍保留。
