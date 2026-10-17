# Linux 原生会话默认并发调查

历史冷分屏合并回归有九项五秒超时，原始证据保留于 `target/probes/cold-split-merge-parallel-failed.log` 及 `target/probes/cold-split-acceptance.json`。六项等待首批解析输出，三项等待完成回执，不能全部归因于回收或 CPU。原先四线程通过不证明默认并发稳定。

本轮未修改生产代码、原测试断言或期限。独立 `pulse-app` 默认并发 294 项通过，完整 `cargo test --workspace --locked` 默认并发 1063 项通过，7 项忽略不计。两者均显式移除 `RUST_TEST_THREADS`，不把本轮未重现记录为已修复。

## 复现入口

```sh
mise run probe-native-parallel
PULSE_NATIVE_PARALLEL_CYCLES=2 mise run probe-native-parallel
```

仅支持 Linux `/proc`。每种模式默认 8 轮，范围 1–32；模式为无线程覆盖、`--test-threads=64`、两个无覆盖测试进程同时运行。64 是测试调度上限，实际并行量受选中测试数和系统资源限制。工具从 Cargo JSON 产物选择唯一 `pulse_app` 测试二进制，使用 `local_` 筛选，并确认历史九项都包含在实际列表中，不重新实现或弱化这些测试。

结果在 `target/probes/native-parallel/`。当前八轮批次筛选 40 项，24 轮、32 个独立进程、1280 次现有测试执行通过；完整批次收据另存 `target/probes/native-parallel-stress8.json`。这是有界样本，不是 1280 个不同测试，也不是默认并发问题修复。后续注册入口和故障监督反例的结果分别记录，不覆盖历史失败。

每个测试仍使用其原期限；20 秒外层监督期限只防止整个测试进程挂死，300 秒构建期限不授予退出或原生完成证明。失败立即停止，不自动重试；失败进程与其后代只记录 PID、启动时间、父 PID、线程名和内核等待点，不记录命令参数、环境、内存或 core。正常输出、非零退出及有限等待点快照均保留。

监督器成为本任务后代的 subreaper，按 PID 和启动时间识别进程，构建/测试失败时回收其后代；Popen 本身回收直接子进程，避免吞掉非零状态。绿色测试仍有原生子进程存活也判失败。固定反例分别验证测试和构建退出 18 不会变为成功，遗留的 sleep 后代实际回收，见 `target/probes/native-parallel-supervisor-counterexamples.json`。

临时目录仅在 `/data/temp/native-parallel-run.*` 创建，I/O 和进程退出后删除；若回收失败则保留精确路径并报错。CI Linux 增加每种两轮的注册任务，跨平台不记为通过。工具不验证真实 WebView、进程树终止承诺、硬件 Agent、外网或发布性能，也不替代历史关闭 SIGSEGV 的独立调查。

最终注册入口另有 8 个独立测试进程、320 次原测试执行通过，Python 语法、Mise 注册、CI YAML、轮次边界和 core 禁用检查通过，Astrolabe 返回 Exact、无匹配诊断。两项失败监督反例按最终实现复验通过。SHA 证据入口为 `target/probes/native-parallel-acceptance.json`，包含历史失败、两批原测试收据、故障夹具源码、配置与当前源码。所有记录进程和项目构建／原生进程退出后，`mise run clean-dev` 回收 4.3GiB，保留证据并清空本批临时目录。历史九项超时与原生关闭 SIGSEGV 仍未定位或修复。
