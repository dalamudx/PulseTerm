# 第三方署名材料与发布文本门禁

工作区材料已经生成：[NOTICE](../../NOTICE)、[THIRD_PARTY.md](../../THIRD_PARTY.md)、[机器可读清单](../../third-party/inventory.json) 与 `third-party/licenses/` 中保留的原文字节。它们覆盖锁文件的工作区范围，尚不能证明某个发行二进制的署名或源码提供义务完整。

## 来源与覆盖

当前清单为 769 个组件：619 个非工作区 Rust 包、pnpm 锁文件中的全部 149 个包，以及原始 JetBrains Mono 2.304 字体。pnpm 范围包括未在本机安装的可选平台包，不能用本机安装清单代替。

生成器用 Cargo.lock 的 SHA-256 或 pnpm v9 的 SHA-512 校验发布归档，再读取包内元数据与独立许可证／COPYING／NOTICE／AUTHORS 等文本。平台归档不解包，不执行其二进制或安装脚本。原文字节按内容 SHA-256 去重存储，各包仍保留包内原路径和来源；这不是用统一 SPDX 模板替代上游原文。

已批准的 `alacritty_terminal` 补丁继续绑定原发布包摘要和 `vendor/` 来源记录，NOTICE 明确标注修改；字体来源、原始 WOFF2 和完整 OFL 也独立校验。不主张补丁已获上游接纳，也不改默认字体字号。

补充来源计划位于 `third-party/supplemental-sources.json`，并参与输入摘要。npm 平台包仅在已锁父包原始 `package.json` 列出其精确可选版本、仓库及许可声明一致且归档摘要相符时复用父包文本。`cargo-vcs` 补充仅来自发布包 `.cargo_vcs_info.json` 指定的干净提交，记录该文件摘要和祖先路径，联网生成再校验上游原文 SHA-256。没有 VCS、dirty 发布、兄弟目录和不匹配仓库都不按这一方式自动补齐。
npm 的另一种补充绑定 `npm-manifest-commit` 只接受完整性已验证的发布归档在原始 `package.json` 内携带的 40 位 `commit`，记录该 manifest 的原文和 SHA-256，并复核包名、版本、许可、仓库与锁定 integrity。补充文本仅限该提交的仓库根目录原文，拒绝当前分支、兄弟目录及仅由可变 registry 元数据提供的提交。发布方内嵌提交是来源声明，不是可复现构建证明；源仓库模板版本与最终发布版本也不要求相同。

`cargo-workspace-parent` 直接复用已锁定工作区父包发布归档内的根目录原文。两包必须携带同一干净提交，父包原始 `Cargo.toml.orig` 必须明确声明子包的路径和版本，包身份、许可、两归档摘要、原始 manifest 和 VCS 证据均需匹配。版本只接受锁定精确值，或明确对应 `.0` 发布的主／次版本；不按任意 semver 范围推断。拒绝 dirty、换提交、穿越／兄弟路径、未锁定或仅有补充原文的父包；不访问当前分支，不接受同名仓库猜测。这一关联不替代系统动态库的许可及发行审查。


补充方式 `cargo-license-reference` 仅接受完整性校验后的 Rust 发布归档内，携带已观察 MPL2 官方声明的 `.rs` 原文。计划保存完整声明源文件与摘要，联网生成从归档重新提取并校验；完整许可只取声明所指 Mozilla 官网的 MPL2 纯文本端点并固定 SHA-256。源码声明和官方原文分别保留，不以 SPDX 短名合成文本，也不把这一方式用于带 SDK 疑义的 objc2 说明。此来源绑定不代表接受 MPL 政策、完成源码提供义务或批准发行。

objc2 家族根目录 `LICENSE.md` 只有许可链接和 Apple SDK 说明；可保留为 `supplemented-notice-only`，仍属缺完整原文，不能以链接说明关闭门禁。独立原归档识别也支持 `MIT-LICENSE`／`Apache-LICENSE`，避免漏掉后缀命名的文本。

