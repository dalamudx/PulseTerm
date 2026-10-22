# Windows 生产链与原生验收

Windows 本地、SSH 和 CLI 入口已接线并完成共享回归及 GNU 交叉检查，尚未取得当前生产链的 MSVC／普通用户实窗验收。原存储 v3 的管理员环境 39/39 只证明当时的存储基础，不能覆盖新增生产用例。阶段退出条件见[下一阶段计划](next-stage-plan.md)。

## 实现范围

| 链路 | 当前实现与约束 | 设计说明 |
|---|---|---|
| 设置、键位与数据目录 | 缺失目录按层私有创建；既有 ACL 不修复；固定文件快照 CAS，损坏／未来版本不重置；取消后 I/O 容量保留到实际退出 | [Windows 私有存储](../architecture/windows-private-directory.md) |
| Profile／Vault／SQLite | 主库、侧文件、迁移备份分别准入；平台句柄留在桌面适配，随数据库线程退出；store 不依赖平台层 | [Windows 私有存储](../architecture/windows-private-directory.md) |
| Shell／ConPTY | 后端发现并保存 `pwsh`、`powershell`、`cmd`、默认已注册 WSL；发现与启动共用尽力刷新 PATH；WSL 固定发行版参数 | [环境刷新与日志](../architecture/windows-profile-environment.md) |
| HTTP／HTTPS | 原页授权后在独立 STA 线程系统交接；每窗一个在途任务，取消和关闭等待实际结束 | [终端链接](../architecture/terminal-links.md) |
| SSH | 显式 `workspace-ssh` 接入密码、私钥、Agent、MFA、临时目标、跳板和配置导入；保持实际 peer、Vault epoch 和退出监督 | [Windows SSH](../architecture/windows-ssh-product.md) |
| CLI | 显式 `workspace-cli` 接入账户核验命名管道、精确窗口路由和固定同伴冷启动；部分／陈旧端点不修复，二实例参数转发拒绝 | [Windows CLI](../architecture/windows-cli-control.md) |

普通 `workspace` 不自动启用 SSH／CLI，现有本地验证包范围不变。CLI 支持离线双语帮助／版本，以及使用已保存默认身份的分屏；缺少默认项不回退，读取不覆盖 GUI 编辑票据，冷分屏仍需两次批准。`pulse .` 当前目录交接尚未实现。使用方式见[CLI 手册](../user/cli.md)。

## 原生 CI 入口

| 入口 | 执行范围 | 证据边界 |
|---|---|---|
| [check.yml](../../.github/workflows/check.yml) | 三平台核心、平台策略、Agent；Linux／Windows CLI 组件；Windows 配置来源；Linux 产品探针 | runner 权限及虚拟显示不代表普通用户、系统 IME 或真实 GPU |
| [windows-product.yml](../../.github/workflows/windows-product.yml) | MSVC 核心用例、PowerShell 5.1 采集、前端生成、完整桌面检查和本地工作区构建 | 可手动执行；上传逐组收据，不发布验证包或登记 GUI 通过 |
| `mise run test-windows-product` | 固定 MSVC，逐项核对 22 项生产目录／设置／SQLite／键位／发现用例 | 零／部分／忽略／错名／重复／未知退出均失败；取消不能证明线程结束 |
| `mise run test-cli-transport` | Linux 11 项协议；Windows 3 项同源管道及 4 项冷启动元数据夹具 | 不启动 GUI；元数据夹具不等于 CreateProcess 验收 |
| `mise run test-windows-ssh-config` | Windows 4 项同源来源／Include／替换保护夹具 | 不读取密钥，不代表 GUI 导入或 SSH 连接验收 |

基础验证沿用 `mise run check`、`test`、`probe`、`audit`。组件执行器共用精确测试身份校验；Windows 核心收据保存独立 runId、源码指纹、stdout／stderr 和退出状态，失败撤销 latest 成功指针，历史批次保留。原生完成状态未知时停止后续组并保留夹具。

远程工作流仍待实际仓库／runner 执行。取得远程证据后回写本页和[验证记录](validation.md)，不以工作流存在或交叉编译替代执行。

## 普通用户验收顺序

1. 同一账户首次启动，保存设置／键位和 Profile，重开后核对；验证损坏、冲突、撤销及数据库侧文件。
2. 明确批准 Shell，核对输入、中文、尺寸、标签／分屏、刷新保活、重开及 GUI／PTY／数据库线程退出。WSL 字节与尺寸单独验收。
3. 验证运行期间更新 PATH 后重新发现与启动；旧会话、隐藏状态和删除墓碑保持。
4. 验证浏览器交接、CLI 冷暖启动／窗口路由／批准／断连及端点回收；再覆盖 Vault、实际 SSH peer、认证方法、MFA、跳板、取消与关闭。
5. 用真实 release 构建测完整回显、全进程内存、启动与恢复；记录图形环境、长尾、失败和未覆盖项。系统 IME／DPI／多屏另验。

## 交付阻塞

GNU 验证包仍因 `winapi-x86_64-pc-windows-gnu@0.4.0` 锁定归档缺独立许可原文而停止组装；MSVC 构建不自动解除实际发行范围的许可审查。五项 Rust 维护公告、21 项完整文本缺口、MPL 源码渠道及签名／更新仍开放，见[依赖审计](dependency-audit.md)。Linux 软件渲染的内存与回显仍未达标，测量统一见[性能评估](linux-runtime-performance-review.md)。

## 本地证据索引

下列路径属于被 Git 忽略的 `target/probes/`，不随源码提交。远程 CI 以本次源码重新生成并上传证据；本地历史收据和原始失败继续保留。

| 范围 | 收据路径（相对 `target/probes/`） |
|---|---|
| 存储基础管理员 v3 | `windows-storage-kit/accepted/v3/native-elevated/verification.json` |
| 生产存储与本地接线 | `windows-product-core/verification.json` |
| 环境刷新 | `windows-profile-environment/verification-v1.json` |
| 系统浏览器交接 | `windows-link-open/verification-v1.json` |
| Agent／SSH 产品 | `windows-ssh-agent/verification-v1.json` · `windows-ssh-product/verification-v1.json` |
| 暖／冷 CLI | `windows-cli/verification-v1.json` · `windows-cold-cli/verification-v1.json` |
| SSH 配置导入 | `windows-ssh-config/verification-v1.json` |
| 离线帮助与默认分屏 | `cli-default-split/verification-v1.json` |

Linux 实窗记录仅用于共享链路兼容性：暖 CLI 的窗口路由／批准／刷新与退出，以及默认项缺失零 PTY、GUI 保存后冷／暖分屏、独立输入与退出已通过。首次观察超时、空前端资源、Cargo 资源缓存及 GTK 下拉夹具失败均在各自收据中，不改记为 Windows 或性能成绩。
