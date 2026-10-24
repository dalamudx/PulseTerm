# 终端剪贴板授权

`terminal.copy_on_select` 与 `terminal.right_click_paste` 均默认关闭，沿用 User／Group／Host 稀疏设置、来源投射和单字段快照 CAS；恢复继承只删除相应覆盖。Settings 修改在下次打开或重新加载终端时应用，不关闭已有会话。

选中复制只处理当前 Surface 的真实主按钮手势，失焦、页面隐藏、焦点转移或 scope 退休清除手势。复用原 `text/plain` 复制事件和 4 MiB 显示选区预算。远端 OSC 52、脚本事件和单纯终端内容变化不能写入系统剪贴板。

右键粘贴只在已启用偏好、当前可输入 Surface、可见页面及真实右键事件中开始。终端处于鼠标报告模式时保留原报告行为，不读取剪贴板。读取结果进入原 PasteGate：最多 1012 UTF-8 字节，C0／C1 不支持项拒绝，多行仍须原页面和原视图的独立批准。读取本身不授权提交，输入仍须后端 verified Commit ACK 和 capture 确认。

三平台产品复用 `workspace_terminal_clipboard_prepare/read/cancel`。准备固定原 WindowToken、page nonce、connection、已确认输入及五秒期限；随机一次性票据既不能换连接也不能重用。Linux 使用异步 GTK `request_text`；macOS 在主线程使用 AppKit `NSPasteboard`，只请求 `public.utf8-plain-text`，检查原生数据长度后才复制并严格解码 UTF-8。Windows 在窗口线程只请求 `CF_UNICODETEXT`，按实际内存大小和字节预算约束 UTF-16 扫描，要求 NUL 终止与严格 UTF-16，并在转换后核对1012字节 UTF-8 上限；不重试系统剪贴板占用，不修改或释放系统持有的数据。实际读取前后都重验原连接、输入授权、窗口焦点和当前页面。换页、resize、取消、关闭、过期或无法核实的状态拒绝晚到文本；命令只返回该次明确读取的纯文本，不扩大 OSC 52 权限。

原生读全局单槽的 OwnedSemaphorePermit 由真正的 GTK 回调或 macOS／Windows 窗口线程读取闭包持有。前端超时或等待者取消可以撤销发布，不能提前释放额度；应用退出 barrier 等待实际读取结束。GTK 请求、AppKit 和 Win32 原生读取都不提供这里可用的同步取消；期限不等于资源退出证明。系统提供者可能先分配原生文本，1012字节检查限制返回和后续复制，不主张限制系统自身的分配。Win32 内存锁和打开的剪贴板在原生作用域结束时归还，关闭失败不发布文本。独立浏览器 Surface 的权限策略不能代替平台产品验收。macOS 私有命名剪贴板夹具已在原生 CI 通过，未访问用户的通用剪贴板；Windows 无授权拒绝和 UTF-16 边界两项用例已在 MSVC 原生 CI 通过，完整桌面检查／构建和22项原生前置亦通过；这两项用例没有从系统剪贴板读取内容。两平台真实右键、多行批准、取消／刷新和退出使用链继续独立验收。

当前 WebKit 在实际权限信号之前拒绝 Clipboard API 读取，未采用的全局 JavaScript 访问或单次 WebKit 许可候选均不启用。失败阶段只留固定状态证据，不记录剪贴板文本、票据或终端内容。

右键报告／PTY 行清除暴露的 DOM 误判已单独修复：xterm `trimRight` 保留实际写下的 ASCII 尾空格，DOM 与 buffer 比较须同样去掉可见尾部 ASCII 空格，并归一化 DOM NBSP。全角空格、内部空格、控制字符和非空白内容仍检查。解析、绘制 ACK、精确恢复与一秒确认期限不改变，没有重放输入或放宽永久不一致。

验收用 `mise run probe-workspace-right-click-paste`，在独立 Xvfb、私有 Profile 和 GTK 所有者中检查真实系统文本、原页批准／取消、刷新退休、鼠标报告优先、迟到 X11 回调拒绝和退出等待。Linux 固定夹具的局部 OCR 与勾选像素门仅作实际窗口观察，不是跨平台结论；证据保留在 `target/probes/`。

真实 Bash／GNU Readline 矩阵另用 `mise run probe-workspace-bracketed-paste`。在私有 rcfile 中的固定 Ctrl-X／Ctrl-B 回调只供测试观察应用自身 READLINE_LINE；用户输入仍由真实键盘和 GTK 剪贴板走生产审批／输入门，未添加应用缓冲读取的产品命令。开启 bracketed paste 时批准多行后仍须应用中的 Enter；关闭模式后应用按自身规则处理已批准行，不把批准解释为“不执行”。模式变化先等后端输出在真实窗口绘制，再刺激粘贴，不能用应用文件回执代表前端已解析模式。

UI 下一次正常粘贴只清本 Surface／页面／当前会话曾设置、仍未被后续状态覆盖的旧粘贴拒绝提示，恢复中性连接文案。它不表示原生写入或应用执行已完成；取消、退休与其它终端错误仍按各自路径显示。


Unix 关停必须先确认子进程退出，再释放输入 writer。锁定的 portable-pty 在 Unix writer Drop 时会注入换行与 VEOF；写线程接收队列关闭先于控制线程终止子进程时，此换行可能使 Readline 执行尚未 Enter 的已批准内容。写线程现在把 writer 返回给 JoinHandle，由控制线程在子进程退出与 I/O 回收边界释放；非 Unix 保持原有释放顺序。原生 completion 和根集合额度仍以真实退出为准。真实 Bash 反例要求关窗前的 READLINE_LINE 确实包含待决两行，关窗后应用执行收据仍严格只有原 13 条；右键延迟矩阵继续严格要求七行，不将 EOF 换行当作可忽略输入。


真实 Vim 应用矩阵由独立 `probe-workspace-vim` 提供：启动已安装的 Vim，明确指定私有配置、空文档与无用户插件／modeline／viminfo／swap／持久 undo／backup。固定 F6 映射只记录应用自身缓冲、模式与修改状态，用户粘贴仍走真实 GTK 和生产多行审批。验收要求启用自动缩进时保留原文本、插入缓冲跨页不重建原进程、一次撤销、精确保存字节，以及关闭未保存尾行不改变已保存文件；截图须等 Vim 收据计数真正绘出。Linux 公开实窗已通过六项应用行为，刷新／保存收据计数同帧画面已审阅；当前源码 Pyright 0 错误，Astrolabe 旧行诊断不计通过，不把中文字节测试称作系统 IME 或字体回退验证。

深历史复制候选由独立 `probe-workspace-history-copy` 验证：600 长逻辑行含中文／emoji／组合字符与软换行，在既有 Actor 最近历史和 Surface 保留上限内运行。物理跨滚动选区与用户 CtrlC 复用既有纯文本／4 MiB 复制门；GTK 回读只用于私有 Xvfb 的独立证据。每次复制前重设并回读哨兵，刷新后原进程／原源文本一致，复制不得写入 PTY。公开实窗结果待收，不能据此宣称任意深度或 4 MiB 极限、系统 IME 或跨平台已验。
