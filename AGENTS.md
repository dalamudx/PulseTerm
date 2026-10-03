# PulseTerm 开发约定

- 先读取当前实现和相关文档，再编辑。
- 工具链版本与开发任务统一由 `.mise.toml` 管理，不创建 `rust-toolchain.toml` 或 `.nvmrc` 等重复版本文件；库依赖由 manifests 和 lockfiles 管理。
- 代码发现、精确引用、调用关系及依赖影响优先使用 MCP Astrolabe；编辑后请求诊断并运行针对性检查。语言服务器返回 Unknown/超时不算通过，安装前必须取得授权。
- 提交信息遵循 Conventional Commits，只写技术/产品变化，不能包含排期、日期、Sprint、周数或里程碑安排等内容。
- 基础验证使用 `mise run check`、`mise run test`、`mise run probe`、`mise run audit`。不得把 headless 探针当作真实 WebView、IME、网络互通或跨平台性能验证。
- 安全约束和模块分层见 `docs/development/guide.md` 与 `docs/architecture/`。
