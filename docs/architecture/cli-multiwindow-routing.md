# CLI 多窗口路由

CLI-01 的 Linux 本地工作区多窗口限定验收批次已完成：真实 WebView、合并回归、SHA 证据归档与清理通过。SSH 跨窗实际认证、跨平台验证及历史原生关闭 SIGSEGV 尚未解决。

Windows 显式 `workspace-cli` 的暖传输已接线，复用以下窗口路由与原页批准；原生运行未验收。平台路径、账户与句柄边界见[Windows CLI 控制](windows-cli-control.md)。Windows 不具备本文件历史 Linux 批次的冷启动或实窗通过证据。

`pulse windows` 仅列出已发布窗口标签，采用 Global／Safe 空参数 Capability。`pulse -w <标签> <动作>` 精确选择后端窗口；省略选择器只在恰好一个已发布窗口时有效，多窗时拒绝猜测焦点窗，未知或已关闭标签不回退。选择器在动作准入前剥离，窗口定位不授予 Capability、Vault、会话或凭据权限。

暖启动 `pulse [-w <原窗标签>] new-window` 在原窗独立确认后只创建一个空工作区，返回后端标签。它不继承终端、Vault 或身份批准，新窗启动终端仍需自己的批准。取消、CLI 断连或原页导航不创建窗口。冷传输不支持 new-window、windows 或按未知标签创建替代窗；冷选择器只接受并标准化主窗 workspace。

原生目录自己创建 Registry owner 并安装到同一 ProductDocuments；外来数值 owner 不能登记。首窗标签是 workspace，后续是 workspace-<canonical v4 UUID>。主窗标签在同进程不复用。实际 builder、上下文提取和每个命令均检查精确原生标签、本地 workspace.html URL 与原页 nonce。成功 build 才发布，未发布或未确认页面不能路由；异步工作后重新验证原窗、原页与来源。元数据、Vault、SSH 配置等命令已移除仅接受主窗标签的旧检查。

应用持有 Registry、ProductDocuments、SessionRoot、运行时、CLI 服务、共享 SQL、设置 I/O、退出屏障及窗口监督器。各窗持有独立 PageVaultOwner、CLI／配置确认银行、终端视图和 ProfileReader。状态从实际 Invoke WebView 提取，前端字段不能挑选状态，异步操作持有本窗 owned 状态。回答、通知、取消、导航和关闭均限定在本窗。

共享 Root 独立持有会话。Registry::owned_page_sessions 在同一锁内校验精确原页并筛选当前窗口 owner；列表不授予连接、关闭或转移权限，空窗不恢复其他窗会话，刷新保留本窗会话。显式转移仍通过原有 Registry 授权。

ProfileStorage 共享同一 SQL worker、应用级单 I/O 额度和物理完成回执；各 ProfileReader 仅撤销本窗票据与等待。取消原生等待者仍由实际操作持有额度，列表保留原五秒总排队预算。单窗关闭不取消共享 worker；应用退出等待全部窗及存储监督器。SettingsIo 同样共享目录、单额度和物理完成回执，设置快照冲突仍返回保存未确认，须重读并重新批准。快捷键下层已有进程级单原生额度。

目录容量为 32，包含创建和关闭中的窗口。构建错误不是原生 Destroyed 证据，不自动重试，也不提前释放容量。目录槽必须同时取得精确窗口的 Destroyed 和实际 I/O 成功回执；取消等待者或单项完成均不能释放。独立监督任务持有上下文到本窗操作退出；失败回执保留，不能升级为退出许可。

应用退出先撤销所有窗，再等待全部窗口及共享服务；一窗失败不跳过其他窗。主动退出先关闭已登记原生窗，避免 Destroyed 与退出许可相互等待。最后一窗关闭后才回收共享 CLI 端点、Root 与 SQL。poisoned 状态仅用于撤销已知资源，不能接纳或释放未知资源。目录锁顺序为 directory → ProductDocuments → Registry，禁止持锁 I/O 或 await。

本轮已核验类型检查、32 项前端、6 项原生生命周期、8 项 Capability、21 项 CLI 解析及严格检查。全部 desktop CLI／SSH 库测试为 287 通过、5 忽略，忽略项未计通过。冷 split／SSH 元数据反例已校正为等待原页确认并在换页后失效，冷 new-window 与任意效果参数仍被拒绝。

