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

**结论**：单独升级 Wry 暂不能解决 `proc-macro-error 1.0.4`。可行的下一步仍是评估两个旧版宏 crate 的定点补丁，或等待完整原生绑定栈升级；二者均未实施。升级路径与许可证审查互不替代，原有门禁保持失败。

## 检查边界

- cargo-deny 的来源与 bans 检查通过（重复版本为警告）。
- 许可证与 advisories 检查失败，完整审计门禁保持失败。
- pnpm audit 当前未报告已知漏洞；这不是前端许可证检查。
- 已配置三平台构建/检查和 Linux 原生 WebView CI；尚未取得 CI 执行证据。
