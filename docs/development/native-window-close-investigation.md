# Linux 原生窗口关闭调查

## 当前证据

保存主机别名功能链的最终八模式真实窗口矩阵、1050 项 Rust、1197 项前端和基础探针通过；305 份不可变 SHA 证据见 `target/probes/saved-alias-acceptance.json`。较早一次三跳冷别名窗口在锁定取消后关闭时，GUI 主进程退出码为 -11（SIGSEGV）；此时尚无网络认证或 Agent 签名。历史非零关闭退出也已保留。四次本任务 GUI 的 GDB 关闭观测正常退出，没有捕获故障栈，不能证明该间歇故障已修复。

当前调查未修改生产关闭逻辑，也未将故障归因于 WebKit、GTK 或应用。原有退出屏障同步撤销页面权限、禁止新的工作，并在独立清理任务确认 CLI、SQL、Profile I/O 和 native 会话物理退出后再次请求退出。源码中的调用关系与实际崩溃原因必须分别验证。

## 有界复现

```sh
mise run probe-workspace-native-close
PULSE_COLD_CLOSE_CYCLES=12 PULSE_COLD_CLOSE_GDB=1 mise run probe-workspace-native-close
PULSE_COLD_CLOSE_PHASE=cancel-unobserved PULSE_COLD_CLOSE_FAILURE_GDB=1 mise run probe-workspace-native-close
PULSE_COLD_CLOSE_PHASE=pending-dialog PULSE_COLD_CLOSE_FAILURE_GDB=1 mise run probe-workspace-native-close
```

正式任务在 `/data/temp` 创建独立 TMPDIR／TMP／TEMP、前端资产和原生助手，退出时回收本任务夹具；调用者可提供 CARGO_TARGET_DIR 以复用构建，验证链结束后再执行 clean-dev。默认分别执行12个锁定与解锁后取消场景，循环上限32；状态只接受locked/unlocked。`PULSE_COLD_CLOSE_PHASE` 只接受 `settled`（原流程）、`cancel-unobserved`（取消后不等CLI回执便关窗）、`pending-dialog`（仍有未决确认框时关窗）。各模式使用独立收据及窗口日志，不批准精确目标、指纹、SSH凭据使用或输入。

每轮明确取消 CLI 待确认意图，核对认证和 Agent 签名为零、SQL 来源摘要不变，再通过窗口管理器 Alt+F4 关闭。GUI 主进程必须由本任务 subreaper 实际回收且退出 0，control.sock/control.token 不得残留。非零退出、超时或未知回收状态均失败。成功循环逐轮保存 PID、退出码与端点回收收据；正常与 GDB 观测结果文件独立，不将观测改变的时序当作修复。

GDB 仅附加到本任务实际 GUI PID，禁止 debuginfod 自动下载；打印参数关闭，不读取局部变量、堆、寄存器中的值或 core 内存。符号栈与 shared-library 映射用于定位 native 关闭。runner 禁止 core dump，避免保存 Vault 进程内存。GDB、CLI、GUI、fixture、转发 peer 及可访问性子进程由现有 finally 路径回收；最终归档和全部退出后才能执行 clean-dev，保留 target/probes。

## 结果与通过边界

本轮语法与生产特性构建通过。正常关闭观测先为锁定／解锁各 12 轮，再分别扩展 32 轮，共 88 轮；GDB 符号观测为各 4 轮，共 8 轮。96 个 GUI PID 已去重核验，逐轮退出 0、零认证／签名／写入及 control.sock/control.token 回收均正常；8 份对应 PID 的 GDB 日志均有附加就绪和 exited normally，没有新的 SIGSEGV 或故障栈。实际 `--lib product_workspace::exit::tests` 的 3 项通过，覆盖真实 PTY／数据库回收、并发只启动一个 supervisor，以及只有物理完成才能允许退出；首个 bin 零测试调用另存且不计通过。最终 254 份 SHA 证据见 `target/probes/native-close-investigation.json`，链接此前 129 份不可变批次，历史崩溃证据保留。全部构建、GUI、fixture、GDB、native 测试退出后 clean-dev 回收 9.9 GiB，本批临时目录清空。SDK 源核对为 tauri/runtime-wry 2.12.1、wry 0.57.0：最后窗口销毁后触发 ExitRequested，AppHandle.exit 通过 runtime handle 请求退出；这个顺序不能说明历史崩溃根因。生产关闭实现未改，间歇故障仍未定位，不计修复。正常与 GDB 结果分别存于 `target/probes/native-close-{locked,unlocked}-{normal,gdb}-result.json`。

