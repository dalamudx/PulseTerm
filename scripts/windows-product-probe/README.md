# Windows 普通用户生产链验证 v1

此包包含真实 PulseTerm 工作区程序、22 项指定的 Windows 生产前置用例，以及独立的 ConPTY 探针。无需安装 Rust。沿用已有页面／窗口／Profile 授权，不接受页面提供可执行文件。

**使用普通、未提升权限的 64 位 Windows PowerShell。** 管理员令牌会被拒绝。旧 v3 管理员 39/39 结果保持独立；本包不运行需要符号链接特权的夹具，不修改系统策略，也不修复已有目录或文件的 ACL。

完整解压到一个新目录后运行：

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

脚本先检查全部文件摘要，发现原生用例，再逐项执行 22 项测试。两组测试程序各自还含其他用例；只把清单中明确执行且返回成功的用例计入本次范围。发现失败、零测试、忽略、错误退出或退出码缺失均不能通过。

随后检查 ConPTY 的 canonical 输入、Unicode 输出、原生尺寸读回、非零退出、EOF 和直接子进程终止。该探针不验证原始键盘协议、WSL 内部尺寸或后代进程树，也不产生性能成绩。

核心前置通过后，脚本依次打开两次真实工作区，每次最多等待十分钟：

1. 第一窗：选择后端发现的 Shell，明确点击启动。检查输入／回显、调整尺寸、新标签和分屏。保存一个设置及默认 Profile，然后正常关闭应用。
2. 第二窗：检查设置和默认 Profile 是否保持，重新启动 Shell；刷新页面后重新附着／打开并检查输入，最后正常关闭。
3. 若存在 WSL，请单独检查中文输出及调整窗口后的 `stty size`。未安装 WSL 不代表其行为已验证。

应用使用正式工作区身份 `dev.pulseterm.workspace-preview`，会在 OS 选择的数据目录中保存设置和 Profile。脚本的测试夹具是临时目录，结果包保留。不要在本轮输入真实 SSH 密码或私钥；Windows SSH／CLI 完整链另属后续阶段。

结束或报错都会生成 `product-results-*.zip`。回传该包，并说明上述窗口操作是否正常。窗口进程退出 0 只记录为退出成功，不自动宣称输入、刷新、持久化或后代进程回收全部通过；摘要中的产品验收保持 `pending`，等待实机行为证据。

如需只检查原生前置：

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1 -NativeOnly
```
## 原生 MSVC 源码验收入口

当前 checkout 的 GNU 验证 ZIP 仍受原始许可缺口阻断，本文运行步骤不代表已有可分发包。在具备 `.mise.toml` 工具及 Visual Studio C++／Windows SDK 的原生构建环境，可使用 `mise run test-windows-product`。该任务固定 MSVC 目标并逐项核对清单；不启动 GUI，也不产生普通用户验收或性能成绩。

`target/probes/windows-product-native.json` 指向本次结果，独立目录 `target/probes/windows-product-native/<runId>/` 保存每组原始输出和摘要。失败同样生成新结果，保留旧批次；未知退出／超时不继续使用同一夹具。CI 上传完整结果目录。

`mise run check-windows-product-runner` 是跨平台替身回归，不能当作 Windows 实机用例通过。


构建为 GNU 交叉编译的开发验证程序，嵌入了真实前端。它不是 MSVC 原生构建或发行包，也不适用于正式回显／内存成绩。运行需要机器已有 WebView2 Runtime；保留沙箱和现有安全策略。完整源码摘要、编译范围及原始第三方许可随包提供；缺失／摘要不符的许可阻止打包。
