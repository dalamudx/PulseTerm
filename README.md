# PulseTerm

跨平台终端与远程连接工作台。目标技术栈为 Rust + Tauri 2 + React。

当前仓库包含工程基础、二进制 IPC 编解码、终端双解析器验证器，以及真实 Tauri WebView 数据通道验证应用，**尚未实现产品终端界面、PTY/SSH 会话或 RDP/VNC**。已实现范围和限制见 [验证记录](docs/development/validation.md)，产品与架构见 [文档索引](docs/README.md)。

## 本地检查

先安装 [mise](https://mise.jdx.dev/)，审阅并信任仓库配置：

```sh
mise trust .mise.toml
mise install
mise run bootstrap
mise run check
mise run test
mise run probe
mise run audit             # 包含原生可选依赖；当前存在待审批的依赖门禁，见验证记录
```

工具版本统一由 `.mise.toml` 管理；Rust/JS 库依赖分别由 Cargo 和 pnpm manifests/lockfiles 管理。探针输出位于 `target/probes/`，不会提交到仓库。

## 原生数据通道验证

安装平台开发依赖后：

```sh
mise run desktop-check     # 构建前端并检查原生适配层
mise run desktop           # 打开开发验证界面
mise run probe-channel     # 自动测量 Channel / 本机 WebSocket、背压及取消恢复
```

Linux 无显示服务时需要 `xvfb`、`dbus-x11` 和 `timeout`。结果写入 `target/probes/channel.json`；这些是合成数据测量，不代表完整终端或远程桌面性能。完整原生依赖审计目前阻塞，**不要将验证应用打包发布**；见 [依赖审查](docs/development/dependency-review.md)。

## 许可证

MIT OR Apache-2.0，由使用者任选其一。第三方依赖的许可证以各自的声明为准。
