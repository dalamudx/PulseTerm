# 候选发行依赖审查

`mise run distribution-review` 在当前解析主机上，用冻结的 Cargo 依赖图对比候选桌面目标。它运行行为检查，验证现有署名清单，再解析五个目标。只运行 Cargo 元数据／依赖树命令，不安装 target、不编译外平台、不打包或签名。

| 配置 | 目标 | 根 features | 用途 |
|---|---|---|---|
| linux-x64 | x86_64-unknown-linux-gnu | workspace-cli,workspace-ssh | 候选 |
| windows-msvc-x64 | x86_64-pc-windows-msvc | workspace-ssh | 候选 |
| macos-arm64 | aarch64-apple-darwin | workspace-ssh | 候选 |
| macos-x64 | x86_64-apple-darwin | workspace-ssh | 候选 |
| windows-gnu-comparison | x86_64-pc-windows-gnu | workspace-ssh | 对照，未选作发行工具链 |

CLI 目前为 Linux 专用，外平台不启用 `workspace-cli`。候选 features 不代表这些平台的产品、认证、窗口或编译验收已完成。工作区 Tauri 配置当前 `bundle.active=false`，也不是已形成的发行包。

每个目标分开解析 `normal,no-proc-macro` 与 `normal,build` 两棵树，固定根包、features、目标和 `--frozen`。普通依赖与额外构建／过程宏组件分别列出；这些都是源码依赖选择，并非静态链接、动态链接或最终二进制内容证明。记录 Cargo 版本、rustc 报告的解析主机及原始树摘要；外平台的构建依赖仍受当前主机影响，不能把 Linux 解析替代该平台原生构建。

前端保守保留全部锁定 npm 包和字体资产。xterm 的多个运行时组件在 `ui/package.json` 中声明为 devDependencies，所以不能用 `--prod` 或依赖类别推断其未进入 WebView 资产。后续应以实际 bundler 资产和原生链接结果补充内容审查；当前不裁减前端许可范围。

结果位于 `target/probes/distribution-review.json`、同名 Markdown 和 `distribution-review-graphs/`。JSON 的 `completed` 只表示本次图解析和一致性检查完成，`releaseApproved` 始终为 false。工作区缺文本列表完整保留，目标投影的数量较少也不会放宽 `third-party-release-check` 或 `audit`。任何输入漂移、清单过期、未知依赖、图关系不一致、命令失败或超时都会拒绝完成报告，撤销旧成功 JSON／Markdown；不自动重试。

本任务在 Linux CI 登记，上传 JSON、Markdown、日志和图。远端 CI 登记不是运行通过证据。系统动态库、原生资源、最终安装包内容、许可接受、维护公告、签名与实际发行义务仍需单独审查。

本轮12项行为检查、五目标真实冻结解析、独立图/输入摘要与配置检查通过。当前解析主机为 Linux；普通／额外构建组件数分别为 Linux 333／119、Windows MSVC 292／98、macOS arm64 291／90、macOS x64 291／91、Windows GNU对照 293／98。各目标均完整保留159 npm与字体资产，对应文本缺口2／2／11／11／3。两个脚本Astrolabe Exact零诊断；工作区22项门禁仍实测退出1。首轮asset fixture类型错误导致未完成报告，修复及未知类型反例后真实入口通过；失败收据保留。最终50份SHA已复核，含clean-dev移除0文件收据，见 `target/probes/distribution-review-acceptance.json`；独占临时目录在最后核验后删除。
