# PulseTerm 开发约定

- 先读取当前实现和相关文档，再编辑。
- 工具链版本与开发任务统一由 `.mise.toml` 管理，不创建 `rust-toolchain.toml` 或 `.nvmrc` 等重复版本文件；库依赖由 manifests 和 lockfiles 管理。
- 代码发现、精确引用、调用关系及依赖影响遵循当前可用的 FFF／ast-sgrep 工具约定，并用当前源码核实；编辑后运行针对性的编译、类型检查与测试。语言服务返回 Unknown／超时不算通过，安装语言服务前必须取得授权。
- 提交信息遵循 Conventional Commits，只写技术/产品变化，不能包含排期、日期、Sprint、周数或里程碑安排等内容。
- 基础验证使用 `mise run check`、`mise run test`、`mise run probe`、`mise run audit`。不得把 headless 探针当作真实 WebView、IME、网络互通或跨平台性能验证。
- 验收批次完成、证据归档且构建／探针／原生窗口全部退出后，执行 `mise run clean-dev` 清理累积的 debug/test 产物。保留 `target/probes/`；不得用无范围 `cargo clean` 删除验收证据，也不在仍需复用二进制的验证链中途清理。磁盘不足先检查容量与 inode，不清理其他项目缓存。
- 安全约束和模块分层见 `docs/development/guide.md` 与 `docs/architecture/`。
