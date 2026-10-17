# 剩余第三方来源审查

本轮从完整性已验证的锁定发布归档读取源码、构建脚本和 manifest，不执行包内代码或平台二进制。实际注册生成、离线检查和独立发布归档核验后，完整文本缺口为22项（18项notice-only＋4项未绑定）；libappindicator-sys的同提交发布父包原文关联已经通过，selectors官方原文继续保留。找到候选文本但未验证的组件不计入减少。

| 组件 | 直接发布证据 | 本轮结论 |
| --- | --- | --- |
| `libappindicator-sys@0.9.0` | 父子发布包均记录干净提交 `eafd1e3682a1247f595410266091e9684021cb6f`；父包原始 manifest 明确指向 `sys/`、版本 `0.9`，父包归档自带完整 MIT／Apache 原文 | 发布父包绑定、39项行为、实际注册生成／离线／独立发布归档核验通过，缺项23→22。FFI运行时加载的libayatana／libappindicator动态库义务仍另审 |
| `russh-util@0.52.0` | 发布 `.cargo_vcs_info.json` 标记 dirty，记录 `3d09c20c52a542ffa327723d3a44cb1edd9b0ca4` | 三个源码文件逐字节一致，但 `Cargo.toml.orig` 由记录提交的 `0.51.0` 改为 `0.52.0`；保留 dirty 拒绝，未引入版本差异豁免 |
| `selectors@0.38.0` | 发布 `lib.rs` 含 MPL2 官方声明及 `https://mozilla.org/MPL/2.0/` | 官网站点提供完整纯文本原文；新规则绑定归档、原始源文件、声明和官方文本摘要，实际生成、离线核验通过，源码声明和官方原文分别保留，未接受MPL发行政策 |
| `winapi-i686-pc-windows-gnu@0.4.0`／`winapi-x86_64-pc-windows-gnu@0.4.0` | 锁定父包winapi@0.3.9记录干净提交796a8e6c2971dc2ff1bcff166e6671284f9b5b6b，原始manifest明确i686／x86_64目标路径。对子包1390／1419个非Cargo生成文件逐路径比较，仅0／1个相同，365／363个路径不存在，LFS指针均为0 | 父包提交不能证明两个实际发布导入库来源，继续保留缺项；不以同仓库或相同许可声明覆盖二进制资源 |
| `stackback@0.0.2` | 发布归档没有内嵌提交；v0.0.2标签候选解引用为2963095372abf7b75ba55f01cef08ab1e62c2ff4。7个发布文件中5个同字节，package.json不同，.npmignore缺失；同候选无完整许可原文 | 继续保留缺项；标签仅为调查候选，不能替代包内提交或完整来源证明。HTTP源码归档404收据保留，实际使用精确核验的Git对象，不checkout或执行源码 |
| 18 项 objc2 组件 | 已保存同提交 `LICENSE.md`，仅有许可链接和 Apple SDK 说明 | 继续 notice-only，Mozilla 纯文本补充方式不适用于这些组件 |

Russh 比较范围是归档全部非生成文件：`Cargo.toml.orig` 与三个 `.rs` 文件；未把 Cargo 生成的规范化 manifest、锁文件或 VCS 记录当原始源文件等价证据。比较脚本和记录提交源码归档摘要见 `target/probes/remaining-attribution-russh-comparison.json`，差异本身也会归档。

Mozilla 官方原文端点为 `https://www.mozilla.org/media/MPL/2.0/index.txt`，本次 16726 字节，SHA-256 为 `3f3d9e0024b1921b067d6f7f88deb4a60cbe7a78e76c64e3f1d7fc3b779b9d04`。直接请求及 `/MPL/2.0/index.txt` 重定向所得字节一致。选择这份文本的依据是已发布源码明确指向它，不是包元数据中的 MPL 短名。

上批官方引用来源的31项行为检查、实际生成、离线/材料及Mise/Git原文字节保护通过，三个脚本Astrolabe诊断为Exact、零诊断。发布文本门禁仍因23项缺口退出1。首轮生成在文档渲染时遇到新来源没有VCS字段的KeyError，已修独立呈现并补端到端反例，失败收据保留。本批 `target/probes/remaining-attribution-acceptance.json` 最终480份SHA已复核并包含clean-dev移除0文件收据；独占临时目录于最后校验后删除。8项许可证、2项维护策略及按发行内容审查保持独立，releaseApproved仍为false。

本轮发布父包批次的39项行为、实际注册生成、离线／独立发布归档原文、Mise/CI/Git属性检查通过，三个脚本Astrolabe Exact零诊断。libappindicator-sys来源已绑定到锁定父包归档，完整文本缺项23→22，文本门禁仍退出1。最终 `target/probes/published-source-acceptance.json` 494份SHA已复核，含clean-dev移除0文件收据；初始fixture失败与历史各批证据均保留。独占临时目录在最后核验后删除，dirty拒绝及动态库发行义务继续保留。

平台候选后续调查未减少22项缺口：winapi逐路径比较包含全部导入库，并排除了LFS指针导致的表示差异；stackback固定标签候选既缺文件又缺许可原文。完整发现清单、逐文件SHA及比较脚本见 `target/probes/platform-attribution-acceptance.json`（最终20份SHA已复核，含clean-dev移除0文件收据；独占临时目录于最后检查后删除）。本轮只更新审查文档，未变更438份原文、94条补充来源、生成规则、锁文件或许可政策。注册离线检查的39项行为通过，文本门禁仍因22项缺口退出1；原生资源发行审查和现有审计阻断保持独立。
