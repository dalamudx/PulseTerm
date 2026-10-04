# 原生依赖门禁：待审查事项

当前功能编译与运行通过，但启用 `desktop` 的完整依赖审计未通过。**不得打包发布，不得声称依赖审计通过。** `.mise.toml` 的 `audit` 已改为 `cargo deny --all-features --locked check`，避免默认特性漏查 Tauri 可选依赖。

复现（联网更新公告）：

```sh
mise run audit
# 网络不可用时，只能检查已有缓存，不等价于获取最新公告：
mise exec -- cargo deny --all-features --offline check
```

## 许可证门禁

现有白名单拒绝以下精确版本；没有自动扩大全局白名单或添加例外：

| crate | 版本 | 许可证 | 依赖来源 |
|---|---|---|---|
| cssparser | 0.37.0 | MPL-2.0 | tauri-utils → dom_query |
| cssparser-macros | 0.7.1 | MPL-2.0 | cssparser |
| dtoa-short | 0.3.5 | MPL-2.0 | CSS 序列化 |
| selectors | 0.38.0 | MPL-2.0 | tauri-utils → dom_query |
| option-ext | 0.2.0 | MPL-2.0 | Tauri/Wry → dirs → dirs-sys |
| target-lexicon | 0.12.16 | Apache-2.0 WITH LLVM-exception | GTK 系统依赖构建工具 |

MPL 为文件级 copyleft，不等同于 GPL/AGPL；开发指南已允许在不修改文件、履行公开修改文件义务的条件下使用 MPL 库，但配置里的具体例外仍需审查。LLVM exception 也不是 GPL，不能仅因名字包含 Apache 就省略核对许可证文本。

**建议的审查方向（未批准、未实施）**：仅为上述已锁定 crate 版本添加定点许可证例外；保留 MPL 文件不修改、提供第三方署名与源码获取方式；审查 LLVM exception 义务。禁止直接放开全部 MPL、GPL 或 AGPL。开发依赖、构建依赖与最终分发的系统库应分别核对；系统 WebKitGTK/GTK 的分发义务不能由 Rust crate 审计替代。

## 维护公告

`RUSTSEC-2024-0370`：`proc-macro-error 1.0.4` 已停止维护，由 `glib-macros 0.18.5` / `gtk3-macros 0.18.2` 引入，用于 Tauri 的 GTK3 构建。该公告是维护状态公告，没有可直接替换的安全升级；不能据此声称它没有风险。

**建议的审查方向（未批准、未实施）**：跟踪 Tauri/GTK 依赖迁移，评估是否批准仅此公告的有理由、可复查例外。不得整体关闭 advisories，不得放行未来的实际漏洞公告。当前没有添加 ignore。

## 单独升级 Wry 的核查结论

用户选择保留 Tauri、优先检查升级 Wry。核查后没有改动 Cargo manifests 或 lockfile：

- crates.io `cargo search wry` / `cargo info wry` 显示最新已发布版本为 **0.57.0**，项目实际已使用该版本；没有更高已发布版本可以升级。
- 上游 `tauri-apps/wry` 的 `dev` 分支 manifest 仍为 0.57.0，Linux 仍依赖 GTK 0.18、gdkx11 0.18、WebKit2GTK 2.0.2。上游分支是可变来源，此结论只对应本次检查；没有把 git 分支引入项目。
- Tauri 2.12.1 本身也直接依赖 GTK 0.18；因此仅改变 Wry 的 GTK 依赖并不能清除整个旧依赖链，还涉及 Tauri runtime、Tao 和菜单库等组件的配套兼容性。
- 额外查看已发布的 `tauri-runtime-wry 3.0.0-alpha.4` manifest，仍要求 Wry 0.57.0、GTK 0.18。没有迁移到 Tauri 3 alpha；该预发布版本也不能证明当前维护公告已消除。

