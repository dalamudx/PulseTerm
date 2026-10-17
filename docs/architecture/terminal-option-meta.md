# macOS Option 作为 Meta 键

TERM-18 的本批交付是全局设置中的“macOS Option 作为 Meta 键”。`[terminal].mac_option_as_meta` 默认 `false`；开启后，新建或重建的终端 Surface 将此值传给锁定 xterm 的公开 `macOptionIsMeta` 选项，让 SDK 处理 Option 的 Meta 行为。关闭时保留 SDK 原有 macOS 字符输入行为。Linux／Windows 的 Alt 行为不受此选项影响。

## 保存与生效

- 全局设置沿用单字段保存、文件冲突检查、原页票据和恢复继承。保存后更新当前页的显示偏好；已经挂载的 Surface 不在线切换，下一次打开或刷新终端视图时生效。
- 设置投影升级为 v9，严格要求布尔值和来源。v1～v8 不接受此新增字段，缺少该字段的旧版投影在 Surface 中按 `false` 处理。
- 存储模型支持默认值 → 用户 → 分组 → 主机的稀疏覆盖，显式 `false` 不等于恢复继承，作用域记录可持久化和删除该字段。分组／SSH入口的[统一编辑与新会话应用](terminal-scoped-behavior.md)已在后续批次接通；直属组以上的祖先组不额外叠加，原生及跨平台效果仍待验收。
- 当前 Profile 现已提供 Option／Meta、Backspace 和 Delete 的稀疏覆盖选择，显式 false 与 null 继承分开处理；与参数、环境等使用同一次 Profile 保存，见[后续成组实现](terminal-behavior.md)。原三字段外观保持兼容。

## 输入边界

此设置仅配置 xterm 的公开选项，不新增键盘监听、字节转换或执行入口。Surface 初始 `disableStdin`、原页／窗格生命周期、Commit／输入授权以及 IME 保护继续沿用既有流程。开启 Meta 不代表启用输入；视图释放后也不能发送输入。

## 本批检查

Linux 主机上完成两项 Store 检查（保存恢复与继承、分组／主机实际持久化），一项原生保存／重新读取／恢复／旧页拒绝检查，27 项相关前端行为及 TypeScript、schema、局部静态、格式、Store／桌面库严格 Clippy 和产品构建。Surface 夹具检查旧配置、显式 true／false、稀疏 Profile 合并和输入仍禁用。Store 诊断为 Exact 零问题；Biome 保留一项既有警告和一项既有信息提示，未新增警告。

未运行完整回归、SSH 或多窗口验收矩阵。未执行真实 macOS Option／输入法或原生 WebView 验收；合成夹具及 Linux 构建不证明物理键、系统快捷键或跨平台行为。开发记录保存在 `target/probes/option-meta-development/`，现有 CI 的设置和前端检查覆盖本次新增用例，不新增矩阵。