有限重复未重现只能证明该批场景正常退出，不等于可靠性问题已解决。只有取得有效故障定位、实现有依据的修复、保持退出授权与物理回收屏障，并通过针对性 native 生命周期和真实关闭验证后，才能更新修复状态。跨平台、外网、硬件 Agent 和发行审计不在本调查的通过范围。

## 未决关闭时序增量调查

最新生产构建的原流程锁定／解锁各12轮、共24个GUI均退出0，零认证／签名、来源摘要不变及端点排空。未据此关闭历史SIGSEGV。

增加两种有区分的关闭时序：取消后不等待CLI回执，以及确认框仍未决时直接关窗。新模式的收据在首轮启动前写为未通过，按已完成轮数更新，避免早期失败留下旧通过状态；文件名包含关闭时序，保留原流程默认路径。首次cancel-unobserved的前9轮正常，第10轮在原八秒初始页期限内仍为黑窗，尚未执行关闭；该轮收据／日志／截图保留在 `target/probes/native-close-attempts/ca91e775/`，没有将其归为关闭崩溃，也未放宽期限。

可选 `PULSE_COLD_CLOSE_FAILURE_GDB=1` 只在关闭诊断失败时附加本任务GUI，最多等待30秒，记录有限符号栈和库映射后分离；禁用debuginfod和参数打印，不输出局部变量、堆、寄存器或core。取栈失败继续执行原有进程清理，原失败不会因此变成通过。最终通过矩阵未触发失败取栈。

两种新时序与锁定／解锁交叉各12轮，共48个GUI退出0；与本批原流程24轮合计72个不同PID，其实际waitpid退出文件已逐一核对。未决分支必须先发关窗、再取得CLI拒绝回执，使GUI可由subreaper读取真实退出状态；回执等待计入原12秒期限。未修改生产关闭代码。六组收据、72份退出文件及SHA256清单单独保存在 `target/probes/native-close-pending-acceptance/962d5f04/`，避免后续循环覆盖。窗口管理器准备失败和探针ChildProcessError分别定位，初始黑窗及历史SIGSEGV仍未定位，不据本批通过关闭问题。

## 固定阶段启动取证（工具验收）

历史认证前失败收据显示无可定位窗口／控制端点，主线程仍在GTK事件循环；现有符号栈不足以判断是窗口构建、页面撤销还是端点发布阶段。新增显式feature `workspace-startup-diagnostics`，仅Linux诊断构建读取 `PULSE_WORKSPACE_STARTUP_TRACE_DIR`。正常构建中的钩子为空操作，不读取该变量或创建文件。记录只含版本、GUI PID、相对微秒和固定阶段码，不记录URL、参数、页面nonce、账户、终端内容、密码或回答。

目录须预先存在、绝对路径、通过既有Unix私有目录准入。每进程以0600独占新建 `startup-<pid>.jsonl`，拒绝已有文件／符号链接，最多64条；失败不改变应用启动决定。目录准入沿用既有路径检查边界，不声明完整TOCTOU保护。阶段覆盖服务初始化、主窗口构建、页面分类／拒绝、nonce注入、可见性、窗口／CLI端点发布、Ready与物理销毁／退出。记录时间从诊断记录器创建起算，不能替代启动性能基线，诊断I/O也会改变时序。

```sh
# 调用者创建并拥有/data/temp下的独占0700目录，结束后自行归档和回收。
PULSE_STARTUP_DIAGNOSTICS=1 PULSE_WORKSPACE_STARTUP_TRACE_DIR="$owned_trace_dir" \
  PULSE_COLD_CLOSE_PHASE=pending-dialog PULSE_COLD_CLOSE_FAILURE_GDB=1 \
  mise run probe-workspace-native-close
```

正式关闭任务的八秒启动／十二秒关闭期限、零认证／签名、来源不变和实际退出要求保持原值；启用阶段记录后，成功矩阵还将调用 `scripts/check-workspace-startup-traces.py` 关联全部锁定／解锁GUI PID。校验器拒绝额外内容字段、非私有／链接文件、非单调或超限记录、缺失端点／退出、失败阶段和陈旧PID集合；默认当前收据位于 `target/probes/workspace-startup-trace/current/`，接受后另行不可变归档。失败矩阵的任务自有原始阶段文件和既有失败取栈仍须保留。这里的阶段取证只配对normal观测；原有GDB循环独立保留。

