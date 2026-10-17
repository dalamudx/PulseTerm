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
| [architecture/profile-editor.md](./architecture/profile-editor.md) | 存储／终端／前端开发 | 自动 Shell 的字面参数、工作目录、环境覆盖与稀疏外观；原页保存及启动授权、Linux 验收边界 |
| [architecture/keymap-source.md](./architecture/keymap-source.md) | 存储／前端开发 | 有界键位覆盖、来源／冲突、固定文件 CAS 与取消安全后台 I/O；不授予动作执行权限 |
| [architecture/workspace-command-palette.md](./architecture/workspace-command-palette.md) | 前端开发 | 当前页固定命令选择、显示目标与权限分离、模态键盘与实际 PTY 隔离 |
| [architecture/terminal-layout-authorization.md](./architecture/terminal-layout-authorization.md) | 终端开发 | 布局专用 resize 授权、输入确认门禁、原生 ACK 与取消后的实际 I/O 信用 |
| [architecture/decisions.md](./architecture/decisions.md) | 全员 | 架构决策记录（ADR）、待决事项、技术验证清单（S1–S21） |
| [development/plan.md](./development/plan.md) | 全员 | v1.0 开发计划：时间线、分工、关键路径、完成标准及动态交付清单／当前进度 |
| [development/guide.md](./development/guide.md) | 开发 | 环境、构建、测试、代码规范、CI、发布、合规红线、安全要求、开源协作 |

当前已实现范围及验证结果见 [development/validation.md](./development/validation.md)，原生依赖门禁见 [development/dependency-review.md](./development/dependency-review.md)。

> 修改架构的流程：先在 `architecture/decisions.md` 登记 ADR，再更新对应的文档。
