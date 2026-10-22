# PulseTerm 命令行使用说明

[English](cli.en.md)

当前为开发预览，尚未达到跨平台 Alpha 验收。本文描述已接通的 `pulse` 命令。Linux 有分项运行证据；Windows 控制入口已接线，原生 GUI 尚待验收；macOS 暂不支持控制命令。

## 离线帮助和版本

```sh
pulse --help=zh-CN
pulse --help
pulse --version
```

`-h` 与 `-V` 分别等价于英文帮助和版本。它们必须单独使用，不读取控制 token、访问工作区或启动 GUI。版本来自当前二进制的包版本；同版本可有不同构建，故障回报还应保留随包提供的构建标识。

PowerShell 从当前目录运行时使用 `.\pulse.exe --help=zh-CN`。CLI 需要显式 `workspace-cli` 构建，普通工作区和本地验证包不自动包含它；SSH 操作还需含 SSH 的工作区。CLI 与配套 GUI 应来自同一构建。

## 发现与启动本地 Profile

```sh
pulse profiles
```

输出形如：

```json
{"version":1,"profiles":["bash","sh"]}
```

复制实际返回的字符串作为 `PROFILE`。自动身份包括 `bash`、`zsh`、`fish`、`sh`、`pwsh`、`powershell`、`cmd`、`wsl`，只返回当前已发现、已保存且未隐藏的项；自定义保存项使用稳定 ID。`pwsh` 与 `powershell` 是不同身份。

例如，在已发现 `sh` 的 Linux 环境：

```sh
pulse new-tab sh
pulse split -v sh
pulse split-pane -h sh
```

在已发现 Windows PowerShell 的 Windows 环境：

```powershell
.\pulse.exe new-tab powershell
.\pulse.exe split -v powershell
```

`-v` 左右分屏，`-h` 上下分屏，可省略 Profile，此时仅用已保存默认项；未设置、隐藏、删除或不可用时失败，不尝试其他 Shell。`open PROFILE` 是 `new-tab PROFILE` 的别名。Profile 是保存身份，不是程序路径；程序、参数、工作目录与环境使用 GUI 中保存并经批准的设置。

新终端需原 GUI 窗口确认，并等待实际呈现完成。返回 `session N` 后，可用 `focus-tab N` 选择该会话；只能选择目标窗口拥有的会话。

## 选择窗口

```sh
pulse windows
pulse -w workspace focus
pulse -w workspace new-tab sh
```

`windows` 返回已发布标签，例如 `workspace` 与 `workspace-…`，不是窗口标题或会话编号。该全局列表不接受 `-w`。多个窗口时，其他动作需要精确的 `-w WINDOW`，应复制实际列表的完整标签。

`pulse new-window` 要求 GUI 已运行并在原窗口批准，创建空工作区，不复制另一窗口的会话或 Vault 解锁状态。

## 连接已保存的 SSH 入口

```sh
pulse open --host my-server
```

入口名必须精确匹配保存记录；含空格的名称按所用 Shell 的规则引用。`open my-server` 会按本地 Profile 解析，不用于 SSH 名称解析。

显式 ID 接口为：

```text
pulse ssh HOST_ID --identity IDENTITY_ID [AUTH]
```

Host 与 Identity 使用已有保存记录的 ID，不接受原始地址、用户名参数、口令、密钥字节或任意程序。显式认证选项包括 `--interactive`、`--key-interactive`、`--agent-interactive`、`--password-interactive`，须与保存身份和 GUI 支持的认证路径匹配，失败不自动换方法。

在 GUI 完成实际目标、首次或变化指纹、Vault 与认证批准。MFA 回答也在原 GUI 输入。保存配置或连上控制通道不授予网络连接和凭据权限；取消批准后不自动连接。

## 串联动作

```sh
pulse profiles . focus
pulse profiles ';' focus
pulse -w workspace profiles . -w workspace focus
```

`.` 和 `;` 只在完整动作之间作为独立参数使用，分号须在 Shell 中引用。单独的 `pulse .` 不属于当前语法。

整条链最多 8 步、64 个参数，启动任何动作前先检查全部语法。`-w` 对每个动作分别生效，不传给后续步骤。首步冷启动后，后续仍用同一连接。

每步获得确认结果后才继续。拒绝、超时、断连或无法确认会停止后续步骤；已创建会话保留。链不回滚、不重连、不自动重试不确定的动作。失败后先检查 GUI 实际状态，再决定下一步。

## 冷启动与故障处理

首次支持的动作可启动同目录的配套 `pulseterm-workspace`（Windows 为 `.exe`），再等待原页批准。`windows` 与 `new-window` 要求 GUI 已运行。冷启动不查找任意程序；Windows 拒绝第二个 GUI 实例的启动动作转发，并发冷起可能失败。

| 情况 | 处理方式 |
|---|---|
| 动作或参数无效 | 查看离线帮助；语法错误退出 2，不执行链的前半段 |
| Profile 不可用 | 重新查看 `profiles`，在 GUI 发现／保存／显示该项，不传程序路径 |
| 窗口不明确 | 用 `windows` 取得完整标签，在该动作前加 `-w` |
| 缺少 GUI 配套程序 | 使用同一构建的配套程序，或先正常打开 GUI，不用其他程序改名替代 |
| 端点不安全、陈旧或认证失败 | 不自动修复、删除 token 或转冷启动，保留错误和实际进程状态 |
| 超时或结果未确认 | 退出 1；检查 GUI 是否已完成，不自动重复创建或连接 |

正常确认返回 0，失败或未确认返回 1，语法错误返回 2。帮助或版本输出不表示平台已验收或可正式发行。

故障回报请提供版本、随包构建标识、操作系统、命令结构、失败阶段、退出码与 GUI 是否仍运行。脱敏入口名、主机名等元数据；不附口令、私钥、MFA 回答或 Vault 数据库。
