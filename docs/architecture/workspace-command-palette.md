# 工作区命令面板与模态键盘边界

本说明记录既有工作区动作的 UI 复用；不新增 Capability 执行协议，不替代后端注册、窗口／当前页／会话授权。

## 已接范围

`ui/src/workspace-command-catalog.ts` 是十六项固定显示目录：原八项 Profile 管理、终端设置、SSH 主机、SSH 身份、Vault、显式 SSH 选择、关闭当前会话及 SSH 配置，加上快捷键、组织、SSH 私钥、Agent 身份、快速连接、SSH 配置导入与两种分屏。条目复用已有入口；不收程序、参数、地址、主机密钥、凭据或文件路径。工厂关闭、按钮不可用或缺少目标的动作不显示。名称和查询支持中英文。

名单来自当前页授权的共享只读 `workspace_cap_list`（默认 11 项／显式 SSH 16 项），只含固定 locale key／窗口／风险／闭合空 schema；不含处理函数、执行权限或 CLI surface。失败或未 ready 时禁用目录，没有静态 fallback；旧页、迟到响应和撤销不能装入新页。显示捕获绑定原页面 nonce、动作与原目标；关闭使用 focusedSession，分屏绑定原来的两个会话。执行重检页面、busy／fit、按钮和目标，回调前单次消费；true 只表示调用原 UI 入口。可配置键位继续保持原八项 UI 动作与 palette，不随面板扩展；保存协议和 CLI 权限不变。查询、复制条目与原始数据仍有界，不确定结果不自动重试。

`workspace-command-palette.ts` 使用固定 dialog 和 textContent；combobox/listbox、当前选中项、方向键、Enter、空结果、cancel 和焦点都由当前生命周期管理。有效选择关闭面板并恢复当前 connected opener，原编辑器沿用自身读取／保存／取消链。旧 nonce、busy 或退出不恢复旧焦点，不安装输入权限。CSS 静态随 Vite 打包，严格 CSP 不放宽。

## 快捷键不等于终端输入

窗口捕获 Ctrl/Meta+Shift+P，重复手势也消费，不向 xterm 变成 Ctrl+P。普通 Ctrl+P 不匹配；IME 组合、Alt、已消费事件不用于打开面板。可配置键位的8固定UI动作、冲突校验与Linux GUI基本链已接，见 [keymap-source.md](./keymap-source.md)；通用执行目录与全终端矩阵仍未交付。

真实 WebKit 暴露 search 输入先用 Escape 清空内容，面板仍开、随后输入落入搜索框；关闭模态时还存在 Escape 跟随焦点落到 PTY 的风险。`workspace-modal-escape.ts` 在窗口捕获普通 Escape 的整个 down/repeat/up，先消费再调用现有 cancel 事件。取消仍由原控制器撤销，未处理的 dialog 才默认关闭；重复手势不能再取消新模态或漏到终端。普通终端 Escape、IME 和修饰键不重解释，页面退出拒旧控制。没有绕过原许可或提前释放后台信用。

## 可配置键位模型与当前产品边界

`ui/src/keybindings/{chords,conditions,definitions}.ts` 先提供投影元数据编译：128字符、最多2步/16绑定，固定已知 modifiers/keys；when仅5已知布尔字段的&&/!，拒绝任意表达式、重复／未知字段，不eval。定义只接受原7显示动作和palette，拒缺字段、继承/accessor、null条件与任意Capability IDs。同context的精确与序列前缀冲突拒绝；普通CtrlK前缀只有显式!terminalFocus可用，终端候选限modified app chord，不是Shell完整兼容证明；当前clipboard/reload保留，CtrlShiftP仅palette。

26模型测试、576flags/letters往返、32布尔状态与64条件对穷举通过。默认Palette predicate已复用公共 matcher，与旧真实窗口验过的函数在768键/flags/state组合下等价；695全TS和严格类型/格式通过，MCP仍Unknown。后续后端合并／JSON投影、固定文件CAS保存与取消安全IO已通过独立基础验证，详见 [keymap-source.md](./keymap-source.md)；后续已挂固定app_config_dir当前页read-confirm-save-reconfirm、序列runtime与Settings键位UI，749TS／134Native及受控Linux保存/禁用/恢复/焦点/外改冲突/同文件跨进程GUI重开通过；已存在按钮的后端许可不变，见来源文档的仍未验范围。

## 验证与未交付

26 个针对性反例、668 全 TypeScript、严格类型／格式通过；TS MCP 初始化失败为 Unknown，不计诊断通过。`mise run probe-workspace-commands` 在隔离 Linux X11／真实 WebView、私有 HOME 和固定 Profile 中验证三页同 PID：快捷键检索、Settings 合法读取、其它模态拒绝重开、空结果取消、三条 exact PTY 输入没有 CtrlP／搜索／Escape 混入、关闭默认保留／再次确认后的原生回收与顺序退出。真实系统剪贴板审批及 SSH 挂起握手取消兼容通过。显式 SSH 工厂补验 palette 的 connect 检索选择→原保存主机／身份对话框→密文 auth1、同 Root 换页、三 echo、尺寸入队与原生退出通过；默认工厂仍关闭。对应 `probe-workspace-ssh-session` 的 commands mode 加入 CI。实际像素与日志保存在 `target/probes/workspace-terminal-commands-*`。

这不是通用 Action/Capability 执行入口，也没有打开任意 Shell、CLI 命令、通用可执行键位目录、多语言、跨平台、辅助技术或系统 IME 验收；普通 CtrlP 的完整终端矩阵仍待。8s 阶段／80s 窗口、1s 绘制及授权门禁不变；原 Profile 列表迟到、TS/starter Unknown 和发布审计八项仍保留。