`mise run check-startup-diagnostics` 的四项Rust记录器及五项收据回归通过；普通生产／诊断两组全目标严格Clippy通过，三项物理退出和一项原页拒绝回归实际命中通过。锁定／解锁未决关闭各12轮通过，24个不同GUI PID与648条固定阶段记录逐份关联；页面确认、可见性、Profile和CLI就绪完整，最后记录Exited，实际退出码全部0，零认证／签名、来源不变及端点回收均通过。收紧新增阶段为必需后，五项收据回归及同批原始轨迹再次通过。接受材料见 `target/probes/workspace-startup-trace/accepted/bd0768d4/`。失败记录独立保留，不将该批诊断通过计作历史启动黑窗或SIGSEGV修复。

首轮实际取证：四项Rust记录器回归、普通生产／诊断两组全目标严格Clippy通过。锁定第一轮在原八秒窗口期限失败，GUI 928701的阶段记录停在约1.802秒 `window_build_started`；此前服务、单实例所有者和私有数据目录准备均完成，未到 `window_built`、页面加载或控制端点发布。对应主线程符号栈显示GDK的`XInternAtom`／`_XReply`／`xcb_wait_for_reply64`等待，尚不足以归因应用、GTK、X11服务端或WebKit。原始JSONL、失败画面及栈归档在 `target/probes/workspace-startup-trace/attempts/4dab7e6e/`。失败取栈新增主线程48帧，其他线程仍各12帧，参数／局部值仍关闭，15秒取证期限不变；后续独立观测及最终通过矩阵见下文。

后续复测分开保存：GUI1012695约0.103秒进入构建、8.372秒建成并发布端点，17.357秒才出现PageStarted，初始邀请超时，未收集到该轮的新栈；旧928701栈不作为本轮证据。独立最小对照GUI1051030约0.292秒建窗、0.649秒发布端点、0.848秒开始加载、6.669秒完成本地页面／nonce注入，初始邀请仍未就绪；本轮截图实际显示初始化布局，主线程48帧栈在普通GTK循环，不能沿用X11等待结论。对应阶段／画面／实际失败分别归档于 `attempts/6b224af0/` 和 `attempts/control-third/`。

为区分页面确认未到达与初始化后续等待，增加固定检查点DocumentConfirmEntered／DocumentConfirmed、VisibilityRead、ProfilesRequested／ProfilesListed及CliPageReady；仍不记录参数、nonce、配置和页面内容，不修改授权、加载顺序或期限。扩充后的四项Rust回归、两种严格构建及24轮原生矩阵通过。首轮定位为构建等待，其他轮为页面／初始化等待，均尚未取得根因或修复证据。

接受与失败证据独立归档后，确认本任务构建、原生窗口、GDB及夹具均已退出，执行 `mise run clean-dev` 清除13.4GiB开发产物并移除 `/data/temp/alpha-startup-trace.NIJr5h`；保留target/probes全部接受和失败材料。正式轻量检查自建／回收独占TMPDIR，Linux以外显式拒绝执行，不以零测试通过。最终摘要清单随接受目录保存。

## 页面确认的有限等待与恢复

`confirmWorkspaceDocument` 与复用它的 CLI 展示回执曾直接等待原生 Promise，后端不返回时不会结束，页面撤销也不能立即完成该等待。现限定每次等待五秒，从调用时的单调时钟起算；结果到达时再次检查期限，避免阻塞事件循环后把过期结果当成成功。页面撤销立即返回取消，成功、拒绝、取消或超时均清理本次监听和定时器。原生调用仍自行完成，不自动重发、不回滚已完成工作，也不提前归还后端资源。

页面确认失败或超时显示“页面验证未完成 · 请刷新工作区重试”及对应英文；使用现有重新加载按钮。超时不继续加载 Profile 或启动终端，迟到成功／拒绝均不能改变该结果。CLI展示元数据沿用现有未确认提示，刷新后使用新文档生命周期。后端窗口／来源／nonce／Vault检查和原八秒启动、十二秒关闭期限不变；其他初始化步骤与原生窗口构建故障不在此修复范围。

新增五项反例在旧实现上实际失败；修复后62项Readiness／冷SSH／CLI新标签／可见性回归、TypeScript、Biome、双语检查及生产前端构建通过。独立Chrome载入实际工作区并受控阻塞确认RPC，约5秒显示刷新建议，中英文状态提示在800px窗口内未溢出；迟到回执未触发Profile或后续原生调用，真实点击重新加载后只发一次新确认，Chrome与Vite退出。该检查不是原生WebView故障注入、系统输入法或辅助技术验收；启动黑窗与SIGSEGV根因继续开放。沿用现有前端CI，不新增认证矩阵。
