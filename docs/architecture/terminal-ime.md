# 终端 IME 的应用层按键边界

系统组合文字和最终编码仍由锁定的 xterm SDK 处理。PulseTerm 不从 `compositionupdate`／`compositionend` 读取文本，不补发组合结果，不把候选阶段字符当作命令或粘贴。

## 组合生命周期

`CompositionGuard` 在当前根上捕获 `compositionstart`／`compositionend`，并在失焦、页面退出和生命周期取消时清除本地状态及监听。每个工作区窗口、终端 Surface 和命令面板搜索框各自持有守卫；状态不跨窗共享。

事件的 `isComposing`、兼容 WebKit 的 `keyCode === 229` 以及 `key === "Process"` 都表示按键归输入法所有。即使浏览器在候选阶段漏标单次按键，活动生命周期仍保护应用动作。结束后不设置延时或吞掉下一次普通按键；结束边界上的 229 事件仍由兼容标记保护。

工作区在组合开始时退休待决快捷键前缀，组合期间跳过快捷键、方向焦点、弹窗 Escape 和强制刷新处理。守卫不调用 preventDefault／stopPropagation，让事件继续交给浏览器和 xterm。命令面板自身也跳过候选导航和确认键，Surface 的选区 Ctrl+C 分支让路给 SDK；普通 Ctrl+C 和已有复制逻辑保持原语义。

## 输入与权限

文字只通过已有 `onData` 订阅进入有界 InputQueue 和当前后端输入授权。组合事件本身不创建发送任务，也不提升输入权限。Surface／页面失效后，旧订阅不能发出迟到文本；后台恢复仍遵循既有捕获、ACK 和输入使能流程。

## WebKit 直接提交的兼容边界

真实 IBus／libpinyin 取证显示，WebKit 可在没有 DOM `compositionstart` 的情况下，依次发送 229 按键、`insertFromComposition` 和未配对的 `compositionend`。锁定 SDK 的 229 文本框读取已负责该次提交；再处理孤立结束事件会在后续提交时重复发送。

`TerminalImeCommitGuard` 只在同一文本框的可信 229 按键及直接提交之后，阻止该未配对结束事件继续传播。正常成对组合、普通输入和未知事件形状仍交 SDK；新按键、keyup、失焦、不同目标、旧页和释放清除该状态。该保护不读取组合正文或文本框值、不按文本内容去重、不补发数据，也不改变已有输入授权。监听随 Surface 生命周期释放，与是否自动复制选区无关。

## 开发验证范围

七个直接相关 Vitest 文件共 79 项行为通过（分批运行，按文件去重），覆盖兼容标记、未标记候选键、失焦／页面退出、根隔离、监听清理、组合确认不执行命令、CJK／组合字符单次原样提交及旧视图拒绝。TypeScript、改动文件 Biome 和产品构建通过，保留既有 lint／打包警告。TypeScript Astrolabe 为 Unknown，不计通过；没有安装新工具、升级 SDK、运行全量回归或新增重型 CI 任务，现有 Vitest／类型／构建任务自动覆盖新增模块。

这些是应用层和模拟 SDK 输入验证。系统中日韩输入法、真实候选框跟随、缩放／跨屏 DPI、Linux WebKit 和其他平台的原生输入事件顺序仍须真实 WebView 验证，不计 TERM-04 整体验收完成。轻量开发证据保留在 `target/probes/ime-input-development/`。

## 真实 IBus／libpinyin 原生矩阵（拼音字节链已验收）

正式入口`mise run probe-workspace-ime`使用独立D-Bus／Xvfb、真实IBus和libpinyin引擎，经GTK输入上下文到生产WebView及真实PTY核对。只由XTest键入拼音和候选选择键，不派发DOM组合事件、不直接注入中文。预验证已在原生GTK输入框显示候选并提交“中文”，不将它计作终端验收。正式矩阵检查候选期间零PTY输入、Escape取消零输入、空格／数字选词各单次提交、刷新零重复、恢复普通Return，以及PTY、GUI、IBus子进程实际退出。

CI 安装 IBus／ibus-gtk3／libpinyin 运行依赖；本机通过 `PULSE_IME_PACKAGE_ROOT` 指定 `/data/temp` 内解压包，使用 bubblewrap 只读叠加 `/usr`，配置、词库学习数据和临时 socket 写入私有目录。两种路径使用同一正式场景。普通生产构建已通过候选零输入、取消、两种选词各单次提交、刷新零重复和精确 `中文中文\r`，GUI／IBus 退出码为 0，原生子进程回收。完整 1543 项前端回归、类型／双语／格式及 Python 检查通过。最终证据见 `target/probes/workspace-ime/accepted/f12d77d0/`。

## Linux IBus 原生预编辑

锁定 wry 0.57.0 会为兼容 Fcitx 关闭 WebKit 预编辑。在已明确选择 IBus 的环境，该选择令候选停在原点：GTK／IBus 客户端的光标坐标正确，候选面板仍向候选窗口请求 `(0,0)`。普通 GTK 输入框启用预编辑时能正确定位。

生产工作区首窗和后续窗口均在 wry 完成配置后，通过 `with_webview` 在 GTK 主线程设置现有输入上下文。非空 `GTK_IM_MODULE` 优先，仅精确值 `ibus` 启用预编辑；没有非空覆盖时才读取 GTK 的输入模块选择。Fcitx、未知或无效编码模块均保持原设置，其他平台不编译此适配。它不替换输入上下文，不修改输入目的／提示，不读取或重发文本。未来 wry 改变该行为时，应重新核对本适配，避免保留过期兼容。

正式位置／字节矩阵已通过三个候选、移动窗口后的坐标、精确 `中文中文\r`、刷新零重复和实际进程退出。四项策略单测覆盖环境优先级、其他模块、GTK 后备和无效编码。原始证据见 `target/probes/workspace-ime/accepted/d9780bb5/`。

候选正文采用 AT-SPI 的实际 SHOWING 判断；窗口可见性另行记录。取消后的翻页箭头空壳在独立 GTK 也复现，仍未解决。`probe-workspace-ime` 成功只代表位置／字节合同，收据保留 `cancelCandidateWindowHidden=false`；`probe-workspace-ime-popup` 强制要求窗口隐藏并继续失败。真实桌面、多屏／DPI、其他引擎、Fcitx 原生复验和其他平台不在本次通过范围。