`.gitattributes` 禁止对 `third-party/licenses/` 的原文作文本换行转换。不要对该目录运行格式器；原文目录仅豁免尾随空格提示，来源与字节摘要校验不变。

## 可重复执行

- `mise run third-party-write`：联网收集锁定归档并生成或刷新材料。只更新先前已验证的生成物；用户改过的文件、外来文件或完整性错误都会阻止覆盖。
- `mise run third-party-check`：运行 39 项行为检查，然后离线验证完整包集合、输入摘要、来源完整性字段、补充关联、每份原文和生成文档。输入变化后必须重新生成。此检查不编译业务代码、不执行依赖包。
- `mise run third-party-release-check`：一致性校验后，若仍有任何组件缺完整原文或仅补入了许可链接说明，则退出非零。这个文本门禁即使以后通过，也不代表许可证政策或按二进制发行审查通过。

脚本使用现有 Python 3.11+ 标准库。联网生成和测试临时目录仅位于 `/data/temp`，结束后回收；CI 在三个系统上登记离线检查，Linux 另运行发布文本门禁，当前只核验了本机 Linux 结果。

## 当前未闭合项

最新材料经锁定归档联网生成与离线校验：769 个组件、437 份去重原文、94 项补充来源，完整文本缺口21项（18项objc2 notice-only＋3项未绑定）。97个发布归档自身未附独立文本仍单列。Vitest 5.0.5 的 SHA512、Node／Vite要求及新增许可原文已独立核验；上游采用why-is-node-running 3.2.1，移除stackback并减少10个声明组件，全部1549项既有前端测试保持通过。libappindicator-sys发布父包与selectors官方引用绑定保留；组件数不是实际分发资源审查。

三项未绑定来源为 `russh-util@0.52.0` 和两个 `winapi-*-pc-windows-gnu@0.4.0`；stackback 已随上游测试工具升级离开锁图，未用标签推测补入文本。逐包发布证据与当前处置见[剩余来源审查](remaining-attribution-review.md)。russh-util原发布仍dirty，winapi仍含待审的原生导入库，既有来源拒绝规则不变。

`releaseApproved` 始终为 `false`；将其改成 `true` 会使校验失败。Cargo/npm 元数据中的作者、许可证表达式和仓库 URL 都只是上游声明。

原生内嵌子组件、系统动态库、平台安装资源及源码提供义务仍需按实际发行内容审查。本批仅升级JavaScript开发测试工具，JavaScript审计零漏洞；Cargo.lock及deny.toml均未改，沿用上批8项许可拒绝／5项维护公告，Rust审计未重复执行。文本门禁仍失败，releaseApproved仍为false。

## 验证与归档

首批注册生成与离线校验通过：778 组件、427 份去重原文、99 项缺文本，`releaseApproved=false`；当时十项行为检查及发布文本拒绝已验证。首批不可变证据继续保留。远端 CI、Windows/macOS 执行未计通过。

最终来源、材料、配置、历史 TLS 失败及通过／拒绝日志的 SHA 归档见 `target/probes/third-party-acceptance.json`。相关采集、原生和构建进程退出后执行 `clean-dev`（移除 0 文件），本批独占临时目录清理；持久材料和不可变证据保留。未重复不受影响的终端／网络业务回归，8 项许可证、2 项维护策略审计阻断及发行审查继续保留。


补充批次已实测：18 项行为、注册生成／离线检查通过；778 组件、436 份去重原文、91 项来源补充，仍缺完整文本 25 项（18 项 notice-only＋7 项未绑定）。发布文本门禁确实因 25 项缺口退出 1。生成器、补充验证器和测试脚本的 Astrolabe 诊断均为 Exact、零诊断；配置及全部原文字节保护已核验。补充批次 SHA 见 `target/probes/attribution-sources-acceptance.json`，最终SHA复核完成并包含清理收据（clean-dev移除0文件）；独占临时目录在最后检查后删除；历史 API 限流及 TLS 失败单独保留，不计通过。


