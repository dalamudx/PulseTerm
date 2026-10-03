# PulseTerm

跨平台终端与远程连接工作台。目标技术栈为 Rust + Tauri 2 + React。

当前仓库包含工程基础、二进制 IPC 编解码和终端双解析器验证器，**尚未实现桌面应用、PTY/SSH 会话或 RDP/VNC**。已实现范围和限制见 [验证记录](docs/development/validation.md)，产品与架构见 [文档索引](docs/README.md)。

## 本地检查

先安装 [mise](https://mise.jdx.dev/)，审阅并信任仓库配置：

```sh
mise trust .mise.toml
mise install
mise run bootstrap
mise run check
mise run test
mise run probe
mise run audit
```

工具版本统一由 `.mise.toml` 管理；Rust/JS 库依赖分别由 Cargo 和 pnpm manifests/lockfiles 管理。探针输出位于 `target/probes/`，不会提交到仓库。

## 许可证

MIT OR Apache-2.0，由使用者任选其一。第三方依赖的许可证以各自的声明为准。
