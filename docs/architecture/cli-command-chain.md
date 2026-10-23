# CLI 命令链

CLI-01 的 `.` 与 `;` 可作为完整动作之间的独立 argv token 使用，例如 `pulse profiles . focus` 或 `pulse new-tab sh ';' split -v sh`。单独的 `pulse .` 是当前目录动作，不与命令链混用；`pulse -w <window> .` 可选择窗口。Shell 的分号需要引号；不解析或执行 shell 源码，也不拆分动作参数里的标点。`open --host "."` 和 `open --host ";"` 仍是原文保存入口名。


`pulse .` 使用后端保存且当前可用的默认 Profile；没有默认项时失败，不退回第一个 Shell。客户端仅捕获一次有界、绝对、可展示的当前目录，冷启动通过固定 GUI 创建空窗口，路径不进入 GUI 启动参数；随后在同一认证连接上传递目录。目录请求在能力参数校验后等待原页面的展示就绪标记（最多八秒，仍在原操作总期限内），再读取默认身份并显示 Profile 与完整目录。换页或取消使等待失效，不重发请求。

批准绑定原页的单次票据。目录只覆盖本次普通／自定义 Profile 的 native cwd，不改变保存配置，也不接受程序、argv、环境或分屏参数。批准前目录被删除时原生启动失败，不回退目录；路径按启动时的文件系统解释，不承诺文件身份冻结或消除 TOCTOU。Linux `mise run probe-workspace-cli-directory` 覆盖未设默认项、取消、刷新、目录失效、冷暖启动、实际 PTY cwd 和原生退出；Windows 原生行为另行验收，macOS 控制入口仍未接通。
`pulse new-tab sh . split -v` 的分屏步骤可省略 Profile，但必须存在已保存且可用的默认身份；缺失或不可用时停止该链，已注册首个会话保留。默认项读取不替换 GUI 编辑票据，命令链仍只用同一认证连接。

启动前检查全部动作及参数。最多八步、64 个 argv；空步骤、尾部分隔符、无效后续参数和超预算会整体返回语法错误，不读取 token、创建端点或拉起窗口。除单独 `pulse .` 从本进程读取当前目录外，不新增任意路径、地址、口令或环境参数；保存 Profile、精确保存入口和身份接口保持原有边界。

Windows 命令链首步可在安全缺失端点时使用固定同伴 GUI 冷启动及 bootstrap，其余动作复用同一认证连接。既有端点失败不冷回退，部分或不可信端点不修复，链内不重连。Windows 二实例参数转发仍拒绝；详见[Windows CLI 控制](windows-cli-control.md)。

首步可使用原有冷启动流程。多步链从首步起持有同一条认证 IPC 连接，包括冷启动 bootstrap；后续只在该连接上顺序调用，每步使用独立相关 ID，保留会话重放与请求预算防护。每次动作仍经过 Capability、原页和用户确认，认证连接不授予执行或凭据权限。后续不重新连接，不重新冷拉起，也不能跟随另一实例替换的端点。

每步必须取得并验证现有完成收据才进入下一步；创建和 SSH 包括原页面呈现完成。拒绝、超时、取消、连接失效或异常收据立即停止，已经注册的会话保留。客户端错误或等待取消会丢弃原传输，不重试不确定原生效果；原生回收仍由既有监督器持有容量与退出屏障。每个动作的批准与原生期限不延长。

Linux 限定验收批次完成：19 项解析／单动作兼容、2 项真实 IPC、44 项前端、5 项 selection 银行及严格原生检查通过；`Client` 和本地呈现模块 Astrolabe Exact、零诊断。本地注册后的 v5 事件绑定原启动 ticket、页面 nonce、selection ticket 与原生会话；原页先 admit，完成布局和输入确认后 complete，才向 CLI 返回成功。重复、旧页、pane 替换、输入失败和不确定完成不升级或重试。`mise run probe-workspace-cli-chain` 七个 Linux 实窗场景及六个共享控制器 cold split 兼容场景已核验，全部 GUI／PTY／端点退出。默认 check/test/probe 完成（1050 Rust／1204 前端，7 忽略未计），四 Rust 测试线程、不放宽断言；前端 23 警告／4 infos 保留。111 份 SHA 证据见 `target/probes/cli-chain-acceptance.json`，包括历史失败图（不作为当前成功证明）及来源快照；全退出后 clean-dev 回收 20.5 GiB，本批临时目录清空。仅本地 Linux 连接与 PTY，不计外部 SSH 命令链、硬件 Agent、跨平台、性能或发行验收；审计仍 8 项许可／2 项维护阻断。该历史批次仅覆盖单窗；当前多窗口 Linux 限定验收见 cli-multiwindow-routing.md。历史关闭 SIGSEGV 未定位，不能因本轮成功而视为修复。