真实 Linux 同进程探针通过：取消／断连／导航零创建；三个后端标签；新窗正常空主机树且没有外窗会话；多窗省略选择、外窗 session 与退役标签拒绝；两个独立 PTY 输入；两窗刷新保持 PID；主窗先关闭后另一窗继续输入并创建空窗；共享端点保持到最后一窗关闭；GUI、PTY 和端点最终退出。探针逐窗限定原生可访问性 frame，输入只发送一次。注册任务 mise run probe-workspace-cli-multiwindow 和 Linux CI 使用同一入口，最终注册任务及七命令链／六冷分屏兼容全部通过。默认 mise run check/test/probe 核验 1063 项 Rust、1208 项前端，7 项忽略未计通过；Rust 四线程且未放宽断言，不据此声称默认并发稳定性已解决；23 项前端警告和 4 infos 保留。审计退出 5，仍有 8 项许可证、2 项维护策略阻断，Alpha 门未关闭。SHA 复核归档见 target/probes/cli-multiwindow-acceptance.json，历史失败与当前证明分列；全进程退出后 mise run clean-dev 回收 25.2 GiB，保留 target/probes 及不可变来源快照。本轮任务临时目录已清理，其他项目目录未触及。

先前目录、共享服务、全窗监督、原页及退出反例和单窗兼容均已通过，但它们不代替 SSH 跨窗认证、IME、跨平台或关闭稳定性验收。历史 SIGSEGV 保留为未复现且未解决；额外无 SSH 窗口严格构建两项占位接口警告、设置 LSP 诊断超时及发行审计阻断继续保留，未降低警告或安装工具。


SSH 跨窗扩展已完成密码、软件 Agent 及密码外跳→Agent 中间／叶端三跳的 Linux 同进程实窗验证：A 解锁不解锁 B；B 取消、CLI 断连或换页使旧请求失效，零认证、零签名、零写入；A 锁定时 B 的独立批准仍可完成。SSH 会话和提示只在 B，A 不能选择 B 的会话；B 刷新不重新认证，先关 A 后 B 继续原文输入；最后 GUI、端点、实际认证连接及父转发流退出。Agent 只使用所选第二把钥匙，直连签一次、三跳中间与叶端各一次；三跳核对每个实际指纹和当前 SQL 地址／端口，批准后只新增两个 jump pin，其他连接及凭据元数据不变。仅覆盖受控本地软件服务，不扩展为系统 Agent、硬件、远端网络或跨平台验收。

实测发现并修复 CLI 路线呈现遗漏会话目录刷新：原生 selection admission 后读取本窗 owner 过滤目录，要求目标仍在列表中且页面／取消／弹窗仍有效，才选择终端；不新增认证、重连或自动重试。探针通过现有命令面板打开每窗 Vault，避免长主机树使侧栏按钮离屏；刷新撤旧页操作而保留已接受密钥，未虚构自动锁库。1208 项前端测试和类型／格式检查已过，原有 23 warnings／4 infos 保留；Astrolabe TS 诊断 Unknown，不计通过且未安装工具。新增注册任务 `mise run probe-workspace-cli-multiwindow-ssh` 和 Linux CI；本轮注册三模式、原冷 SSH 三模式、本地多窗口、七个命令链和六个冷分屏场景均通过。默认 `check/test/probe` 核验 1063 Rust／1208 前端通过、7 ignored 不计通过；Rust 用 4 线程且原断言不变，不宣称默认并发稳定性已解决。审计 exit 5：8 项许可证和 2 项维护策略阻断保持开放。最终 343 份 SHA 归档见 `target/probes/cli-multiwindow-ssh-acceptance.json`，包含修复前目录缺失、离屏 Vault 选择器、刷新后自动锁库误预期及注册入口特性名错误的历史收据；冷分屏 X11 观察器 BadWindow stderr 原样保留，其后正常完成六项生命周期断言，不作为历史 SIGSEGV 的修复证据。所有记录 GUI／服务 PID、运行目录及项目 native 进程退出后，`clean-dev` 回收 12.4GiB，本批独占临时目录清空。扩展验收在 Linux 本地软件服务范围关闭，跨平台、系统／硬件 Agent、远端网络、历史关闭崩溃及 Alpha 发布阻断继续保留。
