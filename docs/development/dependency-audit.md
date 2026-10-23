# 依赖审计与发布阻断

`mise run audit` 独立完成 Rust 与 JavaScript 检查，任一失败都阻断发行。入口为 `scripts/audit-dependencies.mjs`，工具版本由 `.mise.toml` 管理。审计通过也不能代替实际发行组件、完整许可文本及源码渠道审查。

## 执行和证据

- Rust 执行 `cargo deny --all-features --locked check`，保留 `deny.toml`；JavaScript 执行 `pnpm audit --audit-level moderate --json`，不排除开发依赖。
- 每次先将 latest 标记为未完成／未通过，再读取输入、建立独立 `target/probes/dependency-audit/runs/<UUID>/`。每批保留摘要及两项检查各自的 stdout／stderr、退出码、信号和执行错误，不覆盖旧批次。
- `summary.json` 为 schema 2，含 `runId`、`runDirectory`；日志路径相对审计根目录。`completed` 只表示两项已结束，`passed` 才表示通过。
- Rust 失败仍执行 JavaScript；命令缺失、未知退出、损坏／矛盾 JSON、moderate 及以上漏洞、输入指纹变化均失败。保留原非零退出码，无可用非零码时返回 1。
- CI 在作业未取消时执行审计，前序失败不跳过，并上传整个审计目录。工作流配置与远程实际执行分别记录。

10 项执行器回归覆盖真实子进程、两项独立执行、信号／未知退出、矛盾统计、输入变化、后轮不改旧证据及早期失败撤销旧成功。复现：`mise run audit-check`；联网更新公告：`mise run audit`。离线缓存不能冒充最新公告。

## 当前维护链

`ctutils` 已从 0.4.2 升级到 0.4.4，修复 RUSTSEC-2026-0385；本批锁图仅更新该组件及归档摘要。联网审计不再报告该漏洞，Rust 仍因五项维护公告退出 1；JavaScript 退出 0，各严重级别计数为零。存储与 SSH 回归、锁定归属材料生成和离线校验通过，其他发行门禁保持有效。

| 公告 | 锁定组件 | 依赖入口与迁移边界 |
|---|---|---|
| [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436) | `paste 1.0.15` | Specta 2.0.0-rc.25 及 Tauri 类型接线 |
| [RUSTSEC-2024-0370](https://rustsec.org/advisories/RUSTSEC-2024-0370) | `proc-macro-error 1.0.4` | GTK 0.18 的 GLib／GTK 宏，随 WebKit／Tauri 引入 |
| [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371) | `zbus_names 4.3.4` | `zbus 5.19.0` 及稳定单实例插件 |
| [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370) | `zvariant 5.15.0` | 同一 D-Bus 链 |
| [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372) | `zvariant_derive 5.15.0` | 同一 D-Bus 链 |

`zbus 6` 将后三项整合为内部模块／宏，但插件 2.5.3 的 Linux manifest 仍要求 zbus 5；只升级本项目直接依赖不能清除旧链。迁移须覆盖插件 API、实际 bus-name／PID、生命周期、冷启动转发和原页面授权。最近查询 Tauri 2.12.4、插件 2.5.3 和 Specta rc.25 未发现可直接替换的兼容组合；上游版本变化后重查，不自动添加公告 ignore 或宏 vendor。

## 许可与发行

八项精确版本许可例外已获批准并启用，不能再写为未批准的八项许可拒绝；范围见[原生依赖评审](dependency-review.md)和[许可策略](license-policy-review.md)。最新完整工作区材料仍缺 21 项独立原文，另有 MPL 源码获取渠道、实际系统／编译器组件及发行范围待审查。GNU Windows 包的目标 winapi 归档缺口继续阻断组包；MSVC 构建不自动解除义务。

当前归属数量和来源以[署名材料](third-party-attribution.md)、`THIRD_PARTY.md` 和 `third-party/inventory.json` 为准。下载、摘要或完整文本失败继续阻断发布，不以测试通过、SPDX 元数据或部分通知代替原文。

## 本地历史证据

收据均位于被 Git 忽略的 `target/probes/`，远程 CI 重新生成并上传本次证据：

- `dependency-audit/runs/af64ff47-297e-4a98-803c-fd74c593485e/`：ctutils 补丁后的五公告／零 JavaScript 漏洞批次；补丁、测试和归属收据见 `ctutils-security-update/verification-v1.json`。
- `dependency-audit/maintenance-review-v1/`：输入指纹、来源、回归及独立日志布局核验；`before-runner-fix/` 保留 schema 1 原始失败。
- `dependency-audit-acceptance.json`：初始执行器的历史范围。旧的 8 项许可／2 项维护结果不能覆盖当前状态。
