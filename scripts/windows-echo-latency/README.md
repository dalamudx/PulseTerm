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

当前v7启动时显示`echo-kit: webview2-startup-v7`，保留v5进程详情。沿用v4导航成功后才开启诊断的顺序，记录进程失败的符号名称、Reason和十六进制ExitCode，以及独立getter HRESULT／接口不支持提示。`failureQueued`表示首个错误已排队关闭，`closing`表示已开始Controller.Close；后续事件可能属于回收，不能只凭事件先后推断共同根因。SDK中kind1为渲染进程退出，4为工具进程退出，6才是GPU进程退出。没有kind6不能排除驱动关联，也不能把kind1直接称为GPU崩溃。进程描述限定160个单行ASCII字符，环境覆盖只记录是否存在，不记录值；不收集内存转储或关闭GPU／沙箱。请发回完整宿主日志或zip，重点看`process_detail`行。

诊断日志会影响时序，因此该模式不用于延迟比较。正常模式不启用页面消息、写完成回调或逐次原生日志，默认仍测三目标两轮。阈值与1秒超时没有放宽。

## 渲染进程在采样前退出（0xc0000022）

`RENDER_PROCESS_EXITED`＋`CRASHED`＋`exitCode=0xc0000022`表示渲染进程以Windows `STATUS_ACCESS_DENIED`退出。它不能单独确定被拒绝的对象或原因，也不能直接证明WebGL、驱动、杀毒软件或目录权限有问题。若未到达目标导航完成，先运行v6启动对照，无需管理员或Windows Terminal：

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1 -DiagnoseStartup -Rounds 1
```

可沿用`-RuntimePath`指定版本。该选项与`-DiagnoseWebGL`互斥。每轮按顺序用独立临时Profile启动三组：固定内存HTML（不读页面文件、不加载xterm、不执行诊断脚本）、原文件xterm DOM、原文件xterm WebGL。每组等目标导航／诊断就绪后再观察2秒；一组启动失败仍继续其他对照，最后统一报告失败。此模式不发送按键、不启动DXGI采样、不产生延迟成绩；短时就绪不证明长期稳定。

请发回完整`results-*.zip`。其中`startup-summary.json`区分各组结果，三份`*-host.log`保留实际进程退出信息。`startup-system.json`只读收集本次Kit／独立Profile／实际Runtime及其祖先的ACL SDDL，以及本次运行时间窗口内Application（1000／1001）、CodeIntegrity、AppLocker、Defender中与WebView2宿主／Runtime有关的事件；每通道最多检查128条、保留20条相关事件，只导出允许的模块／路径／状态字段。无事件、无权读取、日志不可用和查询达到上限分别记录；未发现事件不证明没有拦截。路径与SDDL可能包含用户名和SID，不导出命令行、通用事件正文、输入内容或内存转储，也不修改ACL、系统策略或GPU／沙箱选项。

内存页也失败时，应继续调查Runtime进程启动及系统环境；内存页成功而DOM／WebGL失败时，范围收敛至文件页面／xterm相关路径；仅WebGL失败时，再关注WebGL差异。以上均为下一步排查依据，不是根因定论。原WebGL样本29无标记问题仍单独保留。

v7修正固定内存页的就绪误判：Runtime可能将`NavigateToString`的目标地址报告为固定内容对应的`data:text/html;charset=utf-8;base64,...`，v6只匹配`about:blank`会把已成功导航记为超时。现在仅在内存对照模式接受该固定页面的两种地址，任意其他data内容、文件页面和过期导航ID仍拒绝。若旧结果为`hostExitCode=0`、`timedOut=true`且宿主日志含`navigation_complete success=1 target=0`，应单独识别此诊断缺陷；它与先前`LAUNCH_FAILED / 40`不同，不据此判定正常回显或原WebGL样本29通过。

## 临时目录对照后仍为 LAUNCH_FAILED / 0x28

若内存页、DOM、WebGL在改用用户本地Temp后仍同样失败，继续使用已有v6宿主，补充运行`collect-startup.ps1`：

```powershell
powershell -ExecutionPolicy Bypass -File .\collect-startup.ps1
```

可使用同名`-RuntimePath`选项。独立小包`pulseterm-native-startup-v1.zip`只含此脚本，可配合v6及之后的完整宿主包；建议使用修正就绪判定的v7完整包。脚本只启动一次固定内存页，并临时给该子进程设置`--enable-logging --v=1 --log-file=...`，本轮使用用户目录下的本地Temp。原额外浏览器参数在本次子进程中被日志参数替代，父PowerShell的参数／TEMP／TMP始终恢复；不开放调试端口、不禁用GPU或沙箱。

请发回`native-startup-*.zip`。其中`host.log`保留宿主退出信息，`chromium.log`在Runtime支持并实际生成时保留原生启动日志；缺失会明确标注。`native-summary.json`记录最多20秒观察期间采集到的宿主／明确匹配本次独立Profile的Runtime模块路径、公司／版本元数据，以及只读系统和按程序配置的缓解策略。每个进程只采一次，最多32个Runtime进程、每进程256个模块；短命进程可能来不及读取，模块清单不是完整加载跟踪，公司字段不等于签名验证。权限不足或cmdlet不可用均记录为不可用，不安装组件。

该模式不发送按键、不读其他浏览器Profile、不采内存转储；原生日志及模块／策略字段未经原事件允许列表过滤，可能包含本机路径、用户名和内部URL。只回收明确匹配本次Profile的Runtime；退出未确认则保留Profile并在摘要和终端报告路径。此模式用于故障取证，不用于延迟比较，也不保证日志一定暴露底层失败原因。

## 范围与解释

Windows Terminal 经 ConPTY；WebView2 在页面内本地回显。路径不同，比较仅用于初筛，不能替代完整 PulseTerm 门禁。两边按键 `1` 都画纯红块，测量器用SendInput发键，DXGI检测像素增量并取对应帧QPC呈现时间；这是软件呈现时间，不是屏幕实际发光或物理键盘延迟。

同一屏幕、刷新率、显卡下各取p50／p95和差值。显示模式变化、缺失呈现时间、焦点丢失等拒绝汇总。记录积压帧与读取完成时间，读屏本身可能带来扰动。

每次运行使用独立临时配置，退出仅清理该目录和对应进程，不读取浏览资料，不保存截图或按键正文。本包已在Linux交叉编译并检查脚本；Windows11／RTX2080Ti／143.998Hz的用户正常回传已完成两轮共600正式样本，四个WebView2宿主导航成功并记录退出0。范围限该机器和运行条件，不能替代完整产品或三平台验收。
## 完整产品回显测量的前置条件

正式产品回显目标为p95<16ms，见[开发计划](../../docs/development/plan.md)。本包的Windows实机两轮WT／DOM／WebGL p95分别为32.024／32.588／32.966ms，只建立独立对照基线，不构成正式门禁通过。

当前完整Windows产品尚未接通：`workspace_profiles`和`workspace_session_open`在非Unix返回`workspace.unavailable`；生产`SettingsFile::open_private`在Windows返回`Unsupported`；SSH会话适配模块受Unix cfg限制。[Windows私有目录及文件基础层](../../docs/architecture/windows-private-directory.md)与设置适配候选已存在，但尚未接入生产且原生验收未完成。不能只把本包指向另一个exe，就将页面本地回显当作完整产品测量。

Windows路径须先完成私有存储／Profile、后端选择的Shell及ConPTY、原页面授权的输入／resize／刷新／物理退出，再让测量器观察该真实产品窗口；本地链完成后单独加入SSH链与网络条件。现有`mise run probe-workspace-echo`覆盖Linux生产WebView／PTY／字形变化及原生写入关联，但依赖X11／XTest、固定Unix夹具及软件渲染环境，不能作为Windows替代证据。当前按既有P0顺序先推进Windows最小生产链，现有平台原生测试包见[Windows存储前置验证](../windows-storage-probe/README.md)；产品测量仍被上述前置阻塞。


## 构建与第三方

Linux交叉构建：设置`CC_WIN`／`CXX_WIN`指向MinGW-w64、`WEBVIEW2_PACKAGE`指向官方`Microsoft.Web.WebView2 1.0.4258.31` nupkg，再运行`mise run windows-echo-kit`。构建校验固定SHA256，未改项目运行依赖。

包内包含MIT的xterm.js6.0.0、addon-webgl0.19.0及完整许可；WebView2Loader.dll来自官方SDK，微软BSD三条款许可、完整NOTICE和来源摘要在`third-party/`。不分发Runtime本体。`SHA256SUMS`覆盖关键exe、DLL、脚本和页面。