后续内嵌提交批次已实测：`@xterm/headless@6.0.0` 的完整性已验证归档携带 `f447274f430fd22513f6adbf9862d19524471c04`，绑定原始 manifest 摘要及同提交根目录 MIT 原文；其源树 headless 模板仍标 5.5.0，没有把源模板当成 6.0.0 发布包。23 项行为、离线/材料、配置和原文字节保护检查通过，三脚本 Astrolabe Exact 零诊断。778 组件、436 份去重原文不变，来源补充 91→92、完整文本缺项 25→24（18 notice-only＋6 未绑定），原归档遗漏 98 仍单列；发布门禁实测因 24 缺口退出 1。`stackback` 的发布元数据没有 gitHead、归档也不带 commit，不能用当前分支补入。两轮注册生成因 TLS EOF 失败后，以相同 `generate` 函数及任务独占校验缓存完成；一个 SHA512 失败响应经 gzip 校验确认为截断，保留大小/摘要后只清除该任务文件，未接受错误响应或改锁摘要。新批次归档 `target/probes/attribution-next-acceptance.json`，最终SHA复核完成并包含清理收据（clean-dev移除0文件），本批独占临时目录于最后检查后删除；旧归档保留。


本轮官方引用批次已实测：selectors@0.38.0的完整性已验证发布lib.rs包含Mozilla标准声明，绑定该源文件全文和SHA-256、发布归档摘要与官网MPL2纯文本端点及摘要。新增两份原文，436→438；来源补充92→93，完整文本缺项24→23（5未绑定＋18notice-only）。31项行为、实际生成与注册离线/发布门禁、独立材料/Mise/Git属性检查通过；门禁仍因23项缺口退出1，未接受MPL发行政策。Astrolabe三个脚本Exact零诊断。生成使用相同generate函数及任务独占校验缓存；首轮渲染KeyError已修复并补回归，失败单列。新批次 `target/probes/remaining-attribution-acceptance.json` 最终480份SHA已复核并包含clean-dev移除0文件收据；独占临时目录于最后校验后删除，历史批次继续保留。

本轮工作区发布父包来源已实测：libappindicator-sys@0.9.0与锁定libappindicator@0.9.0的同一干净提交、父包原始manifest的sys/依赖和0.9→0.9.0版本关系明确，直接复用父包归档MIT／Apache原文。来源93→94，438份去重原文不变，完整文本缺项23→22（4未绑定＋18 notice-only）。39项行为、实际注册生成、离线／独立发布归档原文、Mise/CI/Git属性检查通过，三个脚本Astrolabe Exact零诊断；文本门禁仍因22项缺口退出1。初始端到端fixture问题已修正并保留失败收据，不算生产故障；dirty拒绝、动态库义务、既有审计和远端平台验证边界不变。本批 `target/probes/published-source-acceptance.json` 最终494份SHA已复核并包含clean-dev移除0文件收据；独占临时目录在最后核验后删除。

平台候选调查：winapi父包发布提交中的平台文件与两个锁定子包不能逐字节关联；stackback发布标签候选也有manifest差异、缺文件且无完整许可。逐路径比较确认winapi差异不是LFS指针。未补入不可信文本，778组件／438份原文／94来源及22缺项保持不变，39项注册离线检查通过，发布门禁退出1。直接证据与脚本见 `target/probes/platform-attribution-acceptance.json`（最终20份SHA已复核，含clean-dev移除0文件收据；独占临时目录于最后检查后删除），既有完整材料仍由上一批494份归档保留。


候选目标的依赖范围由 [distribution-review](distribution-review.md) 独立报告：12项检查及五目标冻结解析通过，保留全部前端与字体，目标子集不改变工作区22项门禁。实际二进制、系统库和包内容尚未审查；最终50份SHA已复核，含clean-dev移除0文件收据，见 `target/probes/distribution-review-acceptance.json`；独占临时目录在最后核验后删除。
