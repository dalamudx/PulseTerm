# Windows CLI 控制传输

状态：显式 `workspace-cli` 已接线，Windows 原生执行与 GUI 退出条件未验收。普通 `workspace` 和现有本地验证包范围不变。本文不把 Linux 运行、GNU 交叉检查或 CI 注册视作 Windows 通过。

## 端点与身份

GUI 与 `pulse` 从系统 `FOLDERID_LocalAppData` 解析固定应用 `dev.pulseterm.workspace-preview/cli`，不接受页面、命令行或环境变量提供的管道路径。该目录只提供路径元数据，必须再通过生产私有目录逐层准入；不修复既有权限或删除不确定的旧端点。

本机管道名由当前账户 SID、最终目录的 volume／file identity 派生。监听采用 owner／SYSTEM 的受保护 ACL，不覆盖已有管道。发布前核验锁定单实例插件的精确隐藏窗口确实属于本进程；此证明不是 CLI 或 GUI 动作授权。

每个实例生成独立的 64 字节十六进制 hello token，使用私有 `control.token` 创建而不覆盖。token 不实现 Debug／serde，不投影到页面，客户端副本使用 Zeroizing。客户端验证私有 token，再以非继承、OVERLAPPED、IDENTIFICATION QOS 打开固定本机管道；忙时异步重试，完整连接／认证总期限五秒。失败不启动 GUI，不修复旧文件，不回退到其他管道。两端通过内核 peer PID 的只读进程 token 查询检查账户；这不是二进制身份、远端认证或 Capability 批准。

## 分层与生命周期

`pulse-platform` 只封装 Windows 目录身份、进程账户、非阻塞句柄及按身份删除原文件的原语。桌面 `native_cli_windows.rs` 实现目录／ACL／peer／token 的受信任适配。`pulse-app` 不依赖平台层，只持有有界协议、认证连接、`NativeEndpoint` 生命周期接口和监督任务。该接口只能由受信任的后端提供，不能从页面传入路径、token、句柄或授权回调。

共享协议保留 8 个连接、帧长与请求数上限、相关 ID、重放防护、独立 Capability 和原页批准。监听关闭先撤销操作，再等待所有 handler 实际结束，最后回收本实例 token。取消 shutdown 等待者不会终止监督任务或提前释放文件／目录租约。文件被替换时返回冲突，绝不删除替换者；崩溃留下的旧文件拒绝自动修复。

Unix 适配为 Linux／macOS 共用同一协议：私有目录、内核对端有效 UID、hello token 与身份匹配后清理规则相同。macOS 在已准入的私有父目录内创建 socket，以不跟随符号链接的方式设置 0600 并复核 inode，再发布 token；不修改进程 umask，不修复旧端点，也不承诺消除同账户文件替换的全部竞争。11 项既有协议用例已在 Linux 与 macOS 原生通过（CI `39951225330`）。

`Server::prepare` 在创建窗口前排他预留同一端点，但不开始认证或执行 handler；`PreparedServer::start` 消费该租约并进入既有监督循环。未激活时放弃租约会关闭 listener，再按身份清理 socket／token；替换者保留。客户端仍受原有握手期限约束。含两项新增生命周期回归的 13 项协议用例已在 Linux／macOS 原生通过（CI `39954848476`）；这不是 GUI 验收。

macOS 显式 `workspace-cli` 构建从 Darwin `confstr(_CS_DARWIN_USER_TEMP_DIR)` 读取当前账户的系统临时根，解析系统 `/var` 别名后选择固定子目录 `pulseterm-workspace-cli`；不使用 HOME／TMPDIR／XDG 覆盖值。系统查询只返回元数据，原有逐层目录、所有者、权限与文件身份准入仍必需。GUI 在创建 WebView 前预留端点，窗口发布后才激活服务，并纳入共享 CLI／原生退出监督。第二实例拒绝，不启用上游 macOS 公共路径单实例插件，不删除旧端点或转发参数。客户端复用 Unix 冷启动实现：已有端点只尝试一次认证，失败不冷回退；端点确实缺失时，仅启动 `pulse` 同目录的固定 GUI 同伴，使用相同的程序所有权、祖先目录、链接数、setid 和写权限拒绝规则。八秒内等待发布后，以同一认证连接认领原页面动作；并发第二实例不能转发或接管票据。仅支持同目录开发／安装布局，不通过 LaunchServices、PATH 或任意 `.app` 搜索程序。暖 GUI、原页批准、目录交接和物理退出仍待 macOS 实窗验收。
macOS 原生 CI `40011009741` 已通过 Darwin 目录查询、13 项协议／端点预留、两种 CLI 完整目标检查、CLI＋SSH 严格编译，以及 332 项产品和 34 项 CLI 回归；4 项既有忽略用例未计入通过。生产与验证 Context 分属独立二进制后重复 Info.plist 已消除；真实 PTY 夹具使用 `/usr/bin/true`，不修改生产程序选择。上述为原生组件／编译证据，不替代 GUI 冷暖启动与物理退出验收。
验证宿主与生产工作区分别在自己的二进制入口调用 `generate_context!`，再把 Context 交给各自库入口。macOS 开发构建会嵌入进程级 Info.plist，不能在同一库中生成两份；不以 `test = true` 关闭生产元数据嵌入。验证入口仍用根配置，生产入口仍用 `workspace/tauri.conf.json`，并在启动前拒绝继承验证窗口、身份、CSP 或 capabilities；命令注册与资源路径保持独立。
Windows 的控制流关闭显式标记为不执行依赖的后台 flush。正常成功仍要求调用方实际收到协议收据；取消可丢弃未确认写入。所有接受和连接的 stream 都通过同一关闭包装，避免未知 peer、认证失败或取消路径把句柄留在游离的 FlushFileBuffers 工作线程。它不改变终端流、SSH 或其他库的关闭行为。

