# Linux 原生窗口关闭调查

## 当前证据

保存主机别名功能链的最终八模式真实窗口矩阵、1050 项 Rust、1197 项前端和基础探针通过；305 份不可变 SHA 证据见 `target/probes/saved-alias-acceptance.json`。较早一次三跳冷别名窗口在锁定取消后关闭时，GUI 主进程退出码为 -11（SIGSEGV）；此时尚无网络认证或 Agent 签名。历史非零关闭退出也已保留。四次本任务 GUI 的 GDB 关闭观测正常退出，没有捕获故障栈，不能证明该间歇故障已修复。

当前调查未修改生产关闭逻辑，也未将故障归因于 WebKit、GTK 或应用。原有退出屏障同步撤销页面权限、禁止新的工作，并在独立清理任务确认 CLI、SQL、Profile I/O 和 native 会话物理退出后再次请求退出。源码中的调用关系与实际崩溃原因必须分别验证。

## 有界复现

```sh
mise run probe-workspace-native-close
PULSE_COLD_CLOSE_CYCLES=12 PULSE_COLD_CLOSE_GDB=1 mise run probe-workspace-native-close
```

调用者按开发约定提供 `/data/temp` 下本任务的 TMPDIR、TMP、TEMP，并在任务完成后回收该目录。任务重新构建 workspace-cli/workspace-ssh GUI、CLI 与现有本地固定 fixture，运行独立 Xvfb、D-Bus 和窗口管理器。默认分别执行 12 个锁定与解锁后取消场景，循环上限 32；状态只接受 locked/unlocked。不会批准精确目标、指纹、SSH 凭据使用或输入。

每轮明确取消 CLI 待确认意图，核对认证和 Agent 签名为零、SQL 来源摘要不变，再通过窗口管理器 Alt+F4 关闭。GUI 主进程必须由本任务 subreaper 实际回收且退出 0，control.sock/control.token 不得残留。非零退出、超时或未知回收状态均失败。成功循环逐轮保存 PID、退出码与端点回收收据；正常与 GDB 观测结果文件独立，不将观测改变的时序当作修复。

GDB 仅附加到本任务实际 GUI PID，禁止 debuginfod 自动下载；打印参数关闭，不读取局部变量、堆、寄存器中的值或 core 内存。符号栈与 shared-library 映射用于定位 native 关闭。runner 禁止 core dump，避免保存 Vault 进程内存。GDB、CLI、GUI、fixture、转发 peer 及可访问性子进程由现有 finally 路径回收；最终归档和全部退出后才能执行 clean-dev，保留 target/probes。

## 结果与通过边界

本轮语法与生产特性构建通过。正常关闭观测先为锁定／解锁各 12 轮，再分别扩展 32 轮，共 88 轮；GDB 符号观测为各 4 轮，共 8 轮。96 个 GUI PID 已去重核验，逐轮退出 0、零认证／签名／写入及 control.sock/control.token 回收均正常；8 份对应 PID 的 GDB 日志均有附加就绪和 exited normally，没有新的 SIGSEGV 或故障栈。实际 `--lib product_workspace::exit::tests` 的 3 项通过，覆盖真实 PTY／数据库回收、并发只启动一个 supervisor，以及只有物理完成才能允许退出；首个 bin 零测试调用另存且不计通过。最终 254 份 SHA 证据见 `target/probes/native-close-investigation.json`，链接此前 129 份不可变批次，历史崩溃证据保留。全部构建、GUI、fixture、GDB、native 测试退出后 clean-dev 回收 9.9 GiB，本批临时目录清空。SDK 源核对为 tauri/runtime-wry 2.12.1、wry 0.57.0：最后窗口销毁后触发 ExitRequested，AppHandle.exit 通过 runtime handle 请求退出；这个顺序不能说明历史崩溃根因。生产关闭实现未改，间歇故障仍未定位，不计修复。正常与 GDB 结果分别存于 `target/probes/native-close-{locked,unlocked}-{normal,gdb}-result.json`。

有限重复未重现只能证明该批场景正常退出，不等于可靠性问题已解决。只有取得有效故障定位、实现有依据的修复、保持退出授权与物理回收屏障，并通过针对性 native 生命周期和真实关闭验证后，才能更新修复状态。跨平台、外网、硬件 Agent 和发行审计不在本调查的通过范围。
