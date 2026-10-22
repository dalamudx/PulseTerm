# PulseTerm

跨平台终端与远程连接工作台。目标技术栈为 Rust + Tauri 2 + React。

当前 Linux 工作区已接通本地 PTY、基础标签与双窗格、Profile 管理与参数／目录／环境／外观编辑、设置、Vault 及部分 SSH 和 CLI 路径，并完成各自限定范围的验证。**尚未达到 Alpha 发布标准**；macOS／Windows、系统 IME、性能和依赖审计等验收仍有缺口，RDP/VNC 未实现。最新状态以 [开发计划](docs/development/plan.md) 为准，证据与限制见 [验证记录](docs/development/validation.md)，产品与架构见 [文档索引](docs/README.md)。

## 本地检查

先安装 [mise](https://mise.jdx.dev/)，审阅并信任仓库配置：

```sh
mise trust .mise.toml
mise install
mise run bootstrap
mise run check
mise run test
mise run probe
mise run audit             # 维护公告与完整发行材料仍阻断，见依赖审计说明
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
