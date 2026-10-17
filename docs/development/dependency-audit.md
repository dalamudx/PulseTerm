# 依赖审计执行与发布阻断

`mise run audit` 必须完成 Rust 与 JavaScript 两项检查；Rust 的许可证或维护策略失败不能跳过前端安全审计。统一入口为 `scripts/audit-dependencies.mjs`，使用 `.mise.toml` 已固定的 Node，不安装额外工具或依赖。

## 执行和回执

- Rust 保持 `cargo deny --all-features --locked check`，使用原 `deny.toml`；JavaScript 保持 moderate 阈值，执行 `pnpm audit --audit-level moderate --json`，不排除开发依赖。
- 每次启动先使旧的通过报告失效；两项都尝试执行并分别记录 stdout、stderr、实际退出码、信号和执行错误。命令不存在或子命令非零不能阻止另一项执行。
- `target/probes/dependency-audit/summary.json` 的 `completed` 表示两项执行已结束，`passed` 才表示本次门检查通过。缺少命令、信号退出、无效前端 JSON／严重级别统计、成功退出却包含 moderate 及以上告警均不能通过。
- 原始非零退出码保留；如子命令没有可用非零码，则统一失败为 1。若两项都失败，以第一项的非零码退出，同时完整保留两项结果。
- 记录并比较锁文件、策略、根 manifest 和前端 manifest 的执行前后 SHA。列出的输入在执行中改变会阻断通过；这不是完整 SBOM 或文件系统事务证明。
- 两项原始日志与报告保存在 `target/probes/dependency-audit/`。Linux CI 在作业未取消时执行审计，前面步骤失败也不跳过；`always()` 上传该目录。CI 配置尚无远端执行证据。

## 本轮验证

`mise run audit-check` 的七项实际 Node 子进程反例通过：Rust 失败后仍执行前端、前端漏洞阻断、缺少命令覆盖旧成功、无效／矛盾 JSON 拒绝、low 告警保持现有阈值、输入变更阻断和子进程信号退出。临时夹具仅在本任务拥有的 `/data/temp` 目录创建并在子进程退出后删除。

真实 `mise run audit` 两项均完成：Rust 退出 5，仍有 8 项许可证和 2 项维护策略错误；前端退出 0，info／low／moderate／high／critical 均为 0。总结果退出 5，`completed=true`、`passed=false`。依赖、锁文件及 `deny.toml` 未变更，未新增允许项或忽略公告。JavaScript Astrolabe 诊断为 Unknown（TypeScript 安装定位失败），不计为通过；未安装语言服务器。仅在当前 Linux 环境验证，Windows/macOS 不计通过。

## 仍未解除的阻断

- `paste 1.0.15` 经 Specta function 支持引入，`proc-macro-error 1.0.4` 经 GTK／GLib 宏引入。当前 Specta 固定版本与本轮注册表查询结果相同；未验证兼容的上游替换，不迁移到 Tauri 3 预览版。
- 许可证错误涉及 `bitflags`、`dtoa`、`dtoa-short`、`option-ext`、`r-efi`、`selectors`、`webpki-root-certs`、`zerocopy` 的 SPDX 表达式与现有允许集。接受新许可证或例外需要独立审查，不能由脚本改变。
- `NOTICE`／`THIRD_PARTY.md` 已生成工作区级材料，原文和来源摘要见 `third-party/`。首批记录为 99 项独立原文缺项；最新材料仍有 22 项完整文本缺口，另须按实际发行内容审查。当前数量和来源以[第三方署名材料](third-party-attribution.md)为准，不能视为发布署名已齐全。
- 29 份最终 SHA 证据见 `target/probes/dependency-audit-acceptance.json`；归档前已确认相关进程退出，`clean-dev` 成功（移除 0 文件），本批独占临时目录清空。Alpha 发布门仍阻断；本轮补齐审计覆盖，不代表依赖合规或发布验收完成。