证据来源：本地 `cargo tree --workspace --all-features -i wry`、已发布 crate 的 Cargo manifests，以及 [Wry 上游 manifest](https://github.com/tauri-apps/wry/blob/dev/Cargo.toml)。没有可升级版本，因此无需声称已完成升级构建/运行测试。

**结论**：单独升级 Wry 暂不能解决 `proc-macro-error 1.0.4`。按用户指示已撤销宏补丁实验，保留原有 GTK 宏依赖与锁定版本，不再自动替换或 vendor。实验替代库也有维护公告，不能把换名视为问题解决。持续跟踪完整原生绑定栈升级；原有门禁保持失败。

## 类型生成验证引入的维护公告

S21 受控接入新增 `specta 2.0.0-rc.25 → paste 1.0.15`，`cargo deny --all-features --locked check advisories` 报告 `RUSTSEC-2024-0436`：paste 已停止维护。它用于函数反射的编译期宏，不应据此声称风险为零或自动豁免。原有 `RUSTSEC-2024-0370` 同时保留；Tauri/Wry/GTK 与旧宏版本没有升级或替换。

生成命令、事件和 Raw Channel 在 Linux 实测通过，但 ADR-018 仍是 Proposed。正式采用前需单独审查此维护风险；若不能接受，应评估计划中的 ts-rs 备选，而不是重新 vendor 或自动替换宏。当前不添加 ignore、不放宽许可证规则、不把受控验证结果当作发布许可。

## PTY 原语依赖检查

新增 `portable-pty 0.9.0`（MIT）及原生 I/O 传递依赖，版本通过 Cargo workspace / lockfile 管理。全特性联网审计的来源和 bans 通过；未新增拒绝项，仍是原有 6 个许可证拒绝和 `paste` / `proc-macro-error` 的维护公告。没有替换 GTK 宏、添加 vendor、放宽白名单或添加 ignore。

## 已批准的终端只读补丁

用户批准 alacritty_terminal 0.26.0 的一个只读 inactive_grid getter，当前通过 workspace `[patch.crates-io]` 引入。本轮例外不适用于 GTK 宏链，也不允许修改终端解析行为。保留发布包 Apache许可证、VCS来源、SHA-256文件基线及最小diff，详见 `vendor/README.md`。版本不升级；product Cargo.lock 仅移除该包的registry source/checksum，发布包摘要另有明确记录。

全特性审计没有新增拒绝项：原有6个许可证拒绝和paste/proc-macro-error维护公告仍保留，sources/bans通过。没有添加ignore、放宽白名单或撤销发布门禁。上游测试使用独立冻结的test-only lock，并非product依赖升级。

## DOM renderer开发验证依赖

新增devDependency `@xterm/xterm 6.0.0`，与现有`@xterm/headless`同版；pnpm锁仅新增该包、没有新增传递依赖或升级旧包。已读发布包LICENSE（MIT，保留xterm.js/SourceLair/Christopher Jeffrey版权）；integrity `sha512-TQwDdQGtwwDt+2cgKDLn0IRaSxYu1tSUjgKarSDkUM0ZNiSRXFpjxEsvc/Zgc5kq5omJ+V0a8/kIM2WD3sMOYg==`。DOM源与CSS仅在开发验证surface启动时动态加载；未来分发须包含该版权许可及现有第三方声明。pnpm audit当前无已知漏洞，不替代完整许可证审查。CSP使用Tauri本地页nonce，未放开unsafe-inline或script-src。Cargo.lock、vendor、白名单和公告ignore未变，原有发布阻断继续有效。

## S4可选SSH探针依赖

xtask新增optional `ssh-probe`：精确russh0.63.3（Apache-2.0）、rand0.10、tempfile3；russh default-features=false仅ring，不启用RSA/compression/DES/DSA。desktop依赖树不包含russh，production SSH模块仍未启用。crypto传递依赖含ssh-key0.7.0-rc.11，这不是稳定生产采用许可；后续需关注RC接口/安全维护与feature组合，ring标志也不代表所有RustCrypto算法都被排除。Cargo.lock新增传递包与既有依赖名字版本限定，比较HEAD旧package name/version没有移除或升级已有锁定版本，GTK宏链/approved terminal vendor不变。

完整全特性联网cargo-deny复核：sources/bans通过（更多重复crypto版本警告），license拒绝仍原6包，advisories仍paste/proc-macro-error两维护公告，无新拒绝项或漏洞条目；没有添加allow/ignore。optional开发依赖仍必须audit，probe成功不能许可打包或生产credential网络端点。系统OpenSSH软件本体许可、系统库分发另行审查，不由Cargo allowlist替代。

## russh0.63.3 Agent清理API协议问题

隔离OpenSSH10软件Agent验证中，初始空/加载synthetic key/raw签名/SSH signer认证通过，但AgentClient.remove_all_identities返回early EOF。读取已锁定upstream client.rs确认：先resize4零字节header，再append REMOVE_ALL_IDENTITIES，再把1u32 append到尾部，而没有覆写前4字节为实际frame长度；OpenSSH Agent收到零长度消息后关闭连接。没有提出生产采用许可、替换版本、vendor或patch该库。

探针改用scoped remove_identity(public)，其header正常生成；新增Tokio duplex frame测试确认header/body/string长度和message18，实际private Agent列出空identity、removed-key请求返回AgentProtocolError（不能用transport EOF伪成功）且仍能request-identities证明服务活着。只清probe自己的软件key，不使用用户Agent。future Agent/extension方法须分别验证，不能因为签名通过就忽略remove-all/扩展API风险；升级后复查/跟踪upstream，公开issue/PR仍需用户发布授权。

## 检查边界

- cargo-deny 的来源与 bans 检查通过（重复版本为警告）。
- 许可证与 advisories 检查失败，完整审计门禁保持失败。
- pnpm audit 当前未报告已知漏洞；这不是前端许可证检查。
- 已配置三平台构建/检查和 Linux 原生 WebView CI；尚未取得 CI 执行证据。
