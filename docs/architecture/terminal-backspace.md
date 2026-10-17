# Backspace 编码设置

全局终端设置新增 Backspace 发送选择：`del`（默认，`0x7f`／`^?`）或 `bs`（`0x08`／`^H`），存储为 `[terminal].backspace_send`。支持单字段保存、冲突校验和恢复继承。设置作用于下次打开或重建的视图，已挂载视图保持原偏好。

## 输入行为

默认 `del` 保留 xterm 行为；选择 `bs` 时，只处理当前有效视图中可信、未被处理、无 Ctrl／Alt／Meta／Shift 修饰的 Backspace `keydown`。组合输入状态、WebKit 229、`Process` 和修饰键均继续交给原有逻辑。

Surface 必须已通过现有 `enableInput` 开门，且 SDK 的 `disableStdin` 为 false。选择的 BS 使用公开 `Terminal.input(..., true)` 交回现有 `onData`、查询回复过滤及授权队列，不增加独立的原生输入命令。关闭输入、视图撤销或释放后停止改码。普通文本、粘贴和远端解析不做全局字节替换。

## 配置与范围

v10 起设置投影要求 `backspace_send` 为封闭的 `del`／`bs` 值；v1～v9 不包含该字段，显示投影默认采用 `del`。最新 v11 继续保留此契约。Profile 编辑器现已支持 Backspace 稀疏覆盖，null 继承全局、`del`／`bs` 显式覆盖，与其他 Profile 配置通过同一次保存提交，见[TERM、Delete与Profile行为](terminal-behavior.md)。Store 支持默认→全局→分组→主机的同字段覆盖；分组／SSH入口的[四项统一编辑与新会话应用](terminal-scoped-behavior.md)已接通，原生与跨平台效果仍待验收。

TERM、Delete 与 Profile 按键覆盖已在[后续成组实现](terminal-behavior.md)接通；CR／LF 和终端程序自行管理的键位仍属后续范围。

## 开发验证

一项 Store 保存、继承、非法值和无损删除检查，一项原生保存／恢复／旧页撤销检查及 26 项受影响前端行为通过。TypeScript、settings schema、局部 Biome、格式、Store／桌面库严格 Clippy 和产品构建通过；Store Astrolabe 诊断为 Exact 零问题。沿用现有 CI，不新增矩阵，未跑完整回归。

Chrome／真实 xterm 使用可信 CDP 按键和受控输入接收器确认：默认 DEL、配置 BS、Alt／Shift 保留、粘贴保真，以及开门前、关闭后、视图退场后无输入。该检查没有 PTY 或原生 IPC，不能替代真实 PTY／SSH、原生 WebView、系统 IME、macOS 或 Windows 物理键验收。

开发日志、组件截图和源码摘要保存于 `target/probes/backspace-development/`。
