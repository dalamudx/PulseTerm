# TERM、Delete 与 Profile 终端行为

本批集中补齐 TERM-18 的 TERM 选择、Delete 编码，以及 Shell Profile 的 Option／Backspace／Delete 覆盖。沿用原有设置文件、Profile 单次保存票据、原页授权与输入队列，没有增加执行权限。

## TERM 启动配置

`[terminal].term_type` 是封闭枚举：`xterm-256color`（默认）、`xterm`、`vt100`。它只声明终端能力名称，不切换解析器或承诺完整的其他终端仿真。

本地 GUI／CLI 与 SSH 使用同次受监督设置读取产生的 `TerminalStartConfig`，将 TERM 与基础配色、历史容量一起冻结。已有会话不会在线改变环境。SSH 密码、私钥与 Agent 共用终端初始化，跳板链只在叶连接请求 PTY 时使用该值，中间连接不创建终端。

本地启动将全局值用于 `prepare_local_command` 插入的默认 TERM。显式 Profile 环境项仍位于默认值之后，保留原有优先级；用户可在 Profile 的已有环境变量区域保存 `TERM` 覆盖。参数、工作目录、环境和行为覆盖仍在一次 Profile 保存中提交，不做分次写入。

## Delete 编码与输入边界

`[terminal].delete_send` 可选 `csi`（默认，`ESC [ 3 ~`）或 `del`（`0x7f`／`^?`）。默认交给 xterm；选择 DEL 后，只转换当前有效视图内可信、未被处理、无 Ctrl／Alt／Meta／Shift 修饰且非组合输入的普通 Delete。WebKit 229、Process、组合输入和修饰键沿用既有逻辑。

转换后经公开 `Terminal.input(..., true)` 回到既有 `onData`、查询过滤和输入授权队列。只有输入已启用且 `disableStdin` 为 false 时才转换。开门前、关闭后或退场后不发送；粘贴和普通文本中的字节不做替换。偏好在新打开或重建视图时应用。

## Profile 稀疏覆盖

Shell Profile 表单提供“继承全局设置”选择：`appearance.macOptionIsMeta`（布尔或 null）、`appearance.backspaceSend`（`del`／`bs` 或 null）、`appearance.deleteSend`（`csi`／`del` 或 null），以及 `appearance.colorPreset`（`classic`／`midnight`／`paper` 或 null）。显式 false 代表关闭，null 代表继承，二者不混淆。旧外观记录缺少配色字段时继承全局；未知字段、非法类型和编码被拒绝。

按键值随既有外观快照保存、恢复并合并到对应本地会话的 Surface；Profile 配色在 PTY 创建前覆盖本次启动基色，经后端引导和捕获恢复显示，不作为 xterm 显示偏好自行应用。Profile 修改不在线改变已创建会话的快照；全局设置也不在线改变已挂载的 Surface。分组／SSH入口的[统一编辑与新会话应用](terminal-scoped-behavior.md)已接通配色与四项行为的继承、单次保存和刷新恢复；祖先组不额外叠加，原生及跨平台效果仍需验收。

## 输入／输出滚动与视图应用

全局 `scroll_on_input` 默认true、`scroll_on_output` 默认false。明确保存只更新设置，已挂载Surface继续使用创建时的显示偏好；刷新重建同一会话视图或新建GUI／CLI视图才采用新值。这些是显示偏好，不与TERM、配色、历史容量的会话启动冻结合同混同。恢复继承只移除对应覆盖，随后新建／重建视图采用默认值。

正式 `mise run probe-workspace-scroll-settings` 复用设置夹具和CLI批准，已通过Linux生产WebView／两PTY的五段检查：默认行为、明确反转、旧视图保持、同PTY刷新、CLI新视图继承和逐项恢复继承。输出先等同包BEL解析，再跨越原一秒绘制期限观察保位；最终精确输入分别为abc／xyz，两个实际PTY进程及GUI回收，原生drain／exit完成。收据、原始输入快照与截图见 `target/probes/workspace-scroll-settings/accepted/b2c3c2ac/`。本批不扩大到SSH、系统IME、硬件多屏或跨平台。

## 投影与检查

最新设置投影为 v11，要求 `delete_send` 与 `term_type` 的封闭值和来源；v1～v10 保持原有字段契约，缺少新增项时采用 `csi`／`xterm-256color`。原页保存、恢复继承、冲突检查和过期票据拒绝沿用现有边界。

4 项后端直接检查覆盖设置往返／继承、完整 Profile 重开与来源冲突、TERM 默认与显式 Profile 优先级、原生保存／恢复及旧页拒绝；43 项受影响前端行为与 3 项新场景通过。TypeScript、schema、格式、局部 Biome、Store／App／桌面库严格 Clippy 和产品构建通过。Biome 保留一项既有警告与一项既有信息；Store Astrolabe 诊断 Exact 零问题，TypeScript 语言服务不可用不计通过，未安装额外工具。

一次 Chrome／真实 xterm 组件检查用可信 CDP 按键确认 CSI／DEL 字节、Ctrl 修饰键保留、粘贴保真及输入门禁，同时用真实 Profile 表单和受控原生适配确认三项覆盖单次保存、显式 false 和已有 TERM 环境不丢失。该检查没有原生 IPC、PTY 或 SSH，不能替代原生 WebView、实际 `$TERM`、远端 PTY 请求、物理 Option 或系统 IME 验收。

未跑完整回归或新增 SSH／跨平台矩阵，沿用现有 CI。开发记录位于 `target/probes/terminal-behavior-development/`；CR／LF、主机／分组产品覆盖入口和跨平台实窗验证仍待推进。
