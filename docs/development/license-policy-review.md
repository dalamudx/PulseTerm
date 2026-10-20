# 八项锁定版本许可证例外：待审批

本文件和相邻 `license-exceptions.candidate.toml` 仅供审查；当前 `deny.toml`、依赖版本与锁文件均未修改。新许可证准入按 [开发指南](guide.md) 许可证审批要求处理。本候选不包含公告 ignore、全局 allow 扩张、平台打包或发布授权。

## 精确范围

| crate | 仅此版本 | 许可 |
|---|---|---|
| cssparser | 0.37.0 | MPL-2.0 |
| cssparser-macros | 0.7.1 | MPL-2.0 |
| dtoa-short | 0.3.5 | MPL-2.0 |
| option-ext | 0.2.0 | MPL-2.0 |
| selectors | 0.38.0 | MPL-2.0 |
| doctest-file | 1.1.1 | 0BSD |
| recvmsg | 1.0.0 | 0BSD |
| target-lexicon | 0.12.16 | Apache-2.0 WITH LLVM-exception |

配置使用 `crate = "name@=version"`，未来版本不自动继承。全局七种 allow、0.9 置信度、wildcards 拒绝、来源检查及全部维护公告门禁原样保留。完整 diff 只是在 `[licenses]` 内增加八个例外。

## 原文与发行义务

八项均已有完整文本、锁定发布归档 SHA-256 及文本 SHA-256。`selectors` 使用已验证发布源码中的官方 MPL2 引用；其他七项直接取发布包的独立 LICENSE。逐包记录及原文归档见 `target/probes/license-policy-review/accepted/version-exceptions/package-proof.json`；不是依 SPDX 短名合成模板。

MPL 五项要保留完整许可和相关声明，并按 MPL 3.1／3.2 提供适用源代码及明确获取途径。即使没有修改，分发可执行形式仍有源码可得性义务；修改的覆盖文件还须按 MPL 提供。集成产品可使用其他许可，但不能限制收件人对 MPL 覆盖源码的权利。审批本例外不代表发行包的源码获取渠道已经落实。

两项 0BSD 原文均允许无偿／收费使用、复制、修改和分发，并保留无保证条款；本项目仍收录上游原文。LLVM 文本必须按完整 `Apache-2.0 WITH LLVM-exception` 审查，不把它截成普通 Apache；例外只对文本描述的编译嵌入／结合形式生效，其他 Apache 条件仍需按实际用途落实。

当前没有修改这八项的上游源码。开发依赖、构建依赖、最终二进制、系统 GTK／WebKit 动态库及平台资源继续分开审查；本候选不批准 GPL／AGPL 或 Apple SDK 条款。

## 隔离验证

cargo-deny 0.20.2、同一离线公告缓存、同一全特性锁图：当前配置退出5（八项许可＋五项维护）；候选许可证单项通过，候选全检查退出1（五项维护，许可拒绝为零）。五项维护是 paste、proc-macro-error、zbus_names、zvariant、zvariant_derive，均未忽略。离线结果不等价于更新在线公告库。

独立合成反例：cssparser 0.37.0 被允许，0.37.1 以及无关 MPL crate 均被拒绝，退出4；所有合成文件均在任务私有目录，不是假称已发布的上游版本。静态复核要求候选移除 `exceptions` 后与当前配置逐项相同。

即使审批并启用八项例外，五项维护、21项完整文本缺口、实际安装／更新与跨平台审查仍阻断发布。`releaseApproved` 保持 false，不能把候选许可通过称为 Alpha 或发布通过。

## 审批后的实施范围

若批准，只将这八个精确版本例外接入根 `deny.toml`，刷新受输入摘要影响的署名材料，再运行在线审计、离线校验和发布文本门禁。若只批准0BSD，则只接入doctest-file与recvmsg两项；其余六项保持拒绝。未批准时保留本候选供审阅，实际策略不变。
