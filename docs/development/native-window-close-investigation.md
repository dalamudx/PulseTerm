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
