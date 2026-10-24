# 候选发行依赖审查

`mise run distribution-review` 在当前解析主机上，用冻结 Cargo 依赖图对比五个桌面目标。任务运行行为检查、校验署名清单，再按锁文件获取依赖并解析图；不安装 target、不编译外平台、不打包或签名。

| 配置 | 目标 | 根 features | 用途 |
|---|---|---|---|
| linux-x64 | x86_64-unknown-linux-gnu | workspace-cli,workspace-ssh | 候选 |
| windows-msvc-x64 | x86_64-pc-windows-msvc | workspace-cli,workspace-ssh | 候选 |
| macos-arm64 | aarch64-apple-darwin | workspace-cli,workspace-ssh | 候选 |
| macos-x64 | x86_64-apple-darwin | workspace-cli,workspace-ssh | 候选 |
| windows-gnu-comparison | x86_64-pc-windows-gnu | workspace-cli,workspace-ssh | 对照，未选作发行工具链 |

三平台 CLI 与 SSH 均已接线，候选及 GNU 对照统一启用两者，避免遗漏控制链依赖。每个目标分别解析 `normal,no-proc-macro` 与 `normal,build`，记录根包、特性、Cargo／rustc 解析主机和原始图摘要。外平台构建依赖仍受解析主机影响，这些图不能替代原生构建或证明最终二进制链接内容。

前端保留全部锁定 npm 包和字体。xterm 组件虽声明为 devDependencies，仍可能进入 WebView 资产，不能据此排除。实际 bundler 资产、系统动态库和安装资源仍须单独审查。

结果写入 `target/probes/distribution-review.json`、同名 Markdown 和 `distribution-review-graphs/`。输入变化、清单过期、未知组件、图关系不一致或命令失败均拒绝完成，并撤销旧成功报告。`completed` 只代表图审查完成，`releaseApproved` 始终为 false；目标子集的缺口较少也不会放宽工作区文本或审计门禁。

## Rust MPL 源码伴随包

`mise run distribution-sources` 校验现有归属材料，下载五个锁定 Rust MPL 组件的官方发布归档，按 Cargo.lock 的 SHA-256 复核；归档作为原始字节保留，不解包或执行。伴随 ZIP 包含原始 `.crate`、许可证、来源 URL、摘要清单与取得说明。源码被本地 patch／replace、许可证不完整、归档摘要错误或准备期间输入变化时拒绝生成完成收据。

每次运行写入独立的 `target/probes/distribution-sources/<UUID>/`，保存摘要、可读清单和 `mpl-sources.zip`；失败记录不覆盖前批。临时下载位于 `/data/temp`，结束后删除。此入口在 Linux 发行 job 执行并上传证据，不加入三平台日常产品测试。

伴随包仅覆盖 Rust MPL 注册表源码。npm 原生工具、系统库与实际安装包内容仍须审查。准备材料不会发布公开源码渠道或批准发行；真正发布时，必须将匹配的源码伴随包与安装包放在同一可持续取得的位置，核对许可告知、修改及实际内容，再完成发布审查。当前 `bundle.active=false`，其他门禁继续保留。

## 验证记录

本轮14项图审查行为、五目标真实冻结解析及39项归属回归通过；源码准备另有7项检查。原有完整文本缺口仍为21项，`releaseApproved=false`。最新源码包和失败记录保留在独立运行目录。

初版图审查的50份来源／输入摘要及失败证据保留于 `target/probes/distribution-review-acceptance.json`，其中旧特性组合与数量属于历史结果。当前结果以本轮图及输入摘要为准。三平台原生测试、最终包内容、维护公告、签名和公证分别验收。