## 产品范围

暖入口复用已有窗口目录、原页批准、Vault、SSH peer 检查及物理退出监督。`windows` 只读已发布标签，`profiles` 只返回稳定身份；Windows `pwsh`／`powershell`／`cmd`／`wsl` 与 Unix 身份采用同一封闭解析器，不返回程序路径。多窗口需要精确选择，标签不能升级为页面、会话或凭据权限。

单步和至多八步的命令链持有同一认证连接，链内失败不重新连接；新终端／分屏／SSH 继续等待原窗的明确批准及呈现收据。现有暖端点认证失败、部分 namespace 或陈旧 token 均拒绝，不冷回退。SSH 配置元数据导入已单独接线，详见[来源边界](ssh-config-import.md)。Windows 实际冷暖 GUI／CLI 退出条件尚未验收。

### 固定同伴冷拉起

仅在逐层校验后的缺失端点允许首步冷启动；预检不创建目录、不读取 token 字节、不连接管道。完整目录必须保持私有 owner／ACL；token 与本机管道只存在一个时拒绝，既有不可信状态不删除／修复。

目标固定为当前 `pulse.exe` 同目录的 `pulseterm-workspace.exe`，不查 PATH、不接受客户端目标或通用 argv。路径必须位于本机 ACL 文件系统，逐层拒绝重解析点并持有目录身份；程序为单链接普通文件，原映像只读句柄禁止正常写入、删除与改名，持有至 CreateProcess 返回。安装目录及程序的独立 ACL 检查只允许其他账户读取／执行，不沿用目录祖先允许新增成员的权限掩码；保留 SYSTEM／Administrators／TrustedInstaller 和仅继承的 Creator Owner 安装规则。可读的系统安装目录不要求改成用户私有目录，现有 ACL 不被修改。这些检查不承诺对同账户或特权进程的原子 ACL／全部 DLL 保护。

仅传现有 4096 字节上限的 startup parcel（相关 ID 与保存实体元数据），固定工作目录为 OS 系统目录，标准输入输出为空；不传凭据、路径或执行权限。首实例可接收 parcel，八秒内只观察发布，再用同一认证传输认领 `workspace.bootstrap`；原页、冻结来源、Vault 代际、Capability、明确批准和完成收据全部保留。Windows 二实例仍拒绝任何启动参数转发；并发冷起不确定时失败，不改走自动重试或暖执行。CLI 失败不杀死已经独立运行的 GUI，也不自动执行待批准动作。

## 检查与原生验收

`mise run test-cli-transport` 先运行执行器反例，再按平台执行组件：Linux／macOS 13 项真实控制协议，Windows 同源 `xtask --features windows-cli-probe` 的 3 项管道及 `pulse-platform` 的 4 项冷启动租约夹具。各组通过共享校验器逐项核对完整名称，拒绝同数错名、重复、缺行、零／部分、忽略／测量、非零／未知退出及额外摘要。夹具不启动 GUI，不代替实际冷启动验收。

Windows 原生用例分别覆盖 hello 不执行操作与 namespace 重开、错误／陈旧 token、取消 shutdown 等待者后原生任务和 token 保留。`pulse-platform` 另有 3 项目录 namespace／账户查询、token 排他创建／读取及替换不删除用例。CI `39951225330` 的 Windows 传输收据确认 3 项管道和 4 项冷启动底层用例原生通过；GUI 和普通用户端到端验收仍待完成。

本批 Linux 共享路径四项实窗已完成：focus／profiles 冷起后各三次暖聚焦且零 PTY；原页批准一个新终端；多窗取消／断连／导航零创建、精确标签与外窗会话拒绝、两条独立输入／刷新保留 PID、主窗先关及最后窗口关闭后端点回收。四个 GUI 和三个 PTY 均实际退出。它们只证明本地 Linux 共享路径兼容，不证明 Windows 命名管道或实际账户行为。

实测还修复了开发构建的资源缓存依赖：build.rs 显式跟踪前端产物目录与工作区配置，避免生成 workspace.html 后旧 CLI／SSH 特性组合仍使用空资源。再次运行真实 Vite 构建触发同组合重编译，二进制 SHA 与已测候选完全一致。首次 X11 观察超时、前端未生成和旧资源缓存三组失败原样保留，未增加等待期限或页面权限。证据与清理见 `target/probes/windows-cli/verification-v1.json`。

Windows 完整桌面全部目标和原生夹具的交叉检查只证明可编译。原生退出条件仍需要普通用户 MSVC runner 与真实窗口：同账户／其他账户、缺失／陈旧端点、默认／精确窗口、多窗拒绝、页面刷新与原窗关闭、批准／拒绝与 CLI 断连、持续操作后的 token／管道／GUI／PTY／SSH 实际退出。不得把 Linux 的同一共享协议或历史多窗口成绩外推为这些项目通过。
