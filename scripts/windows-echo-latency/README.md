# Windows 真实 GPU 回显测量包（WebView2 版）

本包比较 Windows Terminal 与产品同版本 xterm.js 6 的 DOM／WebGL 回显。直接使用电脑已有的 **WebView2 Runtime**，无需安装 Edge 浏览器，不修改系统设置。它不是完整 PulseTerm，也不包含产品 IPC／SSH／Vault。

## 运行

1. 在 Windows“设置 → 系统 → 显示 → 高级显示”选定 **144Hz**，关闭 HDR。本机直接运行，不通过远程桌面。
2. 输入法切到英文，关闭录屏／截图工具。解压完整新包，别只替换旧 `run.ps1`。
3. 在解压目录打开普通 PowerShell（无需管理员）：

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

宿主自动发现已安装的 Evergreen WebView2。也可指定已有版本目录：

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1 -RuntimePath "C:\Program Files (x86)\Microsoft\EdgeWebView\Application\155.0.4283.62"
```

共两轮，每轮 Windows Terminal → WebView2 DOM → WebView2 WebGL，每目标预热5次、正式100次。约2–3分钟，期间不要碰键鼠、切窗或移动窗口。先等 WebView2 页面就绪，再开始四秒倒计时；启动失败会保留 `*-host.log`。

结果为 `results-时间-随机后缀.zip`，请将整个结果包发回。失败时保留结果目录和日志。60Hz和144Hz数据分开保存；JSON会记录实际模式。

## WebGL 中途无标记的诊断

若WebGL某次在1秒内没有新增红块，不能直接认定是输入法。请解压完整诊断版，只运行该目标：

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1 -RuntimePath "C:\Program Files (x86)\Microsoft\EdgeWebView\Application\155.0.4283.62" -DiagnoseWebGL -Rounds 1
```

无论成功或失败都会生成`results-*.zip`，请发回整个包。日志仅含固定事件、计数、光标、焦点和像素数，不保存输入正文或截图。主机QPC记录的是事件消息到达原生的时间，不是浏览器事件发生的精确时间；xterm的render事件也不等于最终上屏。失败JSON保留基准／最小／最大／最新红色像素、观测帧数及已经完成的样本，但不产生完整性能统计。页面异常标题和测量中失焦会单独拒绝。

诊断日志会影响时序，因此该模式不用于延迟比较。正常模式不启用页面消息、写完成回调或逐次原生日志，默认仍测三目标两轮。阈值与1秒超时没有放宽。

## 范围与解释

Windows Terminal 经 ConPTY；WebView2 在页面内本地回显。路径不同，比较仅用于初筛，不能替代完整 PulseTerm 门禁。两边按键 `1` 都画纯红块，测量器用SendInput发键，DXGI检测像素增量并取对应帧QPC呈现时间；这是软件呈现时间，不是屏幕实际发光或物理键盘延迟。

同一屏幕、刷新率、显卡下各取p50／p95和差值。显示模式变化、缺失呈现时间、焦点丢失等拒绝汇总。记录积压帧与读取完成时间，读屏本身可能带来扰动。

每次运行使用独立临时配置，退出仅清理该目录和对应进程，不读取浏览资料，不保存截图或按键正文。本包已在Linux交叉编译并检查脚本；WebView2创建、焦点和DXGI仍需Windows实机验证。

## 构建与第三方

Linux交叉构建：设置`CC_WIN`／`CXX_WIN`指向MinGW-w64、`WEBVIEW2_PACKAGE`指向官方`Microsoft.Web.WebView2 1.0.4258.31` nupkg，再运行`mise run windows-echo-kit`。构建校验固定SHA256，未改项目运行依赖。

包内包含MIT的xterm.js6.0.0、addon-webgl0.19.0及完整许可；WebView2Loader.dll来自官方SDK，微软BSD三条款许可、完整NOTICE和来源摘要在`third-party/`。不分发Runtime本体。`SHA256SUMS`覆盖关键exe、DLL、脚本和页面。
