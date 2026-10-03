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

## 检查边界

- cargo-deny 的来源与 bans 检查通过（重复版本为警告）。
- 许可证与 advisories 检查失败，完整审计门禁保持失败。
- pnpm audit 当前未报告已知漏洞；这不是前端许可证检查。
- 已配置三平台构建/检查和 Linux 原生 WebView CI；尚未取得 CI 执行证据。
