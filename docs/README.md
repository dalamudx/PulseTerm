# PulseTerm 文档

> 产品名 **PulseTerm**，命令行入口是 `pulseterm`（可选别名 `pulse`）。
> 技术栈：Rust（tokio）+ Tauri 2 + React 19 / TypeScript + xterm.js。

## 阅读顺序

| 文档 | 面向 | 内容 |
|---|---|---|
| [product/features.md](./product/features.md) | 全员 | 功能规格（带编号和阶段）、非功能指标、平台支持、不做的事项、里程碑 |
| [architecture/overview.md](./architecture/overview.md) | 全员 | 设计原则、进程模型、分层、核心概念、关键流程、IPC 规范 |
| [architecture/backend.md](./architecture/backend.md) | Rust 开发 | Cargo workspace、会话与传输、终端和桌面的数据管线、Vault 存储、安全、Capability 层、CLI 通信 |
| [architecture/frontend.md](./architecture/frontend.md) | 前端开发 | 目录结构、状态管理、工作区模型、ViewHost、TermView、RemoteCanvas、主题与材质 |
| [architecture/decisions.md](./architecture/decisions.md) | 全员 | 架构决策记录（ADR）、待决事项、技术验证清单（S1–S21） |
| [development/plan.md](./development/plan.md) | 全员 | v1.0 开发计划：时间线、各 Sprint 分工、关键路径、风险、完成标准 |
| [development/guide.md](./development/guide.md) | 开发 | 环境、构建、测试、代码规范、CI、发布、合规红线、安全要求、开源协作 |

> 修改架构的流程：先在 `architecture/decisions.md` 登记 ADR，再更新对应的文档。
