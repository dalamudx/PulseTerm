# 已实现范围与验证记录

本文按批次保留验证与失败原文；较早记录中的“尚待”只描述当时状态。当前范围及后续工作以 [plan.md §15](plan.md#15-动态交付清单与当前进度) 为准，动态清单整理前的批次原文见 [交付历史](plan-history.md)。

## SSH 探针与生产适配器配对严格检查

复现了 `cargo clippy -p xtask --all-features --all-targets --locked -- -D warnings` 的54项 `dead_code` 错误。它们来自独立直连探针复用整套生产SSH会话适配器时，缺少桌面路由／交互入口调用者；不是新发现的54处生产功能故障。已在 `xtask/src/main.rs` 的单个 `ssh_session_adapter` 导入项使用带原因的 `#[expect(dead_code)]`。私钥导入项的独立预期被编译器以“未满足预期”拒绝后已移除；`unfulfilled_lint_expectations` 和其他告警仍由 `-D warnings` 阻断。

新增正式任务 `mise run check-ssh-adapters`，先检查 `xtask --features ssh-target-probe --all-targets`，再检查 `pulseterm-desktop --features workspace-cli,workspace-ssh --all-targets`，两者均保持 `--locked -- -D warnings`。生产适配器没有此探针专属例外。Linux CI原SSH专项严格检查已改用配对任务，开发指南同步入口；其余SSH测试／探针不变。

本机Rust格式、xtask全特性／全目标严格Clippy及正式配对任务全部通过，三个检查均零告警。未更改生产认证／路由／会话逻辑，测试目标已参与严格编译，未为lint配置改动重复运行原生认证矩阵。远端CI尚无本次执行收据。失败清单、通过日志和汇总见 `target/probes/ssh-adapter-check/`。发布审计8项许可证、5项维护公告及其他Alpha门禁继续开放。

本批Clippy及构建进程退出后，已执行 `mise run clean-dev`，清除5670项、2.7GiB dev/test产物，随后移除唯一任务目录 `/data/temp/alpha-ssh-ci.3Z8CS1`；三份原始检查日志和汇总仍保留在 `target/probes/ssh-adapter-check/`。

## 回显转发阶段取证与 DOM 变更行候选

以 `1c0ffb3` 为源码基线，隔离 release／Linux Xvfb／软件渲染的临时取证完成了四轮真实 XTest→PTY→字形像素测量，每轮100次。只记录原生发送／有效ACK的序号、长度和时间，未记录终端内容；C使用单调时间与实时时钟配对。原生发送到像素变化的分段为诊断值，受时钟采样偏差影响；各段p95独立排序，不能相加或相减。

| 运行 | 按键至字形 p95 | PTY写出至原生发送 p95 | 原生发送至字形 p95 |
|---|---:|---:|---:|
| 原逻辑，带临时取证 | 52.834ms | 0.334ms | 47.875ms |
| DOM变更行候选，第1轮 | 50.020ms | 0.278ms | 47.041ms |
| DOM变更行候选，第2轮 | 59.730ms | 0.254ms | 54.832ms |
| DOM变更行候选，第3轮 | 57.983ms | 0.287ms | 54.092ms |

四轮均完整测量、正常App退出且全部原生进程排空，`passed=false`，16ms门禁保持失败。原逻辑的发送至页面ACK p95为33.827ms；ACK包含双帧及投影核验，仍不代表桌面合成器实际呈现。发送前仅约0.25–0.33ms的耗时，将主要调查范围收窄至WebView投递／解析／绘制／呈现；尚未定位WebView内部具体函数，也不能直接归因为硬件或某个调度器。

候选只让DOM数据帧使用xterm已调度的变更行绘制，控制／滚动／GPU仍全屏刷新；类型、格式、61项定向回归及release构建通过。但三轮未显示稳定回显收益，已撤回候选及全部临时取证，七个源码文件与签名基线完全一致。未发布新性能优化，也未替换此前无取证的58.746ms正式回显基线。后续需保留输入、ACK及显示核验约束，继续获取WebView时间线／真实显示器／硬件环境证据。

原始收据与截图位于 `target/probes/workspace-load/echo-relay-trace/`、`echo-dirty-rows-1/`、`echo-dirty-rows-2/`、`echo-dirty-rows-3/`；候选与取证补丁为 `echo-dirty-rows-candidate.patch`，回归日志与汇总为 `echo-relay-diagnostic/`。

本轮四次原生测量和全部构建进程退出后，已执行 `mise run clean-dev`（dev产物0项），再移除隔离release／UI构建目录 `/data/temp/alpha-echo-latency.b8WDwV`及本轮创建的 `target/probes/load-shell`、`echo-pixels`；原始收据、截图、候选补丁和回归日志仍保留。七个源码文件经 `git diff --exit-code` 确认为基线原文，未重复运行未变源码的既有检查。

## 已提交基线的 release 整进程复测与负载门禁

`a81b693` 基线的 `mise run probe-workspace-performance` release 复测已完成，独立收据为 `target/probes/workspace-performance/post-convergence.json`，旧 `release.json` 保留。Linux Xvfb／软件渲染下，空窗PSS 284.75MB、单会话335.53MB、十万行历史总进程树增量82.90MB、20标签471.99MB；十万行已完整留存，20个真实会话及原生进程全部退出。单会话<120MB／20标签<250MB／历史增量≤20MB三项仍未达标。单次观测高于旧基线，不能据此将变化归因于某项补丁；后端容器容量收益不直接代表WebView整进程收益。

独立 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 诊断已完成，收据 `no-compositing-diagnostic.json`：空窗286.40MB、单会话331.65MB、历史增量92.94MB、20标签452.62MB，三项内存目标仍未达标且全部原生进程退出。该单次对照未消除固定成本，不能将差异归因为稳定收益；产品渲染配置保持原值，也不作为硬件或默认模式验收。

新 `mise run probe-workspace-load` 已完成首轮真实 `/bin/cat`／release WebView验收：Ctrl+C发起至cat实际SIGINT退出16.107ms（同组信号收到15.828ms），低于100ms，全部原生进程正常退出。但完整1GiB在600秒内未完成，`target/probes/workspace-load/result.json`明确`passed=false`；失败截图保留，未放宽期限。原生显示仍连接、未上报视图失败。独立15秒诊断 `before-batching/result.json` 再次测到7.622ms中断、进程排空，约14.26秒输出11.27MB（约0.79MB/s），WebKit占约12.4 CPU秒、后端约7.2 CPU秒，说明持续输出吞吐需优化。

后端已保留已排队Data合并：最多八条／一个fragment，不等待、不跨控制屏障，原wire帧数／字节信用／ACK期限保持；明确计入单一前瞻内存和修复许可。17项附件边界／许可回归、1项真实actor→wire信用回归及严格Clippy通过；App完整SSH特性366项测试及35项文档测试全部通过。

相同1GiB文本和原600秒期限，release生产WebView实窗 `after-batching/result.json` 通过：真实`/bin/cat`完整输出1073741824字节并正常退出，用时267628.180ms（约4.01MB/s）；Ctrl+C到cat实际SIGINT退出41.002ms（同组信号收到40.715ms），最终完成标记实际绘制，GUI／PTY及所有原生进程正常退出。已审阅 `after-batching/complete.png`。此前600秒失败和15秒约0.79MB/s诊断均保留；因原完整输出未结束，不声称有精确的全量加速倍数。终端总状态比对、回显p95、冷启动、硬件／IME／跨平台仍未验收，内存门禁继续阻断。

Linux CI新增独立`terminal-load`作业，复用正式mise任务与原600秒原生期限、上传完整负载收据和截图；与现有审计失败独立执行，不改变发布审计门禁。当前仅确认本机结果及CI配置，尚无远端执行收据。

吞吐优化已签名提交 `d9fdc40`，`git verify-commit`通过。新 `mise run probe-workspace-echo` 使用原生XTest、固定PTY回显和X11字形像素观察（0.5ms采样）。首轮100次p95为65.860ms，`workspace-load/echo/result.json`保留未达16ms结果；两阶段关联采样 `echo-phases/result.json` 为整体60.275ms／输入7.415ms／输出54.260ms。独立禁合成对照 `echo-no-compositing/result.json` 为57.581ms／7.036ms／49.283ms，仍未达标，产品配置保持不变。

最终 `echo-final/result.json` 100次采样：整体p95 **58.746ms**、按键注入至PTY接收5.716ms、PTY写出至字形更新54.273ms；100次均满足“注入≤原生接收≤原生写出≤像素变化”，排除先于对应输出的无关像素变化。已审阅 `echo-final/echo-complete.png`；测量／EOF／正常App退出／全部原生进程回收均完成，但因性能目标未达，任务和收据保持失败。各分段p95来自各自排序，不能相加推导整体p95。固定PTY小回显不代表大量输出下的交互、真实显示器呈现或硬件／跨平台验收，原始样本和限制均归档。C严格编译、Python语法／诊断、TOML／CI YAML配置及补丁格式检查通过；正式回显入口依赖X11/XTest开发库。
本批1GiB／回显原生窗口、PTY、原生工作者和构建进程全部退出后，已执行 `mise run clean-dev`，清除6.4GiB dev/test产物；本任务目录 `/data/temp/alpha-convergence.cuhc4O`及自建`target/probes/load-shell`／`echo-pixels`可执行文件已移除。release临时产物随任务目录删除。所有性能／负载收据、截图及原始回归／Clippy日志保留在 `target/probes/`，清理后已检查存在。


## 可复现源码基线与 Alpha 收敛

已按依赖拆分签名实现提交：`110aa9e`（冷历史块索引与封存容量）、`008661b`（工作区／SSH联合认证、终端交付与Linux集中原生夹具），两者 `git verify-commit` 均通过。开发进度与本节记录单独提交；沿用已有签名配置，未推送。后续实施优先级及明确退出条件已整理到 `plan.md §15.2`。

正式 `mise run check`、`mise run test`、`mise run probe` 依次通过；前端154个文件／1476项测试通过，Rust workspace及文档测试通过，手动ignore探针保持忽略。check仍有43项前端警告，未作为错误压制；Astrolabe对Rust的诊断认证不完整，以实际编译／测试为准。原生SSH／GUI沿用此前分项实窗收据，不将本次headless probe计作新原生验收。

`mise run audit` 完整执行、退出5：Rust仍为8项许可证拒绝和5项维护公告，JavaScript审计通过且无漏洞；输入锁文件与策略均未变化。审计原文和哈希保留在 `target/probes/dependency-audit/`，组合结果收据为 `target/probes/alpha-convergence/baseline.json`。发布门继续阻断。

当前主机Linux x86_64，已安装Rust目标仅`x86_64-unknown-linux-gnu`；Windows生产Profile多处仍unavailable。跨平台原生链、系统IME／DPI／辅助技术、最新release整进程性能和安装／签名／更新继续实施或保留资源阻塞；本批不登记Alpha完成。构建目录 `/data/temp/alpha-convergence.cuhc4O` 暂保留，供下一条release性能链复用，待验收链结束统一清理。

## Linux 链接：实际 GIO 交接与图形浏览器加载

`mise run probe-workspace-settings-contract` 版本4已通过生产WebView／真实PTY。检查窗口先核对精确的本机HTTP目标，批准前无浏览器进程及页面请求；可信鼠标点击“在系统浏览器打开”后，真实Tauri命令经GIO调用临时默认处理器，启动独立资料目录的图形Chrome 146.0.7680.177。HTTP服务仅绑定127.0.0.1，记录正确路径和Chrome User-Agent，并通过窗口PID、标题、页面文字OCR及截图确认实际加载。成功后打开按钮禁用，物理重复点击不再次启动；正常关闭浏览器后验证其进程组退出。

临时`.desktop`与`mimeapps.list`只写入本批独立XDG目录，默认处理器是只接受本次精确URL的测试监督器，负责启动实际Chrome并在失败时回收它。原有用户浏览器配置不参与本检查。Chrome在root／软件渲染环境使用`--no-sandbox`及`--disable-gpu`，不计日常沙箱安全、硬件渲染或性能验收。此前设置、历史、滚动、响铃、链接检查与GTK剪贴板读回继续通过；PTY累计仍只收到明确测试的`6b`，工作区不导航，三条会话、worker、HTTP服务、图形浏览器与GUI全部退出。

首轮GIO回执成功但Chrome网络服务报`FD ownership violation`；移除夹具的`--disable-crashpad-for-testing`后，独立GIO本地页面复现通过。随后一轮在旧OSC8目标的AT-SPI只读接口就绪前读取而失败，改为在原期限内等待逐字匹配，完整生产实窗通过。Python／Shell语法、脚本诊断、生产构建及补丁检查通过；沿用同一Linux CI任务并明确已有Chrome／Chromium前置要求，未重复无变化的产品单测。

收据`target/probes/workspace-settings-contract-result.json`版本4、`workspace-link-browser-open.png`及`workspace-link-handoff-confirmed.png`已核对。该范围覆盖临时默认处理器到实际图形Chrome的本机HTTP交接；其他默认浏览器、HTTPS／代理／外网、SSH链接、跨平台、系统IME及辅助技术继续开放。

原生窗口、Chrome进程组、三条PTY会话、worker与本地HTTP／OCR进程退出后，已执行 `mise run clean-dev`，清除7.3GiB dev/test产物并移除 `/data/temp/alpha-browser-handoff.ETnkRc`；临时默认处理器、XDG配置和浏览器资料目录已随夹具清除，版本4收据及两张交接截图仍保留在 `target/probes/`。

## 终端链接：真实 WebView 检查与 GTK 剪贴板回读

`mise run probe-workspace-settings-contract` 版本3已通过 Linux 生产WebView／真实PTY。受控输出中的普通HTTPS网址、IPv4:端口及OSC8链接均经真实鼠标点击进入检查窗口；OSC8显示文字为 `https://friendly.invalid`，实际目标及GTK剪贴板回读均为 `https://example.invalid/actual?via=osc8`。IP:端口按既定HTTP候选规则显示及复制。带引号的 `/remote/team notes/report.txt`:17:4 显示独立行列，实际复制仅含去引号后的路径，不显示浏览器按钮。含账号的OSC8目标被拒绝，地址框隐藏、复制与打开禁用。

所有复制由可信鼠标事件触发，再用独立GTK进程读取私有Xvfb的CLIPBOARD逐字比较；Escape关闭不向PTY发送输入、不导航工作区。原raw worker累计接收仍恰为前一滚动场景的 `6b`；原有设置／历史／滚动／响铃检查继续通过，三条原生会话、worker及GUI全部退出。最终收据 `target/probes/workspace-settings-contract-result.json` 版本3，以及 `workspace-terminal-osc8-review.png`、`workspace-terminal-file-review.png` 已审阅。

首轮在OCR定位阶段超过四秒进程期限；同一截图单线程复现可完成，修正为 `OMP_THREAD_LIMIT=1`，并规范标记大小写、选取同名标记的首个坐标，随后完整实窗通过。Python／Shell语法、Astrolabe脚本诊断、生产构建及补丁格式通过；本批只扩展测试夹具与现有Linux CI，未重复无变化的前端单测。实际GIO／默认浏览器交接、SSH输出、Windows路径原生交互、系统IME／辅助技术、外部编辑器和跨平台仍开放。

原生窗口、三条PTY会话、worker与构建／OCR／剪贴板进程全部退出后，已执行 `mise run clean-dev`，清除7.3GiB dev/test产物并移除本任务目录 `/data/temp/alpha-terminal-links.3o53Nw`；收据及OSC8／文件路径截图仍保留在 `target/probes/`。Linux CI已在现有依赖安装步骤加入 `tesseract-ocr`，GTK剪贴板读回程序使用已有GTK开发库。

## 真实 PTY 交互：滚动、响铃与视口绘制竞争修复

`mise run probe-workspace-settings-contract` 版本2已通过 Linux 生产 WebView／真实PTY。沿用GUI／CLI的三会话设置与主／备用／自然结束查找链，新增160行受控输出和实时BEL：默认输出回底关闭时，回看位置在新输出后保留；真实鼠标点击及键盘空格均可返回最新；默认输入回底开启时，唯一明确输入的 `k` 返回底部。重复BEL单提示、清除后新BEL可再显示；保留未清除提示时刷新，同一原生会话恢复且回放不重放BEL。原始PTY记录仅为 `6b`，所有滚动／提示按钮与刷新未额外发送输入。三条会话、辅助worker及GUI全部退出，设置文档与既有冻结／继承检查继续通过。

首轮真实滚轮操作触发 `terminal.dom_projection`，视图因此关闭。根因是视口在双帧等待期间改变，核验仍对应旧绘制。Surface现在采样公有buffer／baseY／viewportY／格子尺寸，并用滚动事件身份识别移出后又返回原行的情况；视口失效时重新安排完整重绘、重新等待两帧，复用原一秒总期限。稳定视口仍逐行核对DOM，取消、隐藏、断连、尺寸变化或超时仍拒绝，不提前释放ACK，也不新增输入权限。

7份受影响Vitest共92项按有效候选合并通过；新增ACK反例修正对bootstrap多次绘制的错误假设后，流文件14项定向续跑通过。类型、局部Biome、Python／Shell语法、生产构建与补丁格式检查通过。收据 `target/probes/workspace-settings-contract-result.json` 版本2及 `workspace-terminal-reading-bell.png`、`workspace-terminal-interaction-refreshed.png` 已核对。正式mise入口、Linux CI及开发指南复用本集中场景。显式滚动偏好切换、SSH响铃／滚动、声音／通知／后台接收、辅助技术、跨平台和性能仍开放。

本批原生窗口、三条PTY会话、辅助worker及构建／测试进程全部退出后，已执行 `mise run clean-dev`，清除7.3GiB dev/test产物并移除本任务目录 `/data/temp/alpha-terminal-interaction.GSPqnR`。版本2收据及两张实窗截图保留于 `target/probes/`，清理后已核对存在。

## 冷历史封存：减少变长日志分配余量

冷块封存时对文本和行索引执行一次容量收紧；活动块继续采用原增长策略，不改变十万行上限、16MiB预算、Unicode文本、软换行及裁剪规则。163项终端测试和严格Clippy通过，新增回归检查变长块裁剪后的内容／标记、累计分配计账和清空。Astrolabe诊断Unknown不计通过。

同机release、相同100,010行输入、最终保留100,000行、64行热区的两轮前后对照：变长日志冷区分配稳定由7,949,824降至5,582,448字节，减少2,367,376字节（29.8%）；定长日志8,030,208字节不变。变长原始文本5,147,004字节及391块保持一致。首轮耗时出现波动，反向复测未优化实现也变慢；最终一轮变长输入未优化253.9～261.5ms、优化242.5～252.8ms，不据此宣称吞吐提升。全部样本见 `target/probes/history-capacity-result.json`。

复现：`cargo test -p pulse-term --release cold_history_capacity_probe -- --ignored --nocapture`（通过mise工具链运行）。这是后端容器容量统计，不是分配器RSS、原生WebView进程树内存或输入延迟。真实release进程树基线仍保持原验收状态，十万行内存及性能门禁未通过。

所有测量及测试进程退出后，已执行 `mise run clean-dev`，清理729.2MiB dev/test产物，再移除包含release构建的本任务目录 `/data/temp/alpha-history-capacity.9oInU5`。前后两轮全部样本已归档到收据，清理后检查 `target/probes/history-capacity-result.json` 仍存在。

## SSH 历史：跨软换行、硬换行隔离与自然 EOF

`mise run probe-workspace-ssh-settings` 版本3已通过 Linux 生产 WebView／真实 loopback SSH。远端输出一条超过最大列数的中文长逻辑行、分解形式 `e + U+0301` 和120行历史。以含中文的长查询确认跨多个物理软换行匹配，结果页直接检查原始组合字符码点保留；历史早期行定位成功，拼接硬换行两侧的查询明确无匹配。随后远端发送退出状态7、EOF和通道关闭，物理流退出后仍可查找原跨行历史及主屏结束标记。查找操作前后远端数据接收事件数不变，零终端输入和重新认证。

原有三层 TERM、按键、前景色、逐级恢复继承及刷新冻结检查继续通过，五条 SSH 流和 GUI 全部退出，连接／凭据／信任数据直接比较不变。收据 `target/probes/workspace-ssh-settings-result.json` 版本3记录上述检查；`workspace-ssh-settings-ssh-history.png` 与 `workspace-ssh-settings-ssh-ended-history.png` 已审阅。正式mise任务、Linux CI和开发指南复用同一场景，没有增加认证矩阵。生产构建、Python语法和Rust格式检查通过；Astrolabe夹具诊断Unknown不计通过。

初版将分解重音符号放入整串 X11 键盘查询，返回无匹配；尚无证据确认键盘产生的码点与原文一致。最终将跨行查询和组合字符保留分开验证，不改变原始Unicode不规范化的查找合同，也不将这批证据计作组合字符输入／系统IME验收。十万行内存、搜索／停止延迟及跨平台仍开放。

本批原生窗口、SSH流和构建进程退出后，已执行 `mise run clean-dev`，清除9.1GiB dev/test产物，并移除本任务目录 `/data/temp/alpha-ssh-history.ZlEfMK`。最终收据、peer日志及截图保留于 `target/probes/`，清理后已核对仍存在。

## SSH TERM：远端 PTY 请求与三层恢复继承

`mise run probe-workspace-ssh-settings` 的版本2实窗链已通过。Linux 生产 WebView／真实 loopback SSH 密码会话的五次 PTY 请求实际依次收到 `xterm-256color`、`xterm`、`vt100`、`xterm`、`xterm-256color`，对应全局、直属组、入口、清除入口覆盖、清除组覆盖。每次保存先核对数据库中的精确 `term_type`，再检查远端请求；已有会话重新选择、设置修改和刷新不重发 PTY，不重新认证。按键原始字节和 OSC 10 前景色的原断言继续通过，五条连接及 GUI 全部退出，凭据、连接和信任记录直接比较未变。

生产构建、Python语法、Rust格式及补丁格式检查通过。Astrolabe对特性下的夹具诊断为 Unknown，未计通过。夹具仅在作用域模式允许封闭 TERM 集合，其他场景仍仅接受默认值，尺寸约束保持原样。正式 mise、Linux CI 和开发说明同步扩展，无新增认证矩阵。收据 `target/probes/workspace-ssh-settings-result.json` 及 peer 日志已核对，最终窗口截图已审阅。此批验证远端协议请求，不代表外部 Shell 的 terminfo 兼容、macOS Option／IME、外部网络或跨平台完成。

本批原生窗口、五条 SSH 流及构建进程退出后，已执行 `mise run clean-dev`，清除8.3GiB dev/test产物并移除本任务临时目录 `/data/temp/alpha-ssh-term.QI4EtN`；最终收据、peer日志和截图仍保留在 `target/probes/`。

## 历史查找：主动停止与在途读取屏障

历史查找新增双语“停止查找”。停止保留当前页与匹配位置，撤销扫描代际并丢弃迟到匹配／错误，不再读取下一批；单次后端读取仍按原行数、字节与授权预算完成，返回前继续占用 busy，禁止新查找与翻页。编辑查询、关闭弹窗及页面失效保留原清理语义，不向终端发送输入。

类型、Biome、双语目录检查及 27 项历史查找／分页回归通过；新增反例覆盖重复停止、迟到成功／失败、在途时拒绝新请求、旧结果保留和关闭后零发布。Chrome 使用实际弹窗与受控 IPC 验证八项交互，包括语言实时切换、800px 无横向溢出和 Escape 返回入口；收据 `target/probes/history-search-stop-browser.json`，`history-search-stopping-en.png` 与 `history-search-stopped-zh.png` 已审阅。AFT 对查找状态机返回零诊断。原生 WebView／实际大历史停止延迟继续开放；本批不重跑认证、Rust 或全量矩阵，既有前端 CI 自动覆盖。

## 临时目标 Agent＋MFA：原页认证、成功后保存与物理退出

临时身份选择器已接通明确 Agent＋MFA，普通 Agent 保持默认。应用层新增 `prepare_temporary`，仅接受当前页发行的临时直连来源；保存目标 `prepare` 和路由 `prepare_route` 保持各自来源限制。原始页面、Vault epoch、选中 Agent、目标句柄及固定截止在准备与使用时复核，拒绝撤销后签名、跨来源准备与借用后来解锁。前端取消后的迟到成功不打开保存表单，不回退其他认证方法。

10 项 Agent 后端回归、50 项前端对话框／连接回归、类型检查、App／desktop 生产组合严格 Clippy 已通过。AFT 诊断 Unknown 未计通过。现有 Linux CI 与 `mise run probe-workspace-agent-interactive` 共用扩展场景，原保存目标反例继续覆盖普通 Agent 不升级、第一因素拒绝、仅公钥成功拒绝联合策略、错误回答无回退和来源变化零回答。

Linux 生产 WebView／真实软件 Agent／loopback SSH 的最终收据 `target/probes/workspace-agent-interactive-result.json` 记录十四项检查、四条成功联合会话和十一条连接退出；十一份签名均仅使用所选公钥，无 Agent 协议失败。新增临时链覆盖首轮取消后单条物理退出且原成功连接存活、两轮认证后暂不保存、终端输入、刷新不重认证，以及成功后明确保存精确已有目标。数据库以直接内容比较确认凭据、信任和主机元数据未变，最近记录不纳入内容不变断言；本次保存分支复用已有目标，不覆盖新建主机。最终窗口和全部 native 工作者退出，`workspace-temporary-agent-interactive.png` 已审阅。外部／硬件 Agent、企业 MFA、外部网络和跨平台继续开放。

探针开发失败已修正：直接比较数据库时的缩进错误、首次窗口／页面准备未分步，以及把仍活跃成功会话算入取消退出数的旧断言。启动等待现分为创建窗口与页面就绪各30秒，仍拒绝非就绪页面；取消检查本次物理退出增量和成功连接零退出，最终仍要求十一条连接全部排空。

本批窗口、全部连接及构建进程退出后执行 `mise run clean-dev`，清除 9.4GiB dev/test 产物，并移除本任务目录 `/data/temp/alpha-temporary-agent.7KDBN2`；最终收据、原生／peer 日志和截图保留在 `target/probes/`。

## 十万行历史查找：冷区块索引

冷历史行读取由逐块线性扫描改为块起点二分定位，分页、字面／正则查找和自然 EOF 文本共同受益；公开协议、修订、软换行、查找批次及 16MiB 冷区分配预算保持不变。补充变长块、行数／字节裁剪、环形块容器、部分前缀回收、清空与长会话计数重基反例。`cargo test -p pulse-term --release --locked` 通过 162 项，手动定时项默认忽略；显式定时也通过，`cargo clippy -p pulse-term --all-targets --locked -- -D warnings` 通过。AFT 诊断为 Unknown，未计通过。

同机 release 对照使用 80×3 格、64 行热区，写入 100010 行含中文和组合字符的固定文字，确认保留 100000 行后，对不存在的字面查询前后各完整扫描十次。两候选每方向均执行 490 个有界查询回合。十次总耗时：向后续行扫描由 512123µs 降到 244822µs，向较早行扫描由 534133µs 降到 250144µs；平均每次约 51.2→24.5ms 和 53.4→25.0ms（约 2.1 倍）。仅为本机后端扫描观测，不含 WebView／IPC 往返、输入延迟或内存验收，不设墙钟通过阈值。复现命令：`mise exec -- cargo test -p pulse-term --release --locked history_search_100000_rows_timing -- --ignored --nocapture`。对照收据 `target/probes/history-index-result.json`；既有 Rust CI 自动覆盖新增正确性测试，不新增重型矩阵。

## 终端失败提示：双语原因、恢复建议与输入不确定性

单窗格、分屏、SSH 显示、GUI／CLI 本地启动及重新选择会话共用有限错误投影：已知字体、尺寸、绘制、恢复、队列上限和原生可见性失败显示固定中英文原因与下一步操作；输入不确定、传输中断或队列停止明确提示先检查终端状态，不重复发送刚才的输入。未知错误保留各阶段原有提示；原生／远端错误文本、路径和敏感内容不进入界面，诊断上报仍限于原有白名单。CLI 仅在原当前请求失败时向本地反馈传递错误，原批准、取消和完成回执合同保持不变。

分屏按会话保留失败原因，其他窗格状态更新、聚焦和布局通知不会将其清空；对应窗格恢复／移除或整个视图销毁时清除。类型、Biome、双语目录检查与五个相关测试文件的 67 项回归已通过，覆盖输入不确定性、未知／恶意文本拒绝、分屏原因保留和 CLI 完成失败仅回报一次；AFT 对共享投影器返回零诊断。

Chrome headless 使用实际工作区样式和错误绑定，模拟输入不确定、字体及尺寸失败，检查中英文切换、1280px／800px 宽度下的换行、无横向溢出和 `aria-live=polite`；四张截图已审阅，收据为 `target/probes/terminal-error-browser-result.json`。这组证据仅覆盖浏览器呈现，原生 WebView 故障注入和辅助技术验收继续开放。

## SSH 作用域：全局／直属组／入口覆盖与逐级恢复继承

`mise run probe-workspace-ssh-settings` 已通过 Linux 生产 WebView／真实 loopback SSH 密码会话：从冷启动的全局经典配色与 DEL／CSI，依次验证直属组午夜配色与 BS／DEL、入口纸张配色与 DEL／CSI；清除入口覆盖后新会话恢复直属组值，清除组覆盖后新会话恢复全局值。每次保存均核对数据库中的精确值，实际输入以远端接收字节确认，颜色以 OSC 10 前景色查询确认。已有会话保持原按键与前景色，刷新不重新认证，保存组织／行为设置不启动连接；除组织、对应作用域和最近记录外，其余数据库内容直接比较未变。

收据 `target/probes/workspace-ssh-settings-result.json` 的检查全部为 true，记录五次密码认证及实际 GUI／五条 SSH 流全部退出；组行为面板和最终恢复全局的截图已审阅。正式 mise 任务及 Linux CI 已接入。本批覆盖按键和预设前景色，其他作用域字段、真实系统外观、macOS Option／IME、外部网络与跨平台仍按计划验收。

实窗发现两处行为元数据丢失并已修复：Root 回收曾遗漏活跃协议 owner，密码认证完成又直接拆解 parts、绕过携带外观的统一 `commit`。现由统一提交保留冻结偏好，Root 在活跃和关闭中保留对应记录，确认物理回收后才清除。协议 Root 生命周期和实际密文密码会话回归、App／desktop 生产特性组合严格 Clippy 均通过。组织行为字段使用稳定可访问名称及关联描述，组／入口动作名称明确区分；类型、Biome、双语检查及六项现有前端回归通过。原生选择需避开分组控件中间值触发的重绘，保存前核对精确目标标题，原保存、批准、冻结、源比较和退出断言保留。

`xtask --all-features` 的 `-D warnings` 检查仍被 54 项共享桌面模块未使用告警阻断；本批没有将该检查记为通过。

本批原生窗口、SSH 流和构建进程退出后，已执行 `mise run clean-dev`，清除 24.6 GiB dev/test 构建产物，再移除本任务临时目录 `/data/temp/alpha-closure.o4Fg4G`。SSH、Profile、全局设置／历史及 release 性能收据保留在 `target/probes/`，清理后已检查其仍存在。

## Profile 作用域：GUI／CLI 明确覆盖与恢复继承

`mise run probe-workspace-profile-editor` 已通过 Linux 生产 WebView／真实 PTY，保留原页取消／换页零写入、参数／目录／环境的精确原生字节及字号几何检查。明确保存 Profile 的 BS／DEL 与午夜配色后，原会话继续使用全局按键和前景色，新 GUI 与原窗口批准的 CLI 会话使用 Profile 覆盖；刷新与独立进程重开保持同一含义。随后将三项恢复为继承，当前 CLI 会话仍保留原覆盖，新会话恢复全局按键与经典前景色。原生颜色证据为 OSC 10 前景色，按键证据为 PTY 实际接收字节。

收据 `target/probes/workspace-profile-editor-result.json` 为 v2，记录九项检查、五次原生启动及全部退出。全局设置和其他表以直接内容比较确认未改。正式入口依次构建基础 workspace 与 workspace-cli，修复通用取消令牌和回答清零依赖仅在较高特性中启用的问题；两个构建及实窗均已通过。直属组／SSH 入口的按键与前景色继承验收见上节；其他 Profile 字段与跨平台继续验收。

## 全局设置与历史查找：GUI／CLI 冻结启动及自然 EOF

`mise run probe-workspace-settings-contract` 已通过 Linux 生产 WebView／真实 PTY：显式保存并重新打开 Backspace、Delete、TERM、终端预设及历史容量；刷新仍使用同一原生会话，按键编码按新显示设置生效，旧会话 TERM 和初始前景色保持启动值。新建 GUI 与经原页批准的 CLI 会话均继承新全局 TERM／前景色及 1000 行容量；原会话保留 1089 行，设置重开和会话启动均未改写配置。配色的原生查询证据为 OSC 10 前景色，不覆盖全部调色板或真实系统外观切换。

同一实窗完成历史／主屏、备用屏、自然结束后保留文本的查找，覆盖大小写、整词、正则、双向定位及非法正则；查找文字没有写入 PTY。成功截图已审阅；收据 `target/probes/workspace-settings-contract-result.json` 的 `passed`、`checksPassed`、`nativeExitConfirmed` 均为 true，三条原生会话全部退出。SSH 查找、软换行的原生专项、十万行内存／延迟及作用域继承仍按计划推进。

本场景发现并修复真实 EOF 缺陷：完成监督和 delivery 原本在 End ACK 后撤销页绑定，导致仍显示可查找的结束终端读取失败。生产捕获显式选择结束保留策略，仅在自然 `Ended` 且最终 ACK 成功时保留原页／代际的有界只读控制；转发任务退出，desktop 完成票据拒绝输入和布局。关闭视图、换页和替换代际撤销读取，旧票据不能操作新绑定；默认捕获的原撤销行为保留。24 项 Native 视图、17 项共享视图／交付回归及 App／desktop 全目标严格 Clippy 已通过，包含默认／显式保留、取消／发送失败、原页撤销及实际额度回收；Astrolabe Unknown 未作为检查通过。

原生脚本先前失败分别保留在历史输出：WebKit 下拉项的原生展开／提交、同名标签与控件区分、CLI Profile 必填参数、历史入口和 EOF 读取。修正选择动作后仍检查持久化值及实际输入字节，未移除保存、批准、冻结或退出断言。

## 保存路线 Agent + MFA：每跳批准、角色约束与物理排空

保存路线每跳可明确选择普通 Agent 或 Agent + MFA；原跳捕获收紧后保留路线来源、页、Vault epoch、原期限与转发／叶终端用途。联合转发只允许已冻结的下一目标，叶联合只允许原终端，普通 Agent 的准备与认证策略保持原行为。所选指纹签名与实际部分成功仍是进入问答的前提；每轮及最终注册复核原来源，失败无其他钥匙／密码回退。

`mise run probe-workspace-route-agent-interactive` 已通过 Linux 生产 WebView、软件 Agent 与真实三跳 SSH 的六项检查：密码第一跳、Agent 联合中间跳及 Agent 联合叶终端逐跳批准；终端输入和刷新不重新认证；每次签名只发生在对应精确 peer 批准后；第二次叶跳取消使父连接／转发通道退出，原成功会话继续存活；保存元数据和密文不变；关闭后父、叶及通道全部退出。收据 `target/probes/route-agent-interactive-result.json` 记录四次所选钥匙签名和零 Agent 协议失败；截图已审阅，正式任务及 Linux CI 已登记。

原生探针先后修正了混合场景的累计签名基数、只读预览与精确 ID 等待、实际 Agent 失败事件名，以及取消后按每条连接新增退出计数；未将原失败改写为通过。该批不覆盖临时目标 Agent 联合、硬件 Agent、外部 MFA 或跨平台。

## 原生可见性暂时失效的恢复

修复原生可见性一次失败／忙后永久关闭观察器的问题：发布 invalid 使旧 view 退役，保留后续事件 hint 或新页 snapshot 的只读采样；未知状态禁止输入。真实 GUI I/O 继续持有额度直至退出，Destroyed／shutdown、元数据异常与晚到结果保持关闭边界。Rust 的失败／忙／恢复、超时额度和窗口关闭八项回归，以及前端来源组合与过滤十九项测试、严格 Clippy 已通过。Astrolabe 对该 opt-in 文件返回 unlinked／Unknown，未当作诊断通过。

## CLI Agent + MFA：冷暖精确意图、原捕获与四条物理连接退出

新增 `pulse ssh <保存入口 ID> --identity <Agent 身份 ID> --agent-interactive`，三个交互标志互斥，别名、路由直连绕过、未知字段及不匹配身份拒绝。暖请求只消费原 `SavedSessionGesture` 并收紧成联合手势，保留原页、epoch、取消与截止；不发现 Agent、不读取凭据、不借后续解锁。冷请求在原窗口先解锁，再查看明确方法与精确目标。普通 Agent 手势不能升级为联合意图。前端原 ticket／token／一次性轮次监督复用已有问答链。

生产 Linux WebView、真实 CLI、双公钥软件 Agent 和实际 SSH partial_success 已通过：锁定取消、解锁不批准、目标取消均零新增签名；问题 Esc 和 CLI 断连各退出一条连接；冷暖两次联合认证各通过两轮、终端输入与刷新后输入，刷新不重新认证。四次签名均限保存公钥，未选公钥零签名；四条连接与原窗口端点退出，配置、Agent 元数据和密文保持原样。收据 `target/probes/cli-agent-interactive-result.json`，来源 `target/probes/cli-agent-interactive-test-server.json`；第二轮与刷新终端截图已审阅。正式 mise 任务及 Linux CI 已登记。

开发检查覆盖捕获过期／旧 epoch／源失效／期限不续期、CLI 冷传输及命令链互斥、旧轮次和晚到回答，前端类型与双语检查通过。新增 CLI 测试首次错误引用二进制 crate 的私有模块，后改用库公开模块并通过；严格 Clippy 发现新增测试的 expect，改为正常错误返回后通过。实窗准备先因夹具变量错误失败；共享探针后发现旧累计零签名断言与前面合法取消签名冲突，改为每次批准前零新增签名，再独立完整复验通过。软件 Agent、loopback 与软件渲染的覆盖范围保持不变，外部 MFA、硬件 Agent、逐跳／临时目标联合及跨平台继续开放。

## GUI 保存直连 Agent + MFA：保存指纹唯一签名、原捕获与八条连接排空

身份选择器新增明确的 Agent + MFA，普通 Agent 保持原方式。独立手势、动作、意图和标记 sink 保留原页、Vault epoch、保存 Host／Agent 元数据、取消及固定五分钟截止；后端发现 socket，只按保存算法和完整指纹请求所选公钥的一次实际 SSH 签名。只有已使用签名且服务器返回部分成功并继续声明 keyboard-interactive，才进入同一连接的有界问答。普通拒绝、仅公钥成功和普通 Agent 部分成功均不升级、不回退；每轮发送前重验来源。临时、路线、CLI 与密码联合不从本批意图继承权限。

生产 Linux WebView、含两个公钥的合成软件 Agent 与真实 loopback SSH 通过十一段验收：解锁不授予联合权限、普通 Agent 不升级、第一因素拒绝、仅公钥成功不跳过 MFA、Esc 与错误回答拒绝、第二轮等待时 Agent 元数据变化导致零回答、两轮联合成功、刷新不重认证、刷新后再次明确联合，以及正常关闭全部八条连接和原生资源退出。只有保存指纹对应公钥签名，未选钥匙未使用；两条 Root 终端输入回声通过，Agent 元数据、配置和密文恢复／保持原样。成功结果及截图分别为 `target/probes/workspace-agent-interactive-result.json` 与 `target/probes/workspace-agent-interactive.png`。

67 项受影响前端检查、2 项 Core 原捕获反例、3 项 Agent 协议回归通过；全仓 Rust 1138 项通过／7 项按既有条件忽略，前端 1443 项通过。最终 check、probe、类型、静态文案、产品构建、App 与 Native 全目标严格 Clippy 通过；正式认证任务十二组共 53 项均非零通过。早期实窗因探针错误的 SQLite 连接调用、旧按钮名称、遗漏选择函数及只读连接用于受控写入而失败，修复后静态名称检查及完整实窗通过。首次认证任务受并发产品资源重建影响，最终按实窗→check→认证串行复验通过；没有用这些前置失败作为产品通过证据。Rust Astrolabe 精确诊断无报错；TypeScript 初始化不可用不计通过，未安装服务。

正式 mise 入口 `probe-workspace-agent-interactive`、认证任务与 Linux CI 已注册。隔离服务端保留带来源哈希的 SDK partial-success 写包修正，生产依赖、锁文件和 registry 缓存不变；证据见 `target/probes/agent-interactive-test-server.json`。原始日志、来源、结果与最终收据归档于 `target/probes/ssh-agent-interactive-development/`。本批限 GUI 保存直连、Linux 软件渲染和合成 Agent／回答；CLI／逐跳／临时目标 Agent 联合、密码联合、外部企业 MFA、硬件 Agent、辅助技术及跨平台继续待验收。

JavaScript 审计通过；本次 Rust 审计为 8 项许可证拒绝与 5 项维护阻断，其中新增 zbus_names、zvariant、zvariant_derive 的 RUSTSEC-2026-0371／0370／0372。未放宽策略、未变更锁定依赖，软件 RSA 继续禁用。全部验证进程和原生窗口退出后，clean-dev 清理 21598 个文件／18.9 GiB；本轮唯一隔离目录已删除并核验不存在。原始日志与 target/probes 证据保留，最终清理状态见 receipt.json。

## SSH 保存路线逐跳私钥 + MFA：原角色、实际部分成功与三跳排空

每跳实际指纹确认可明确选择保存私钥或“私钥 + MFA”，账号仍独立选择保存密码或 MFA，Agent 保留原方式。新路由联合准备消费原私钥捕获，保留原页、Vault epoch、路线来源、取消与原截止，不能从账号捕获或普通私钥会话意图升级，也不能替换成直连五分钟期限。中间用途只允许转发冻结的下一目标，终端在借用凭据和调用适配器前拒绝；叶用途拒绝转发。Core 角色反例与既有原捕获测试共用非零正式入口；每跳来源复核、typed 方法选择及单次轮次约束继续有效。

生产 Linux WebView、原窗口批准后的 CLI 保存路线与三个真实 loopback SSH 服务通过十五段验证：冷启动“私钥 + MFA／保存私钥／私钥 + MFA”混合路线、暖启动三跳全联合，两轮问答、真实终端输入及刷新不重认证；普通私钥部分成功、第一因素拒绝和仅私钥完整成功均不能自动转入或满足联合策略；每层 Esc 取消和每层错误回答均拒绝而无方法回退；叶问题等待时改变私钥元数据，发送回答前拒绝，服务没有收到第二轮回答。精确首次 pin 已验证，保存目录与密文哈希除明确 pin 外不变，正常关闭后每层父连接、转发通道及全部叶连接退出。成功截图核对了两条 Root 终端，结果见 `target/probes/route-key-interactive-result.json`。

首轮正式认证和 Native strict 因旧测试的 Prompt 字面量漏新能力字段而编译失败；补齐后十组共 46 项认证测试非零通过，Native 全目标及 App 严格 Clippy 通过。初次实窗又发现指纹探测会产生额外短连接，固定按跳数计算退出数不正确；最终逐层按打开／退出余额验证前次尝试已全部回收，并在窗口关闭后逐层核对所有连接与通道。来源变化检查使用服务真实第二轮事件，避免空计数伪通过。54 项直接前端检查、类型与文案、全仓 `check`／`test`／`probe`、产品构建通过；Rust 1138 项通过／7 项按既有条件忽略，前端 1435 项通过。Astrolabe Rust 精确诊断超时、TypeScript 服务初始化失败均不计通过。

`probe-workspace-route-key-interactive`、现有正式认证入口与 Linux CI 已注册。隔离服务器复用有来源哈希校验的 SDK 服务端 partial-success 修正，记录在 `target/probes/route-key-interactive-test-server.json`；生产依赖、锁文件与 registry 缓存不改。原始日志和最终汇总保留在 `target/probes/ssh-route-private-key-interactive-development/`。本批仅验证受控 Linux loopback／软件渲染；外部企业 MFA、硬件设备、Agent／密码联合与跨平台继续待验收。JavaScript 审计通过，Rust 仍有既有 8 项许可证拒绝与 2 项维护策略问题，软件 RSA 继续禁用。全部验证进程与原生窗口退出后，clean-dev 清理 797.1 MiB；本轮约 16 GiB 唯一隔离目录已删除并确认不存在，保留 target/probes 验收证据。

## CLI 私钥 + MFA：原捕获、冷暖批准与命令链边界

`pulse ssh <保存的入口 ID> --identity <保存的私钥 ID> --key-interactive` 现在明确选择私钥与 MFA 联合；与账号 `--interactive` 互斥。闭合参数校验继续拒绝未知方法、null、别名、不匹配身份及配置跳板。暖请求消费原 `SavedSessionGesture` 后收紧，保留原页、Vault epoch、截止和取消，不重新捕获后续解锁；冷请求仍需原窗口解锁后单独查看目标，再批准精确目标和方法。转换不读取密码或私钥，后续复用已验收的实际部分成功门禁、原始单槽、轮次回答与 native 排空。

生产 `pulse`／WebView 和真实两轮 SSH 冷暖链通过：锁定阶段与目标阶段取消均零认证，解锁本身不是批准；问题 Esc 与暖 CLI 断开能撤销原请求，两次待认证流真实退出。冷暖各两轮联合成功，刷新前后服务收到输入、刷新不重认证，配置／密文哈希不变，原窗口正常退出且 CLI 端点排空；最终四条连接含两条已认证流全部退出。截图核对了真实问题与刷新后的终端。测试服务器只在隔离构建中修正锁定 SDK 丢失 partial-success 位的问题，来源与哈希见 `target/probes/cli-key-interactive-test-server.json`；生产 SDK、锁文件和 registry 缓存不改。

首轮前置曾因夹具脚本字面量、种子模式及身份标签不符而失败，尚未建立产品连接，不计通过。完整冷 CLI 又暴露最外层链预检未接新标志，原文 `pulse: unsupported command`，修复后补充窗口选择、连续动作与互斥标志反例。完整 CLI 26 项、认证任务十组共 46 项均非零通过；全仓 Rust 1138 项通过／7 项按既有条件忽略，前端 1427 项通过。最终 `check`、`probe`、静态文案、产品构建、App strict 和 Native 全目标严格 Clippy 通过；新增测试格式失败已修复，最后 check 再次通过。Rust Astrolabe 精确诊断无报错；TypeScript 服务初始化失败不计通过，以正式类型与直接行为检查为证据。

任务 `probe-workspace-cli-key-interactive`、正式认证过滤器及 Linux CI 已注册。原始全仓、CLI、认证、strict、产品和实窗日志保留在 `target/probes/cli-private-key-interactive-development/`，最终收据为该目录 `receipt.json`。本批只覆盖受控 Linux loopback／软件渲染；跳板及其他方法联合、外部企业 MFA、硬件设备与跨平台仍待验收。JavaScript 审计通过，Rust 仍有既有 8 项许可证拒绝、2 项维护策略问题；软件 RSA 继续禁用。验证进程与原生窗口退出后，clean-dev 清理 797.1 MiB，本轮约 17 GiB 唯一隔离目录已删除并核验不存在；保留 target/probes 验收证据。

## SSH 直连私钥 + MFA：明确联合意图与实际部分成功门禁

保存主机与原页临时目标的私钥选择现在各有明确“私钥 + MFA”方式。独立原点击意图保留原页、Vault epoch、选定来源和固定截止；普通私钥意图不能升级。实际指纹固定后，仅在服务器确认私钥部分成功且继续声明 keyboard-interactive 时进入问答。普通拒绝、未声明下一因素及仅私钥完整成功均不能满足联合策略；配置跳板不得借联合直连绕过保存路线。Core 两项与 SDK 方法门禁一项直接反例均非零通过，57 项受影响前端检查、TypeScript 和全仓检查通过。

Linux 生产 WebView 与真实 loopback SSH 通过十段合成，原始收据 `target/probes/workspace-key-interactive-result.json`：解锁不授权认证；普通私钥部分成功不自动 MFA；第一因素拒绝不回退；仅私钥成功不跳过联合策略；Esc 取消并排空；错误第二因素拒绝且零 Root 会话；保存身份完成两轮 MFA；真实重载恢复终端而不重复因素；临时私钥联合成功且暂不保存、元数据和密文不变；正常关窗等待全部七条连接及原生工作退出。两个成功会话的真实终端输入回声已核对，成功截图已审阅。合成密钥为加密 Ed25519，不扩称 RSA、所有私钥格式、CLI／跳板／Agent／密码联合、外部 PAM／企业认证、系统 IME、完整辅助技术或跨平台完成。

锁定 russh 0.63.3 测试服务器会把 signed-public-key 的 partial-success 位清零。本批只在 `/data/temp` 隔离解压、校验原锁定归档摘要并修正测试服务器这一处写包，普通拒绝仍清零。生产客户端与依赖不修正，registry 缓存和仓库锁文件不改；原始／修正源码及测试二进制摘要保留在 `target/probes/key-interactive-test-server.json`。构建流程与安全边界见 [私钥联合认证](../architecture/ssh-private-key-interactive.md)。

最终 `mise run check`、全仓 `test`（Rust 1138 项通过、7 项既有条件忽略，前端 1424 项通过）、`probe`、正式认证任务十组共 42 项非零测试、文案检查和 Native 全目标严格 Clippy 均通过。最终产品构建、原生命令和 Linux CI 注册已核对；Biome 报告 38 项 warning，不记为零告警。Astrolabe 对 Native 模块返回 Exact 无匹配诊断；TypeScript 服务初始化失败返回 Unknown，未计通过且未安装替代服务。Rust 审计仍有 8 项许可证拒绝和 2 项维护策略问题，JavaScript 无漏洞，不放宽门禁。

正式日志及聚合收据保留在 `target/probes/ssh-private-key-interactive-development/`；原始测试、认证和 check 日志副本与原件 SHA-256 一致。验证任务、窗口和 SSH 服务全部退出、证据归档后执行 `mise run clean-dev`，清理 2082 个文件、797.1 MiB；约 18 GiB 隔离目录 `/data/temp/pulseterm-key-mfa.lhtIW1` 已验证删除，十段实窗、截图、测试服务器来源及正式检查日志保留。

## 临时 SSH 目标 MFA：原页句柄、明确方法与可选保存

临时目标表单现在允许账号身份明确选择交互认证／MFA；准备阶段先解析原页临时 UUID，再沿用受监督的账号元数据来源。实际连接指纹核对、原页／Vault epoch、临时句柄存活与一次性回答轮次贯穿认证；不读取保存密码，不自动回退。Core 新反例分别验证准备前和准备后撤销均拒绝认证，适配器零调用且主机目录不变。

Linux 生产 Tauri WebView 与真实 loopback SSH 服务通过六段合成验收，原始收据 `target/probes/workspace-temporary-interactive-result.json`：解锁不启动 SSH；Esc 取消并排空且零写入；错误回答拒绝且无密码回退或保存；两轮成功后“暂不保存”，保留终端并刷新且不重复认证；第二次成功后明确保存，复用精确已有目标且不写凭据；正常关窗等待全部四条连接退出。成功截图已审阅。本批保存分支验证精确已有目标复用，不扩称全新主机创建、临时私钥、外部 PAM／企业 MFA、系统 IME、完整辅助技术或跨平台完成。

`mise run check`、全仓 `test`（Rust 1138 项通过、7 项既有条件忽略，前端 1416 项通过）、`probe`、正式认证任务九组共 39 项非零测试，以及 Native 全目标严格 Clippy 均通过。`check` 包含 TypeScript、双语静态检查及 Biome；Biome 报告 37 项 warning，未计为零告警。生产资源及原生构建通过，验收任务已接入 `.mise.toml` 与 Linux CI。Astrolabe 在两处 Rust 模块返回 Exact 无匹配诊断；TypeScript 服务初始化失败并返回 Unknown，后者未计通过且未安装替代服务。Rust 审计仍有 8 项许可证拒绝和 2 项维护策略问题，JavaScript 无漏洞，未放宽门禁。

聚合收据及正式认证、strict 日志保留在 `target/probes/ssh-temporary-interactive-development/`。本轮验证与原生进程退出、证据归档后执行 `mise run clean-dev`，清理 2091 个文件、797.1 MiB；约 17 GiB 隔离目录 `/data/temp/pulseterm-temporary-mfa.gd5dl5` 已验证删除，六段实窗结果、成功截图及正式检查日志保留。

## SSH 保存路线逐跳 MFA：明确方法、角色及原生排空

账号身份在每跳的实际指纹确认中明确选择保存密码或 MFA，不自动改用认证方式，也不修改保存配置。GUI 与原窗口批准后的普通 CLI 路线共用问题协议；CLI 全局 `--interactive` 仍限定直连，不能绕过保存路线。转换消费原账号捕获并保留原页、Vault epoch、来源和截止时间；中间跳只能转发冻结的下一目标，申请终端在适配器调用前拒绝，叶跳拒绝转发。回答绑定当前跳／请求／一次性轮次，发送、取消或关闭清空；服务器问题保持不可信字面数据。

Linux 生产 Tauri WebView 与三个真实 loopback SSH 服务通过九段合成验证，原始收据 `target/probes/route-interactive-result.json`：冷启动 MFA／保存密码／MFA 混合路线，暖启动全 MFA 和两轮回答，刷新前后真实终端输入且不重认证，三个层级分别 Esc 取消并等待所属父／叶退出，错误叶回答拒绝且无密码／私钥回退，首次 pin 精确保存以及其余配置／密文不变，最后正常关闭等待全部父连接、转发通道和叶连接退出。成功截图已审阅；每个父连接的打开／退出及通道转发／退出均对应核验，未使用不存在的聚合事件计数。

`mise run check`、全仓 `test`（Rust 1138 项通过、7 项既有条件忽略，前端 1414 项通过）、`probe`、文案检查、正式认证任务九组共 38 项非零测试、App／Store 及桌面 Native 全目标严格 Clippy 均通过；生产资源与原生二进制构建通过。聚合证据见 `target/probes/ssh-route-interactive-development/receipt.json`。Astrolabe 精确服务不可用，未计通过且未安装替代服务。Rust 审计仍有 8 项许可证拒绝和 2 项维护策略问题，JavaScript 无漏洞，未放宽门禁。

验收证据归档且验证进程退出后执行 `mise run clean-dev`，清理 797.1 MiB；本轮约 18 GiB 隔离目录 `/data/temp/pulseterm-route-mfa.f2Nokw` 已验证删除，成功截图、原始九段结果及正式认证日志保留在 `target/probes/`。
本批证据限定隔离私库、受控服务和 Linux 软件渲染 WebView；临时目标 MFA、完整多方法联合、外部 PAM／企业 MFA、系统 IME、完整辅助技术及跨平台仍开放。既有直连及软件 RSA 阻断边界保留；较早记录中的“跳板未交付”仅描述当时状态。

## CLI keyboard-interactive／MFA：冷暖原窗批准与取消排空

`pulse ssh <保存入口 ID> --identity <保存账号 ID> --interactive` 明确选择 MFA；省略标志仍使用原有分派，不自动尝试或降级，不接受原始地址、回答、密钥、命令或自动解锁参数。冷暖捕获沿用原页、Vault epoch、元数据源和原截止；已配置跳板、私钥／Agent 与 MFA 混用明确拒绝。实际 peer 指纹固定后才向原窗口定向传递有界问题，服务器文字保持字面数据，回答不进入配置或日志。

Linux 生产 `pulse`／Tauri WebView 与真实两轮 russh 服务的合成检查已通过，收据 `target/probes/cli-interactive-result.json`：锁定取消、解锁不授权、目标取消零认证、问题 Esc 取消、暖 CLI 进程退出、冷暖两轮明确回答、刷新前后真实终端输入且不重拨、来源文件哈希不变、原窗口正常退出以及端点与物理连接排空；共两条认证连接和两条取消连接。截图已审阅。受控服务和软件渲染不建立外部 PAM／企业 MFA、系统 IME、辅助技术完整兼容或跨平台验收。

实窗发现并修复了两处直接检查遗漏：Esc 只关闭原生 dialog、未结束回答 Promise；MFA 启动等待未监听 CLI transport 的取消。现将 cancel／close 与局部回答信号联动，并监听调用方断开；CLI 及时收到拒绝，银行拥有的 native 启动任务继续持有 token／permit 直到真正退出，成功收养的 Root 会话不因 CLI 等待结束而关闭。57 项前端直接检查通过；早先零项 capture 过滤结果明确撤回，改用实际模块路径执行四项原页／epoch／源／过期检查。正式认证任务及 Linux CI 已覆盖同一入口，七组共 33 项非零检查通过。最终 `mise run check`、全仓 `test`（Rust 1138 项通过、7 项按既有条件忽略；前端 150 文件／1409 项通过）、基础 `probe`、文案检查、生产前端构建及产品组合严格 Clippy 均通过。Astrolabe 对 Rust 分派返回 Exact／零诊断，TypeScript 服务初始化失败，不计通过、未安装额外工具。Rust 审计仍有既有 8 项许可证拒绝、2 项维护策略问题，JavaScript 零漏洞，发布门仍阻断。聚合收据为 `target/probes/cli-interactive-development/receipt.json`；验证进程均已退出；`mise run clean-dev` 清理 797.1 MiB，本轮约 20 GiB 的隔离临时目录已删除并核验，`target/probes/` 中的实窗及聚合证据保留。

## SSH NIST ECDSA：格式、密文重开与实际认证

在原有 Ed25519 上增加 ECDSA NIST P-256、P-384、P-521，覆盖 OpenSSH、明文与有界加密 PKCS#8 PEM。导入和实际使用共用算法检查；PKCS#8 在算法专用构造前检查 OID、命名曲线和载荷大小。保存保留原始格式并加密，解析与解密沿用既有 CPU／口令／KDF 预算，签名前仍核验原页、Vault epoch、源快照和实际 peer。中英文导入提示已同步，原凭据与显式操作保持兼容。

生成式检查覆盖三种曲线的格式往返、公钥一致性、真实签名、口令和 RSA／未知曲线拒绝；真实 loopback 矩阵对三曲线分别使用加密 OpenSSH、加密 PKCS#8 来源，验证 SQLite 密文重开、重新显式解锁、源变更零认证、同一 Root 收养／刷新恢复、EOF／主动关闭与 SDK／物理连接排空。修正了开发夹具重开后复用旧 reader 解锁凭证的错误假设，产品 reader 绑定未放宽。六组收据为 `target/probes/ssh-private-key-session-nistp{256,384,521}[-pkcs8].json`，均标记 `realWebViewChecked=false`；不作为新算法原生 WebView、外部 SSH 或跨平台验收。

软件 RSA 未交付：启用锁定库功能会引入 `rsa 0.10.0-rc.18` 的未修复时序侧信道漏洞 `RUSTSEC-2023-0071`。本批撤回新增 RSA 特性及测试依赖，保留拒绝反例，不增加忽略项。正式任务及 CI 已挂接算法矩阵；最终 ring-only 的 `mise run check`、`test-ssh-authentication`（2 项原页／元数据、8 项格式、6 项 broker／真实 SSH）、六组算法探针、文案检查及桌面产品组合严格 Clippy 均通过；25 项直接前端检查与生产前端构建通过。Astrolabe 对算法准入与解码源码返回 Exact／零诊断。实际工作区全功能依赖图已无 `rsa` 包；最终审计回到原有 8 项许可证拒绝、2 项维护策略问题，JavaScript 零漏洞，发布门仍阻断。撤回最后一个 RSA 依赖入口后的全仓 `mise run test`／`probe` 再次通过：Rust 1138 项通过、7 项按既有条件忽略，前端 1394 项通过；验证进程已退出；`mise run clean-dev` 清理 870.6 MiB，本轮约 15 GiB 隔离临时目录已移除，六份算法报告及其他 `target/probes/` 证据保留，完整收据归档于 `target/probes/ssh-ecdsa-development/receipt.json`。

## 连续 P0：终端双语、命令目录、自定义 Profile 与 SSH 认证

本批连续接通终端历史／链接／粘贴与 CLI 确认、响铃／返回输出和目录状态双语；命令目录扩充至 16 项，新增入口复用原授权且分屏绑定原两个会话，原键位协议保持封闭。Linux 原生面板→快捷键→设置→终端返回已通过。Vite 保留空 nonce 样式标记，保持严格 CSP；布局成功后仅对已验证回执接受旧连接已经撤销，未确认布局仍失败。

自定义 Profile 提供独立 UUID、创建／复制／编辑、隐藏、删除／恢复、默认选择及 GUI／CLI 启动，使用真实私库重开、CAS／HLC／原页撤销和 PTY 字面参数检查。Linux 实窗验证保存零启动、GUI 明确打开、独立进程重开保留同一 UUID、CLI 身份目录和原窗口批准、刷新不重复启动及 native 退出；收据 `target/probes/workspace-custom-profile-result.json`。修复了保存后侧栏与目录读取争用单槽，串行刷新反例和 23 项组件检查通过；终端另有 26 项组件检查，Profile／默认／CLI 直接检查 70 项。

Ed25519 PEM 的四项生成式检查覆盖 OpenSSH／明文 PKCS#8 身份一致性、加密 PKCS#8 两种 PRF 和两种 AES-256 方案、口令、预算及多块拒绝。直接 MFA 增加原页／epoch／元数据两项、broker 三项和真实 loopback SSH 三项；实际服务验证两轮接受／拒绝、无回退、取消／源变更零回答、问题／轮次限制和错误指纹，且 native 连接归零。SDK 在 AuthInfoRequest 等待时忽略 Disconnect，已改为先撤销网络再释放本地等待，并始终等待 SDK 与物理 I/O 退出；六项普通 peer 关闭回归通过。

MFA 前端 52 项直接检查与 16 项 Chrome 组件检查通过；覆盖明确方法、账号未保存密码仍可选、重复问题、单次截止、输入回显／隐藏、字面服务器内容、中英文保留草稿与焦点、明确提交清空、Esc／迟到响应和 390px 无横向溢出。生产 Linux WebView 与隔离真实 SSH 的五阶段合成检查随后通过：解锁不授权 SSH、Esc 后未认证连接退出、明确重试两轮回答、真实终端输入与刷新不重复认证，以及关窗后 SDK／物理连接排空。共认证一次、退出两条原生连接，收据 `target/probes/workspace-ssh-interactive-result.json`；这些不算外部 PAM／企业 MFA、系统 IME、辅助技术或跨平台验收。

最终 `mise run check`、全仓 `mise run test`、`mise run probe`、`workspace-language-check`、`test-ssh-authentication`、App／SSH 与桌面产品组合严格 Clippy、类型及生产构建通过。静态文案任务覆盖 138 个生产源码模块和 423 条字面消息，补齐已删除分组、分屏会话选项及比例辅助名称；38 项相关回归验证语言更新不重建 pane、不改比例或发起 resize。额外 `xtask --features ssh-private-key-probe --all-targets` 严格组合因共享产品适配器未使用入口失败，未计为通过；没有放宽 lint。Rust 依赖审计保留 8 项许可证拒绝与 2 项维护策略问题，JavaScript 审计零漏洞，发布门未解除。

已新增 [PEM 格式](../architecture/ssh-pem-format.md)和 [MFA 授权／排空](../architecture/ssh-interactive-session.md)说明，并把认证任务接入 Linux CI。MFA 当前仅限保存主机直接连接，不绕过临时／跳板配置；其他算法、联合认证和 CLI MFA 未交付。完整 Alpha、UI-05、跨平台和发布门仍开放。

轻量聚合收据归档于 `target/probes/alpha-p0-continuous-development/receipt.json`；原生 Profile 与 MFA 分别保留独立收据。验收服务、测试标签页和原生进程全部退出后执行 `mise run clean-dev`，移除 2358 个文件／1.4 GiB 开发产物；本批临时目录及编译目录已移除，`target/probes/` 证据保留，清理后复核通过。

## 独立指纹、SSH 配置导入与快速连接中英文（开发验证）

接通独立主机指纹的首次／匹配／变更、保存与失败提示，保留取消默认焦点、变更密钥禁止替换及不认证／不连接的语义。ssh_config 读取、预览、IdentityFile 数量、源文件核对、准备导入和显式保存使用完整消息；名称与别名保持原值。快速连接覆盖 IPv6 多行目标、已有／重复主机、临时目标和连接后保存，语言切换保留原输入、焦点、确认状态，不写入、不重读或批准；沿用原票据、源检查、失败不重试及迟到响应处理。

五组 43 项直接测试、TypeScript、局部 Biome 和最终产品构建通过。Biome 返回成功，仍有 15 项 CSS 层叠特异性警告。补齐现有导入测试夹具的 ownerDocument，未新增依赖或扩大测试矩阵。Astrolabe TypeScript 引用／诊断服务初始化失败，未计为通过，未安装额外工具。

Chrome 的 34 项组件检查通过：语言切换不产生写入或重复观察，用户名称和指纹保持原样且不插入 HTML，首次默认取消与显式保存、匹配／变更拒绝保存、Esc／迟到响应、配置草稿和 IdentityFile 提示、导入准备／保存与源变化、已有主机不重复保存、重复目标阻断、临时目标显式准备、连接后保存失败不重试及迟到结果不发布。英文正文与辅助名称扫描排除已独立断言的用户字面名称；四处 390px 布局检查包含外层、表单、字段和动作组。截图审阅通过，动作按项换行，目标及长指纹完整显示。

收据：`target/probes/connection-language-development/receipt.json`。组件使用生产 HTML／样式／控制器与模拟 I/O，不代表真实 ssh_config 文件读取／导入、指纹信任持久化、SSH 连接、原生 WebView、辅助技术、IME 或跨平台验收。本批只改前端，未重复 Rust／PTY／真实网络矩阵；沿用现有 CI 测试发现与构建。UI-05 继续推进终端查找／链接／粘贴等专用弹窗和剩余错误文案。

## 本地 Profile 与组织管理中英文（开发验证）

接通 Profile 显示、默认选择／保存／启动前重新确认、删除／恢复及完整参数／工作目录／环境变量／外观行为表单；组织管理接通分组路径、标签／收藏、成员分配、删除后果、终端配色／键行为与继承来源。动态文案使用完整嵌套消息，名称、路径、参数和环境变量值保持字面数据。语言切换不保存、不删除、不启动，保留草稿、选择、原控件和焦点；沿用原页面票据、失效和迟到响应处理。

八组 56 项直接测试、TypeScript、局部 Biome 和最终产品构建通过。Biome 返回成功，CSS 仍有 15 项层叠特异性警告。新增回归检查区分关闭动作 Close 与开关 On／Off，避免共享目录词义冲突。Astrolabe TypeScript 引用／诊断服务初始化失败，未计为通过，未安装额外工具。

Chrome 使用生产 HTML／CSS／控制器与模拟 I/O，40 项组件检查通过：保存重开、空参数／路径／环境变量保留、增删参数辅助名称编号、默认重新确认、删除恢复后果、语言切换不写入或启动、分组名称与标签、继承来源／false 与空覆盖、失败显式重读、Esc 与迟到响应、英文正文／ARIA及 390px 布局。扫描排除夹具中有意与文案同名的字面名称“保存”，并独立断言其原样保留。截图复核发现组织内部按钮撑宽，修复按钮换行及边框盒后，扩展检查到字段区、表单和动作组，无横向滚动。

收据：`target/probes/profile-organization-language-development/receipt.json`。组件保存／重开使用模拟存储，不建立真实 Profile 文件、SQLite／PTY、SSH、原生 WebView、辅助技术、IME 或跨平台结论。本批只改前端，不重复对应矩阵；既有 CI 自动发现新增契约断言，无新增依赖或任务。UI-05 继续推进独立指纹、导入／快速连接、终端专用弹窗及剩余错误文案。

## 身份与 Vault 中英文（开发验证）

接通密码身份、私钥身份及导入、Agent 身份和 Vault 创建／解锁／锁定的标题、说明、状态、错误、动作与辅助名称。共享身份弹窗按入口切换标题；私钥与口令状态使用嵌套消息，Agent 删除确认使用字面用户参数。语言切换保留敏感输入、草稿、焦点和选择，不提交、不读取 Agent、不签名、不创建／解锁或批准 SSH 使用凭据。原提交、失败、关闭和撤销后的输入／缓冲清除规则继续生效。

五组 50 项直接测试通过，覆盖密码／私钥清除、失效表单、旧页／迟到响应、Agent 协议与文案占位符。TypeScript、局部 Biome 和最终产品构建通过。Astrolabe TypeScript 初始化失败，未计为诊断通过，未安装额外工具。

Chrome 使用生产 HTML／样式／控制器和模拟依赖，25 项组件检查通过：中英正文／ARIA、原样名称和指纹、密码／私钥／口令／主密码与焦点保留、显式保存和清除、Vault 创建后锁定／解锁后未授权提示、显式锁定、失败重读、Esc 与迟到响应，以及四种 390px 布局。三处敏感弹窗统一边框盒尺寸，Agent 复用表单布局并让长指纹完整换行。

收据：`target/probes/identity-vault-language-development/receipt.json`。组件模拟不代替真实密码派生／加密保存、Agent／SSH 网络、原生 WebView、辅助技术、物理键盘／IME 或跨平台验收；本批未扩大 Rust、PTY 或网络矩阵。UI-05 继续推进本地 Profile、组织、独立指纹、导入／快速连接和终端专用弹窗。

## SSH 主机、配置与连接确认双语（开发验证）

沿用用户层语言选择，接通 SSH 主机／多入口管理、SSH 配置的名称／默认身份／跳板、连接选择和逐跳目标／实际指纹确认。共享目录的搜索、批量选择、数量和删除后果提示接入同一文案机制；输入、名称、地址、端口、算法与指纹保持字面数据。切换只更新文本与辅助名称，保留草稿、焦点、身份禁用状态和待确认连接，不触发保存、删除或授权。修复三处弹窗在 390px 下的盒模型与长指纹换行。

受影响七组共 72 项前端测试、TypeScript、局部 Biome 和产品构建通过。Chrome 使用真实 HTML／CSS／控制器与模拟 I/O，检查保存重开、失败表单失效、草稿与焦点、批量删除、不可用身份、逐跳实际指纹、语言不批准连接、Esc 拒绝、迟到响应、字面数据不插入 HTML，以及英文正文／ARIA 遗漏扫描和三处窄窗口布局。收据位于 `target/probes/ssh-language-development/`。

Astrolabe TypeScript 服务初始化失败，Unknown 未计通过，也未安装额外工具。本批只改前端，未重复 Rust／PTY／真实 SSH 网络矩阵。原生 WebView、辅助技术、IME、真实网络与跨平台尚待集中验收；Identity／Vault／本地 Profile／组织管理、独立指纹、导入／快速连接、终端专用弹窗和完整错误文案仍需补齐，**UI-05 未整体完成**。沿用现有 CI 的测试发现与产品构建，无新增依赖或验证矩阵。

## 工作区语言选择与首批双语（开发验证）

用户层 `ui.language` 接通简体中文／English、来源、显式保存、重开读取和恢复继承，设置投影升级至 v15，兼容旧版本。工作区框架／状态、全部设置字段与预览、快捷键来源确认／规范化同意，以及命令面板支持双语。设置和命令保留中英文检索；保存后即时切换并恢复其他设置草稿、预览、筛选与焦点。读回同样应用后端语言；迟到响应与已撤销页面不能切换。远端输出、用户名称、输入、语言自称和字体样本保持原文，文案不按 HTML 插入。详见[工作区语言](../architecture/workspace-language.md)。

受影响的 77 项前端、2 项语言存储、18 项原生设置检查实际运行并通过，包含真实文件重开、配置冲突、旧页拒绝、恢复及三种 TOML 形式／注释保留。类型、局部格式、schema 生成／漂移、Store 全目标与桌面库严格 Clippy、产品构建通过。Chrome 使用真实 HTML／样式／组件与模拟 IPC，检查保存边界、英文来源／ARIA、草稿与焦点、混合搜索、Esc 返回、失败重读、迟到响应、整页遗漏扫描与 390px 布局；收据位于 `target/probes/workspace-language-development/`。

Rust Astrolabe 返回 Exact、零诊断；TypeScript 服务初始化失败，未计通过。未扩展 Rust 全工作区、PTY 或 SSH 网络矩阵。原生 WebView、辅助技术、物理键盘／IME、系统菜单与跨平台尚待验收；管理／导入／连接及终端专用弹窗和完整错误文案仍需国际化，**UI-05 未整体完成**。沿用既有 CI 自动发现新增测试，无新增依赖或验证矩阵。

## 系统深浅终端主题策略（开发验证）

全局设置新增固定／跟随系统方式，以及分别选择浅色和深色配色的局部预览；20 组设置支持保存／恢复。`terminal.theme` 整组原子保存，保留独立固定配色，恢复继承移除策略；手写内联、点键和 `[terminal.theme]` 分表均可编辑并保留相邻配置和注释。后端通过原窗口系统外观选定新会话基色；Profile／直属组／SSH 入口固定覆盖继续优先，已有会话保持启动时颜色。未知原生外观采用浅色分支。见[终端配色](../architecture/terminal-color-presets.md)。

验证限于直接影响范围：38 项前端测试、2 项存储策略检查、17 项原生设置和 1 项真实 PTY 探针通过。PTY 同一探针验证 Profile 覆盖、全局系统深色和全局系统浅色的实际 OSC 11 回应；原生文件检查验证整组保存、重新打开、恢复和旧页面拒绝。新增标准分表后复跑原 2 项存储检查；另 1 项现有目录测试用于输出浏览器使用的后端序列化 RGB 数据。最后前端说明调整只复跑预设文件。TypeScript、局部 Biome、Rust 格式、schema 生成／漂移、产品构建及 Store／App 全目标、CLI＋SSH 桌面库严格 Clippy 通过。现有 CI 的相关测试和 schema 检查自动覆盖新策略，没有扩大 SSH 网络矩阵。

Chrome 组件使用真实编辑器、CSS、后端色板与内存保存适配器，覆盖搜索入口、草稿零写、双预览、整组保存、重开、失败后显式重读、恢复继承、Esc、关闭后的迟到响应和 390px 单列布局。它不证明原生 WebView、真实系统外观变化、GUI／CLI 多窗口、SSH 网络或跨平台验收。Astrolabe Rust 返回 Exact、零诊断；TypeScript 服务初始化失败，不计通过，也没有安装工具。轻量收据见 `target/probes/system-terminal-themes-development/receipt.json`。

## 命名内置终端主题目录（开发验证）

新增 One Dark/Light、Catppuccin Mocha/Latte、Dracula、Solarized Dark/Light 七套固定配色；原午夜／纸白保留 ID 和 RGB，显示为 PulseTerm Dark/Light，连同经典黑底共十套。全局、本地 Profile、直属分组与 SSH 入口复用同一目录及既有稀疏保存／继承。预览只读后端绘制数据，ANSI 原色以色块显示，文字采用主题正文色；全部十套基础正文对比度 ≥4.5:1。新会话在后端冻结配色，已有会话维持原值。见[终端配色](../architecture/terminal-color-presets.md)。

验证限于受影响范围：41 项前端测试、2 项目录保存／正文对比度、3 项组织存储、5 项 Profile 存储、16 项原生设置、1 项 Actor 和 1 项真实 PTY 检查通过。新增预览色块后只复跑该文件 11 项前端测试；原生单项另以 Dracula 保存／读回并保留旧页拒绝。真实 PTY 的 OSC 11 查询验证 Catppuccin Mocha Profile 覆盖以及移除后全局午夜继承。初次组织／Profile 命令因使用文件名过滤而为零项，未计通过；改用 `organization_behavior::tests`、`profile_editor::tests` 后实际分别运行 3、5 项。TypeScript、局部 Biome、Rust 格式、最终产品构建、schema 生成及漂移检查、Store／App 全目标和 CLI＋SSH 桌面库严格 Clippy 通过。

Chrome 组件使用后端序列化的十套 RGB，覆盖逐套背景／正文／ANSI 原色块、说明文字继承、局部预览零写、保存重开、恢复继承、搜索保留、390px 换行和无水平溢出、Profile 字段及 SSH 入口来源。初轮夹具遗漏生产弹窗类名的等待失败、通用 span 样式覆盖正文色的问题已修正并复核。Rust 配色源 Astrolabe 返回 Exact、无诊断；TypeScript 服务初始化失败未计通过。未新增依赖或运行完整回归、真实 SSH 网络、原生 WebView、辅助技术及跨平台矩阵；按系统分别指定深浅主题与 UI-05 完整中英文仍待实现。沿用既有 CI 的前端／存储／原生检查，schema 随目录同步。轻量收据保留于 `target/probes/builtin-terminal-presets-development/receipt.json`；本批浏览器、服务退出后执行 `mise run clean-dev` 并清理唯一临时目录。

## 设置中的快捷键入口与返回（开发验证）

快捷键入口纳入“键盘与剪贴板”分类及中文／英文关键词搜索，复用原键位编辑器与授权。进入时只挂起设置 DOM，返回保留筛选、未保存草稿、滚动和发起控件焦点；页面 nonce、epoch、销毁和其他模态限制旧返回回调，不续期来源票据。失效返回也不解除其他模态的忙状态。见[设置导航](../architecture/workspace-settings-navigation.md)。

直接检查通过：设置与键位编辑器 24 项测试、TypeScript、局部 Biome 和前端产品构建。Biome 保留 workspace-main.ts 既有非空断言 warning。Chrome 使用生产编辑器、导航、返回模块及 CSS，配受控 IPC，检查搜索／分类零写入、单层模态、物理 Esc／Tab、草稿／焦点保留、明确保存、来源失败后重新进入、迟到旧读取与 390px 无横向溢出。原生探针同步了搜索后三次 Tab／回车、冲突后返回重读和两次 Esc；相同键盘路径经 Chrome 检查，脚本仅做 bash -n，未重跑实窗。

Astrolabe TypeScript 初始化仍失败，未计为诊断通过，也未安装工具。首次构建误用了不存在的配置名，修正为现有 vite.config.ts 后通过；失败不计为验收。轻量收据见 target/probes/settings-keymap-development/receipt.json。本批未改 Rust／IPC／配置格式，未跑全量、Rust 或 SSH 矩阵；组件结果不证明真实 Tauri IPC、原生 WebView、辅助技术或跨平台验收。既有前端 CI 自动覆盖测试、类型与构建，保留原生探针待集中执行。

## Profile／分组／SSH 入口终端配色（开发验证）

三套固定终端配色已接通本地 Profile 与组织管理的“终端配色与行为”。Profile 使用稀疏 `appearance.colorPreset`，旧记录缺省或显式 null 继承全局；组织覆盖 v2 必含 `terminal_preset` 及原有四项行为，顺序为默认→用户→直属组→SSH入口，逐项 null 恢复。保存、重开和恢复沿用原页票据、组织源与 SQL CAS；后端在新 PTY／SSH 启动前冻结基色，现有会话与刷新不在线换色。见[终端配色预设](../architecture/terminal-color-presets.md)。

直接检查通过：40 项受影响前端行为、3 项组织继承／冲突／撤销检查、5 项 Profile 持久化／冲突检查，以及 2 项 App 检查。后两项包括单个真实 PTY 探针：先保存纸白 Profile 并查询 OSC 11，再移除覆盖并查询全局午夜；两次均收到准确基色回复。补齐分组恢复到用户来源后只复验对应既有用例；前端补充非法配色／缺字段拒绝后仅复验对应文件，均包含在上述数量，不重复累计。TypeScript、局部 Biome、Rust 格式、产品前端构建，以及 Store／App 全目标和 CLI＋SSH 桌面库严格 Clippy 通过。

Chrome 使用真实编辑组件和生产 CSS、受控 IPC，覆盖草稿零写、统一保存、重开、继承及来源、保存失败重读、旧表单、取消后的迟到响应、真实 Tab 和 390px 无横向溢出。此组件结果不代表真实 Tauri IPC／WebView、SSH 网络认证、跨平台、IME 或辅助技术验收；本批未跑全量回归或 SSH 矩阵。Astrolabe TypeScript 初始化失败未计诊断通过，未安装工具。

首次浏览器夹具 nonce 格式错误，修正为仓库 UUID v4 后通过；首次构建使用不存在的配置名，改用现有 Vite 配置后通过。严格检查还发现已有启动配置测试使用 expect，与 lint 冲突，已在同一相关测试中改为错误传播并断言冻结配色；扩大到桌面全部目标仍被 `pulse_main.rs:492–493` 的两处既有 CLI 测试 unwrap 告警阻断。上述失败均不计通过；未更改无关 CLI 测试。既有 CI 的 Store／App 测试、工作区前端构建与桌面库编译自动覆盖本批字段，不新增重型任务。轻量收据见 `target/probes/scoped-terminal-presets-development/receipt.json`。

构建、测试与浏览器退出并归档轻量收据后，对本批专用 Cargo 目录执行 `mise run clean-dev`，清理 9113 个文件、约 5.2 GiB；随后删除唯一临时目录 `/data/temp/pulseterm-scoped-palette.V95pgt` 并验证不存在。保留 `target/probes/`，未清理其他项目缓存。

## 固定界面强调色（开发验证）

“设置→外观与配色”新增青、蓝、绿、琥珀、红五种固定强调色，仅用户级 `[ui].accent`；默认青色。草稿只改变表单预览，成功回执后采用后端投影，恢复继承只移除强调色。启动与显式重读应用持久值；其他窗口仍待显式重读。v12 投影严格验证固定值及 default/user 来源，继续支持旧版投影。见[界面外观设置](../architecture/ui-appearance.md)。

直接检查通过：2 项存储、1 项数据库作用域和 41 项受影响前端行为；原生先运行 2 项 UI 真实文件设置检查，再因 v12 影响既有版本断言而限定复核同一设置模块的 16 项，全部通过（包含上述 2 项）。TypeScript、Rust 格式、Store／桌面库严格 Clippy、局部 Biome、schema 生成／漂移检查、产品前端构建通过，保留原有非阻断告警。前两次原生过滤命中零项，未计通过；核对源码后改为实际库模块 `product_workspace::settings::tests::`。Astrolabe Rust 超时、TypeScript 初始化失败均未计诊断通过，未安装工具。

Chrome 使用生产编辑器与 CSS、受控 read/save，覆盖预览零写、保存／恢复焦点与筛选、旧表单、失败重读、迟到结果、重新打开、真实 Tab 和 390px 布局。首次发现青色选中态约 3.95:1，随后发现琥珀预览文字约 4.497:1；改用固定第 12 阶文字色后十种浅／深组合均超过 4.5:1，预览文字最低 11.09:1、选中态最低 9.70:1。不算真实 Tauri IPC／原生 WebView、辅助技术、跨平台或终端验收；没有跑全量回归、SSH 矩阵或重复发行审计。轻量收据见 `target/probes/ui-accent-development/receipt.json`，沿用现有设置／前端 CI。

构建、测试、Clippy 与浏览器全部退出并归档轻量收据后，在本批专用 `CARGO_TARGET_DIR` 执行 `mise run clean-dev`，清理 14094 个文件、9.5 GiB；随后删除唯一临时目录 `/data/temp/pulseterm-accent.LtzqPc` 并验证不存在。保留 `target/probes/`，既有 Vitest 缓存未改动。

## 设置分类、搜索与键盘焦点（开发验证）

真实设置弹窗的 18 个字段按四类组织，支持中文／英文标签与配置键、多词交集、结果数／空结果及清除筛选。保留原单字段保存／恢复与来源，成功重绘保留筛选和操作焦点，关闭清空筛选并使迟到响应失效；不确定保存仍清空字段并要求重读。默认终端字号保持 14px。设计见[工作区设置导航](../architecture/workspace-settings-navigation.md)。

设置编辑器、协议与外观的 28 项既有直接行为、TypeScript、局部 Biome 与产品前端构建通过。第一次静态检查发现分类元组推断问题和新增 DOM 属性与旧外观夹具不兼容，已修正后通过；未放宽断言。TypeScript Astrolabe 服务初始化失败，未计诊断通过、未安装工具。Biome 保留受影响文件原有警告，构建保留既有静态／动态导入提示。

Chrome 受控 read／save 组件检查通过四类分组、筛选零写入、中文与多词搜索、保存禁用／焦点恢复、继承恢复、旧 DOM 拒绝、空结果文本安全、实际 Tab／方向键、关闭后的迟到结果、重新打开及失败禁用。初次夹具未进入 Vite 的 HTML 转换链导致页面等待超时；修正夹具后通过，不作为产品错误。截图审阅发现窄窗口双重滚动后，改为仅字段区滚动并复核完成按钮可见；桌面浅色与 390px 深色均无横向溢出。轻量收据见 `target/probes/settings-navigation-development/component.json`。

沿用现有前端 CI，未跑全量回归、SSH 矩阵或重复发行审计；本批没有后端／依赖变化。组件适配器受控，不计真实 Tauri IPC、配置文件持久化、原生 WebView、辅助技术和三平台验收。

组件／浏览器与构建全部退出后执行 `mise run clean-dev`，清理 2129 个文件、972.2 MiB，保留 `target/probes/`；独占临时目录及本批新建的 Vite 优化缓存已删除并核对。

## 分组／SSH 入口终端行为覆盖（开发验证）

统一编辑 TERM、Option／Meta、Backspace、Delete，按默认→用户→直属组→保存入口继承，支持逐项恢复和一次保存。组织源、成员关系和两层设置版本在共享 SQL 事务内校验；原页撤销、缺字段或冲突拒绝。GUI／CLI 直连与跳板叶终端采用覆盖，注册 SSH 会话保留按键偏好用于刷新恢复。4 项后端与 11 项受影响前端直接行为、类型、格式、局部 Biome、Store／App／桌面库严格 Clippy 和产品构建通过；Astrolabe 对新增 Store 模块返回 Exact 零问题。一次 Chrome 受控组件检查覆盖两类表单、继承／显式关闭、重复抑制、冲突重读、旧页响应与720px布局，不代替原生 IPC、真实 SSH 或跨平台验收。沿用现有 CI，未运行全量回归或 SSH 矩阵。范围见[实现说明](../architecture/terminal-scoped-behavior.md)，轻量记录见 `target/probes/scoped-behavior-development/`。祖先组叠加、原生 WebView／PTY／SSH 与跨平台行为仍待验收。
## TERM、Delete 与 Profile 覆盖（开发验证）

全局 TERM 固定选择、Delete CSI／DEL 与 Shell Profile 的 Option／Backspace／Delete 覆盖已成组接通，配色／容量／TERM同次读取后冻结，新会话生效。4项后端、46项前端直接行为及TypeScript、schema、格式、局部Biome、Store／App／桌面库严格Clippy和产品构建通过；未跑完整回归。一次真实Chrome／xterm组件检查确认Delete字节、修饰键／粘贴保真、输入门禁和Profile单次保存，不含PTY／SSH或原生IPC。Store诊断Exact零问题；TypeScript服务不可用不计通过。现有CI覆盖，无新增矩阵。记录见 `target/probes/terminal-behavior-development/`，范围见[实现说明](../architecture/terminal-behavior.md)。组／主机产品入口、真实TERM／PTY和跨平台物理键效果仍待验收。
## Backspace DEL／BS（开发验证）

全局选择框、单字段保存／恢复继承、v10封闭投影和可信普通Backspace改码已接通，默认DEL，下次打开或重建视图生效。1项Store和1项原生检查、26项受影响前端行为、TypeScript、schema、局部Biome、格式、Store／桌面库严格Clippy及产品构建通过；未跑完整回归。真实Chrome／xterm按键检查确认DEL／BS、修饰键及粘贴保真和输入门禁；无PTY或原生IPC。Store诊断Exact零问题。开发日志及源码摘要见 `target/probes/backspace-development/`，范围见[实现说明](../architecture/terminal-backspace.md)。分组／主机产品入口、CR／LF及原生／跨平台物理键效果仍待完成。

## macOS Option／Meta（开发验证）

全局开关保存、恢复继承、v9封闭投影和xterm公开选项已接通，默认false，下一次打开或重建视图生效。两项Store检查、一项原生保存／恢复／旧页撤销检查、27项前端行为、TypeScript、schema、局部Biome、格式、Store／桌面库严格Clippy及产品构建通过；未跑完整回归。Store诊断为Exact零问题，局部Biome保留一项既有警告和一项既有信息提示。开发日志及源码摘要见 `target/probes/option-meta-development/`。

分组／主机验证仅覆盖存储继承与持久化，产品编辑入口仍待完成。未运行完整回归或SSH／多窗口矩阵，未验收真实macOS物理Option、系统IME或原生WebView。详见[实现说明](../architecture/terminal-option-meta.md)。

本文记录可复现证据，不替代架构决策或验收清单。验证只在当前 Linux 环境执行；已添加三平台 CI，但尚无 CI 执行结果。

## 带空格的绝对文件路径（开发验证）

TERM-10 扩展完整单／双引号路径，行列后缀可在引号内或外；复制去掉包围引号和行列，保留路径中的普通空格、变量文字和 Windows 反斜线。未闭合／嵌套／粘连词元不截取部分路径；仍不读取文件、不调用编辑器或浏览器。

34 项受影响前端行为（首次通过项加唯一失败项定向续跑）、TypeScript、五份源码局部 Biome 和产品构建通过。首次失败夹具将 Tab 解析后的单元格空格误当原始控制符，已修正；原始字符串控制符拒绝检查保留。真实 Chrome／xterm 组件检查覆盖引号 Unix／Windows 路径、行列展示、完整字面复制、失败／退场处理和网页兼容，确认零系统打开、零输入；剪贴板适配受控。TypeScript 语言服务不可用，未计通过、未安装。沿用现有前端 CI，不新增矩阵；不跑完整回归。原生 WebView、系统剪贴板、真实 PTY／SSH、性能和跨平台仍待验。

## 绝对文件路径检查与复制（开发验证）

TERM-10 新增 Unix／Windows 盘符绝对路径及行列后缀识别，文件目标只读展示并显式复制，网页和文件来源分开；不读取目标文件，也不交给系统浏览器或外部编辑器。

29 项直接行为覆盖解析、无效地址／路径边界、引号跳过、软换行单元格位置、文件名标点保真及退场拒绝；类型、6 份改动文件局部 Biome 与产品构建通过。真实 Chrome／xterm 组件检查覆盖路径与行列分离、受控剪贴板复制与失败、合成点击拒绝、网页兼容、OSC 8 文件目标拒绝、窗格退场、720px 布局和零输入／零系统打开。未跑完整回归；组件结果不替代系统剪贴板、原生 WebView、PTY／SSH 或外部编辑器验收。

## IP:端口链接识别（开发验证）

TERM-10 补齐严格 IPv4／方括号 IPv6 与端口的网页候选识别，使用现有检查、复制和受限系统打开入口。默认解释为 HTTP，界面显示完整规范化目标并明确协议假设；不推断端口服务、不自动访问目标。

23 项链接直接行为通过（首次22项及修正后1项定向续跑），覆盖 IPv4／IPv6、端口范围、无歧义写法、嵌套地址片段拒绝、宽字符与软换行坐标、迟到激活和扫描预算；TypeScript、三个改动文件的 Biome 和产品构建通过。未跑全量回归或原生／SSH 矩阵，未新增浏览器验收；TypeScript 语言服务不可用，未计为通过、未安装工具。现有前端 CI 自动覆盖新增用例。记录在 `target/probes/ip-links-development/`；真实 WebView、系统浏览器和跨平台验收仍待完成。

## 终端链接受限系统打开（开发验证）

已接通 Linux 显式确认后交给 GIO 默认浏览器的受限入口。普通文本与 OSC8 共用目标检查；前后端验证 HTTP／HTTPS、无凭据和4096字节上限。实际窗口与原页在命令接收／主线程交接前校验，单窗一个任务、回调结束前不释放容量，退出等待实际完成；不启用输入或导航 WebView。

3 项原生及63项前端受影响行为通过（失败项只定向续跑），另通过 TypeScript、局部 Biome、Rust 格式、桌面库严格 Clippy 和产品构建。真实 Chrome／xterm 的打开适配器受控，覆盖显式确认、重复抑制、失败重试、不可用时复制、IME、窗格隔离和退场，零导航及零输入；不构成系统浏览器启动、系统剪贴板、原生WebView、PTY／SSH或跨平台验收。未跑全量回归或认证矩阵。日志与截图见 `target/probes/link-open-development/`。

## 终端链接识别与目标检查（开发验证）

TERM-10／SEC-08 首批接通普通 HTTP／HTTPS 文本、跨软换行与 OSC 8 链接的目标检查和显式复制。24 项直接相关前端行为、TypeScript、局部 Biome 和产品构建通过。真实 Chrome／xterm 组件验证实际目标显示、可信点击复制、复制失败、凭据拒绝、窗格隔离、撤销、焦点与720px布局，零输入、零导航；剪贴板使用受控适配器，未访问目标或写系统剪贴板。沿用现有CI，未跑完整回归。TypeScript语言服务不可用未计通过。系统浏览器打开、系统剪贴板、原生WebView、真实PTY／SSH和跨平台仍待验收，详见[实现边界](../architecture/terminal-links.md)。证据在 `target/probes/terminal-links-development/`。

## 手动回到最新输出（开发验证）

回看当前窗格时显示手动回底按钮，回到底部后隐藏；与输入／输出自动回底设置独立。15 项直接相关前端行为、TypeScript、局部 Biome 和产品构建通过。真实 Chrome／xterm 检查默认、关闭和开启自动输出回底时的按钮行为、鼠标与键盘操作、独立窗格、焦点恢复、零输入及撤销清理，720px 无横向溢出。沿用既有 CI，未运行完整回归。证据在 `target/probes/scroll-return-development/`；原生 WebView、真实 PTY／SSH、性能和跨平台仍待验收。详见[实现边界](../architecture/terminal-output-scroll.md)。

## 终端可视响铃（开发验证）

当前挂载窗格接入 xterm BEL 固定文案提示，重复响铃合并、可清除；只有 Commit 后实时 Data 解析期间的有效信号可触发，恢复／修复回放、取消及旧视图不触发。显示焦点不授予输入权限，不新增输出订阅或 ACK。

三份直接相关前端文件共 25 项行为、TypeScript、局部 Biome 与产品构建通过；真实 Chrome／xterm 组件确认 2,000 次合并、窗格隔离、清除／焦点、撤销清理、720px 布局与零输入事件。已有 CSS 特异性警告保留，TypeScript 语言服务不可用不计通过。未跑 Rust、全量前端回归或 SSH／多窗口矩阵。必要记录见 `target/probes/terminal-bell-development/`；现有 CI 自动覆盖新增测试。范围与待验收见[实现说明](../architecture/terminal-bell.md)。

## 新输出时回到底部（开发验证）

新增默认关闭的 `[terminal].scroll_on_output`，与输入回底独立，支持保存明确值和恢复继承。v8 封闭投影保留 v1～v7 兼容，新挂载视图采用偏好，开启时在有效文本解析完成后调用 xterm 公共 scrollToBottom。空文本／UTF-8 片段不滚动，旧作用域、释放或迟到回调不触发滚动；输入权限和输出 ACK 流程保持原有边界。

4项后端（包含2项输入偏好兼容检查）、26项前端直接行为、TypeScript、格式、settings-schema、Store／桌面库严格Clippy和产品构建通过，Store诊断Exact零问题。真实Chrome组件使用实际Surface及xterm公开buffer坐标：默认／关闭时viewportY维持4，新输出使baseY从72增至73；开启时viewportY变为73。三种情况inputDisabled均为true且输入事件均为0。组件未使用PTY或IPC，不计为原生WebView验收。首次投影／夹具和浏览器加载失败保留，修正后复验独立记录。沿用既有CI，未运行完整回归或SSH／多窗口矩阵；原生实窗、真实PTY／SSH、性能和跨平台仍待。收据见 `target/probes/output-scroll-development/`。

## 输入时回到底部（开发验证）

新增默认开启的布尔设置，明确false可保存和恢复继承，投影v7保留v1～v6兼容；新挂载视图映射xterm公共scrollOnUserInput，Profile显示合并保留该值，不改变输入权限。2项新增后端、30项受影响前端行为、类型、格式、settings-schema、Store／桌面库严格Clippy和产品构建通过；Store诊断Exact零问题。首次编辑、夹具编译和格式保留失败记录，修复后独立复验通过，未回写原失败。记录见target/probes/input-scroll-development/，沿用现有CI、未运行完整回归；完整输出跟随控制、原生实窗、真实性能和跨平台仍待。

## 历史容量设置（开发验证）

新增“保留历史行数”：默认100000，可设1000～100000，恢复继承回到默认。GUI／CLI本地和SSH直接／跳板叶终端在同一原页设置读取中冻结配色与容量，新建生效，已有会话不缩容，16MiB冷区预算不变。4项新增后端、22项受影响前端行为、类型、格式、schema、Core／App／Store和桌面库严格Clippy、产品构建通过；Core诊断Exact零。检查覆盖配置继承与无损保存／恢复、冷热边界和EOF、原生文件同快照及旧页拒绝、v6封闭投影和输入范围。记录见target/probes/history-capacity-development/；沿用现有Rust／Vitest、原生编译和settings-schema CI，不新增矩阵、未跑完整回归；原生实窗、真实性能与跨平台待验。

## 查找入口与可配置快捷键（开发验证）

按钮、命令面板和默认Ctrl/Meta+Shift+F已共用聚焦窗格只读入口。find可改键、禁用、恢复默认，保留终端Ctrl+F；组合输入让路，原页／窗格／视图／布局变化拒绝旧选择，读取权限不变。20项后端、98项前端直接行为、8份投影／1158项Rust与TypeScript策略对照、类型、格式、相关库严格Clippy及产品构建通过。Chrome组件验证快捷键、命令选择、焦点、禁用和紧凑布局，采用模拟IPC，不代表原生WebView或真实PTY验收。开发记录见target/probes/find-entry-development/，沿用既有CI，未跑完整回归；实窗、性能和跨平台仍待。

## 整词匹配（开发验证）

历史与查找对话框新增Unicode整词模式，默认关闭，可与区分大小写、正则及主屏／备用屏范围组合；连续中文不自动分词。字面先转义、正则整体分组后施加Unicode边界，沿完整逻辑行定位，共用原有非回溯引擎和扫描预算。wholeWord请求／响应必须为布尔值并一致，切换重置游标、丢弃迟到结果；EOF冻结内容继续只读，原页／窗口权限不变。直接行为19项查找、2项Registry隔离、1项原生协议及24项前端通过，TypeScript、Rust／Biome格式、Core／App／桌面库严格Clippy和产品构建均通过。Chrome采用模拟IPC，仅计组件交互和布局。开发记录见`target/probes/whole-word-development/`。沿用既有CI，未跑完整回归或SSH／CLI矩阵，原生WebView、真实PTY／SSH、性能与跨平台仍待。

## 正则查找（开发验证）

历史与查找对话框新增正则模式，覆盖冷热历史、主屏、活跃备用屏和自然EOF冻结文本；按完整软换行逻辑行匹配，硬换行不拼接，每次定位一条逻辑行，零宽结果也前进。已锁定的非回溯引擎限制512字节查询、编译程序／缓存及嵌套；不支持回溯引用和环视。单行超出2048行／512KiB扫描预算明确拒绝，未完成行不在批次边界测试锚点。请求／响应严格回显regex，模式或范围切换从头查找；错误保留当前内容。直接检查结果：15项查找、2项Registry隔离、1项原生协议及21项前端通过，类型、格式、相关库严格Clippy和产品构建通过。语言服务超时未计通过。直接检查涵盖正则算法、EOF双来源、原页／跨窗隔离、原生封闭参数及两个前端模块；Chrome模拟IPC仅算组件呈现。日志见`target/probes/regex-search-development/`。沿用既有CI，不重复全量回归或SSH／CLI矩阵，不计原生WebView、真实PTY／SSH、性能或跨平台验收。

## 备用屏查找（开发验证）

历史对话框增加“历史与主屏／备用屏”显式范围，备用屏只读当时活跃画面，支持宽字、组合字符和双向软换行匹配。查询与响应包含alternate，严格拒绝范围错配、缺省或错类型字段；切换范围不复用游标，搜索片段不进入主屏历史分页，未启用备用屏时保留当前页并提示不可用。EOF可冻结当时活跃备用屏，与主屏热区共用8MiB附加文本预算；超限不部分移动来源，原页／owner／binding及跨窗拒绝保持不变。直接检查覆盖查找、EOF、预算、Registry与原生参数，另有19项前端、类型和产品构建及Chrome模拟IPC组件呈现，既有CI自动覆盖。不新增重型矩阵，不计vim／less、真实PTY／SSH、原生WebView、系统IME、性能或跨平台验收；正则进展见上文开发记录。日志见`target/probes/alternate-search-development/`。

## 保留历史查找（开发验证）

历史对话框字面查找已覆盖冷热保留历史和当前主屏，按真实软换行拼接，不跨硬换行；支持跨冷热／主屏和扫描批次的匹配行段。每批最多2048行／512KiB，原Actor修订与原页／owner／binding校验保持；搜索响应含独立screenRows和匹配起止行，搜索片段不得送入历史翻页。8项查找、3项分页、16项前端、类型、格式、产品构建与CLI＋SSH严格Clippy通过；Chrome模拟只读IPC验证跨屏多行高亮、双向查找、显式返回历史、过期保留、IME让路、720px布局及焦点恢复，仅作组件验证。备用屏／正则、原生WebView和性能仍待；自然结束读取由下述补充覆盖；未跑全量，现有CI自动发现，不增加重型矩阵。日志见`target/probes/primary-search-development/`，上一批保留历史入口记录仍在`target/probes/history-search-development/`。

自然Ended终态现保留有界只读文本快照，支持原页绑定内的冷热历史分页、主屏及软换行查找；冷区转移所有权，热区／主屏分配及行容器另限8MiB，预算失败明确拒绝，不静默裁剪。快照不持有Actor、PTY、SSH流或输入租约；Closed／Failed不发布。EOF使旧游标失效，刷新后重新绑定，跨窗及旧页拒绝；产品入口ready／ended可读，暂停／失败不可读。EOF、历史与Registry直接检查及17项前端、类型／格式与Core／App严格Clippy通过，日志见target/probes/ended-search-development/。只做相关检查，既有CI自动发现；真实PTY／SSH与原生WebView未在本轮验收，备用屏／正则仍待。

## SSH 新会话配色预设（开发验证）

经典／午夜／纸白已接GUI、CLI直接SSH三类身份与跳板叶；当前原页共享设置读取在网络启动前冻结基色，逐跳不重读，Actor接收初始缓冲前安装。组合原生编译、1项原生只读设置、4项前端、类型、格式及CLI＋SSH严格Clippy通过；单个既有真实密码SSH探针验证两次连接纸白／午夜首条OSC背景回复、捕获基色、导航恢复以及原输入／EOF／物理退出。只使用原有Linux CI入口，不跑完整回归。其他身份和route的配色实窗、产品GUI／CLI、跨平台与在线主题仍待；不借用既有认证验收作配色结论。见[配色实现](../architecture/terminal-color-presets.md)与`target/probes/ssh-terminal-presets-development/`。

## 新本地终端配色预设（开发验证）

经典黑底／午夜／纸白已接GUI与CLI新本地会话，默认与14px保持。局部预览只读，保存／恢复继承沿用CAS；后端读取冻结配色并在Actor首字节前安装，远端覆盖／重置和原捕获恢复保持。2存储、1Actor、1App、1原生文件及20直接前端行为、TS／产品构建、Chrome组件通过；格式／注册schema生成与漂移检查通过，未跑完整回归。新投影v5兼容v1–v4，固定RGB预览与后端同源；Rust Exact零诊断，TS Unknown不计。已有会话与SSH不换色，真实PTY／原生WebView／跨平台／在线应用与主题库仍待。见[实现边界](../architecture/terminal-color-presets.md)与`target/probes/terminal-presets-development/`。

## 界面外观（开发验证）

浅色／深色／跟随系统、局部草稿预览、当前页确认保存后立即应用及逐字段恢复继承已接通用户级设置；主机／组覆盖和整表替换拒绝。终端ANSI、字体默认、现有保存CAS和实际I/O监督不变。5项新存储、1项原生文件、22项相关前端、TS／格式／schema及产品构建通过；88项设置相关检查先前通过、1忽略未计入。Chrome使用真实CSS与模拟设置I/O确认保存／取消／系统切换和720px布局，不计原生WebView或PTY验收。未跑全量；Astrolabe超时／不可用不计。见[实现边界](../architecture/ui-appearance.md)，开发证据在`target/probes/ui-appearance-development/`。

## IME 应用层按键保护（开发验证）

组合生命周期与WebKit229／Process兼容已接通工作区快捷键、弹窗Escape、刷新、命令面板及Surface复制按键；候选键让路给浏览器／SDK，不补发组合结果。旧页／Surface提交仍受原输入授权。7个直接相关Vitest文件共79项通过（分批按文件去重）、TS／改动文件Biome及产品构建通过；保留既有warning，TS Astrolabe Unknown不计，不安装额外工具、不运行全量回归。现有CI自动覆盖新增模块。系统中日韩IME、真实候选框位置／DPI及WebView事件顺序仍待验。见[实现边界](../architecture/terminal-ime.md)和`target/probes/ime-input-development/`。

## 后台视图资源调度（开发验证）

隐藏分屏、document hidden与原生隐藏／最小化共同门控已接通；首窗／新窗各自观察器、精确WebView事件、当前原页双重复验、监听先于快照、独立序号及窗口退出等待均接通，Linux接GTK状态信号，无空闲轮询。暂停释放Surface、输入和尺寸观察，恢复仍走原会话捕获／ACK链和detach接收回执，不改物理容量释放。此前65项结果保留；本批25项前端、7项原生观察器、TS／Biome、CLI＋SSH原生库和Vite构建通过，未运行全量回归。Rust诊断Exact零、TS Unknown不计，保留既有lint和动态导入警告。原生WebView／WebGL实窗、跨平台、遮挡、停车池和资源指标仍待。见[实现边界](../architecture/terminal-background-views.md)，本批开发证据位于`target/probes/native-background-development/`。

## 十万行历史冷区（开发验证）

生产Actor已接通紧凑内存冷区和冷热统一分页，最多100000物理行、冷区16MiB分配预算，超限清理最早内容并提示；不落盘。7项冷区、3项既有分页和8项前端检查、TypeScript、App编译、pulse-term严格Clippy及改动文件格式通过。覆盖一次大批输出、跨冷热区、中文组合字符、容量、备用屏resize、显式清除、同步输出与真实Actor通道。沿用现有Rust／Vitest CI自动发现；本轮未跑全量回归、SSH／CLI矩阵、性能或原生实窗。证据见`target/probes/history-cold-development/`，行为与边界见[架构说明](../architecture/terminal-history-pages.md)。后台停车池、结束历史和资源指标仍待完成。

## 工作区与目录管理（Linux限定）

- [内置字体预设与后备及运算符连字](../architecture/terminal-font-preferences.md) 已完成Linux限定验收：官方原始JetBrains Mono 2.304、完整本地OFL、预设／许可零写、明确保存、字体就绪及页面撤销，缺失主字体的内置后备、固定ASCII／中文／emoji、同PTY刷新、第三进程配置零写重开及原生退出。原字体批23份证据见 `target/probes/terminal-font-acceptance.json`；该批clean-dev回收10.6GiB。连字使用按需WebGL、局部OpenType覆盖、有界GPU fence后ACK，默认DOM／14px／关闭不变；真实webgl2、整串／逐字142像素差、实际运算符合并与混合宽度输出通过，原文输入不变。1182项全前端及修复后41项相关检查通过，13份不可变SHA证据见 `target/probes/terminal-ligatures-acceptance.json`。当前二进制复用于CLI Agent验收，链退出后清理；不计GPU性能、跨平台、系统IME、全面CJK或发行审计通过。
- CLI Agent已完成Linux限定验收：原窗显示精确saved目标和Agent名称，锁定／取消／导航／断连零签名，明确批准后仅所选第二钥一次Ed25519签名、密码认证零；固定原文输入、刷新不重认证、独立重开与native排空通过。两项App来源反例和全目标CLI／SSH严格检查通过，22份SHA成功／失败证据见 `target/probes/cli-agent-acceptance.json`。CLI跳板亦完成下述限定验收，合并链结束后clean-dev回收22.7GiB。
- [CLI 跳板原手势交接](../architecture/cli-ssh-jump-route.md) Linux真实链已通过：同 SQL 快照三类型路线、最初页面／epoch、首跳取消零认证零写、三次实际指纹批准、两个所选 Agent 签名、叶输入 `GUI_SSH_ECHO:R`、刷新不重拨、Recent 独立重开及父 stream 与两个进程正常退出。Done 消费／typed owned-session 选择 ACK 保证 CLI 成功回执在原页 inputReady 之后。默认 check/test/probe 全部通过，1050项Rust与1187项前端、7项忽略不计。39份不可变SHA证据见 `target/probes/cli-route-acceptance.json`；构建／窗口／native全部退出后clean-dev回收22.7GiB，临时目录清空。一次手工续跑遗漏实际环境键的普通Recent失败未计为CLI route结果；本轮通过开始时间核验，旧故障明确标记。跨平台、硬件Agent、外部网络、系统IME和性能不计通过；发行审计仍阻断。
- [冷 SSH 保存意图](../architecture/cli-cold-ssh-intent.md) 的 Linux 密码真实窗口已通过：锁定取消与解锁后取消均零认证／零写，解锁不替代明确点击；最终精确目标批准后认证一次，生产刷新前后两个原文回显且不重拨，三独立 GUI／端点／peer 正常退出。两项 Vault、28 项前端、5 项银行和10项启动检查通过。默认 check/test/probe 通过：1050项Rust、1191项前端、7项忽略不计；审计退出5，许可证／维护策略仍阻断。26份SHA证据见 `target/probes/cold-ssh-acceptance.json`，进程全退出后clean-dev回收18.4GiB、本批临时目录清空。不计冷Agent、冷跳板或跨平台完成。

- 冷 Agent 及密码／Agent 三跳完成 Linux 本地软件 peer 限定验收：单入口只签预存第二把钥匙一次、零密码；三跳首跳取消零认证、逐跳精确目标／身份／本轮指纹批准，外password一次、中叶Agent各一次，SQL只新增两条匹配公钥的跳板pin。最终共用harness的密码／Agent／三跳三模式和默认check通过，刷新前后原文输入、不重拨与GUI／父连接／转发流退出正常；现有1050项Rust、1191项前端及基础probe参考已复核归档，7项忽略不计。24／33份SHA分别见 `target/probes/cold-agent-acceptance.json`、`target/probes/cold-agent-route-acceptance.json`；全退出后clean-dev回收10.6GiB且本批临时目录清空。Clippy all通过但unused告警保留，Astrolabe引用Unknown未计通过；不计硬件、外网、跨平台或审计通过。

- [冷 CLI 分屏](../architecture/cli-cold-split.md) 的 Linux 六真实冷模式已验：首步取消零PTY，次步取消／断连／换页保留已注册首个；两次独立批准绑定精确SessionId／pane，左右／上下双PTY刷新前后独立原文输入、原PID保持及GUI／native／端点退出通过。36项局部前端、10项启动边界、冷观察期限、严格原生和新模块Exact零诊断通过；默认check、1050项Rust、1195项前端及基础probe通过，7项忽略不计。初次并行回归9项native完成超时保留，14项单独复核通过，最终完整回归使用4个Rust测试线程且未改5秒断言；不宣称默认并发稳定性已解决。56份SHA证据见 `target/probes/cold-split-acceptance.json`；全退出后clean-dev回收14.5GiB、本批临时目录清空，target/probes保留。审计退出5仍有许可证／维护阻断，跨平台、IME、性能及多窗口未计通过。

- [保存主机别名](../architecture/cli-saved-host-alias.md) 的 Linux 功能矩阵完成：密码／Agent／三跳冷暖六场景和同名／缺默认身份两拒绝场景通过最终版本；暖态缺失别名、目标取消以及冷态锁定／解锁后取消零认证／零写。实际默认身份、逐跳目标／指纹与 SQL 两 pin、所选 Agent 签名、刷新前后原文与不重拨、本轮 GUI／peer／父连接／端点正常退出均核验。原 epoch 与来源 CAS、独立精确目标批准和闭合 Capability 注册反例通过；默认 check/test/probe 通过（1050 Rust、1197 前端，7 忽略不计），Rust 完整回归四线程且不改断言。305 份 SHA 副本含历史非零退出、锁定取消后 SIGSEGV 和四次 GDB 正常关闭；该间歇故障仍未解释，不计可靠性解决。见 `target/probes/saved-alias-acceptance.json`；全退出后 clean-dev 回收 20.1 GiB，本批临时目录清空。审计仍 8 许可／2 维护阻断，不计硬件、外网、多窗口、跨平台或发行通过。
- [原生窗口关闭调查](native-window-close-investigation.md) 本轮完成有界观测：先锁定／解锁各 12 次正常关闭，再各 32 次，累计 88 正常加 8 GDB（各 4）；96 个独立 GUI 退出 0、零认证／签名／写入及端点回收核验。实际 lib 的 3 项退出屏障通过，涵盖真实 PTY／数据库物理完成、并发单 supervisor 和未完成不能放行；bin 零测试调用另存未计。254 份最终 SHA 证据链接并复核先前 129 份批次，见 `target/probes/native-close-investigation.json`。全退出后 clean-dev 回收 9.9 GiB，本批临时目录清空。未重现历史 SIGSEGV、未捕获故障栈，生产关闭代码未改，不能计为可靠性修复；未重跑无依赖变更的发行审计，既有 8 许可／2 维护和跨平台阻断保留。
- [CLI 命令链](../architecture/cli-command-chain.md) 完成 Linux 限定验收：整链非法四反例零 GUI／端点／PTY，首步取消零 PTY，次步取消／断连／换页只保留首个会话，左右／上下成功双 PTY 各自原文输入、刷新原 PID 及 GUI／native／端点退出。每步独立批准、同认证连接和唯一相关 ID，不跟随替换实例；本地注册等待原页 v5 selection admit／布局输入完成／complete 后再回执。19 解析／单动作、2 真实 IPC、44 UI、5 原生 selection 银行、严格检查及 Client／本地呈现 Exact 零诊断通过；六冷分屏兼容实窗通过。默认 check/test/probe 核验 1050 Rust／1204 前端，7 忽略不计，四 Rust 线程且不改断言，前端 23 警告／4 infos 保留。111 份 SHA 证据见 `target/probes/cli-chain-acceptance.json`，历史失败图单列，不将空 type 日志或旧截图作为成功证据；真实窗口验证后仅等价格式及回调 guard 调整，最终合并覆盖。全退出后 clean-dev 回收 20.5 GiB、本批临时目录清空。审计退出 5，8 许可／2 维护及历史关闭 SIGSEGV 保留；不计多窗口、外部 SSH 链、跨平台、硬件或发行通过。
- [CLI 多窗口](../architecture/cli-multiwindow-routing.md) 完成 Linux 本地限定验收：原窗独立确认建空窗，三后端标签、精确路由、多窗无选择/外窗session/退役标签拒绝；取消/断连/导航零创建，新窗正常主机树且无外窗会话。双PTY独立原文输入，两窗刷新保留原PID；关闭主窗后另一窗继续输入并再建空窗，最后GUI/native/端点退出。应用共享服务与每窗Page/Vault/确认银行分开，目录等待Destroyed和实际I/O双完成。全部desktop CLI/SSH库287通过、5忽略未计，32项局部前端/6生命周期/8Capability/21CLI及严格通过。最终注册多窗task、七命令链、六冷分屏和默认check/test/probe通过：1063 Rust、1208前端，7忽略不计，四Rust线程且不改断言，23前端warning/4info保留，不声称默认并发稳定性已解决。SHA见 `target/probes/cli-multiwindow-acceptance.json`，历史失败与来源快照分列，进程退出核验后clean-dev回收25.2GiB、本批临时目录清空，CI/指南/计划已同步。SSH跨窗认证、IME、跨平台、硬件和性能不在本批范围；历史SIGSEGV未解决，审计仍8许可/2维护阻断。
- [CLI SSH 跨窗](../architecture/cli-multiwindow-routing.md) 完成本地软件fixture限定验收：密码、选定软件Agent及混合三跳通过；独立Vault/原窗提示、取消/断连/换页零认证零写、A锁定/关闭不撤B批准或终端、B刷新不重认证、父流退出。已修路线显示终端后漏刷本窗会话目录。注册三模式、原冷SSH三模式、本地多窗、七命令链及六冷分屏、默认check/test/probe核验1063Rust/1208前端，7忽略不计，Rust4线程且断言不变；不表示默认并发稳定性已解决。343份最终SHA见 `target/probes/cli-multiwindow-ssh-acceptance.json`，全退出后clean-dev回收12.4GiB、本批临时目录清空；task/CI/指南同步。保留原23警告/4infos、TS诊断Unknown及冷分屏X11观察器BadWindow stderr；后者有正常生命周期断言，不是历史SIGSEGV修复证据。系统/硬件Agent、远端网络、跨平台与8许可/2维护审计阻断仍未验收或解决，Alpha门保持开放。
- [原生默认并发调查](native-parallel-investigation.md)：App默认并发294项及workspace默认并发1063项通过，7忽略不计；40项现有测试有界执行1280次，注册入口另320次，未重现历史九项超时，保留原五秒断言与历史未解释状态。新增Linux诊断任务和CI；失败构建／测试退出码18与后代回收反例通过，最终SHA见 `target/probes/native-parallel-acceptance.json`，全退出后清理4.3GiB，任务临时目录清空。
- [依赖审计执行](dependency-audit.md)：修复Rust失败短路前端的漏执行；七项真实子进程反例通过。真实两项均完成，Rust仍5（8许可证／2维护），前端0且五级告警零，总结果5；依赖、锁文件与允许集未改。Linux CI保留两个审计的独立日志与退出码；远端CI、Windows/macOS及Alpha发布门不计完成，29份最终SHA见 `target/probes/dependency-audit-acceptance.json`；进程退出后clean-dev成功（0文件），本批临时目录清空。
- [第三方署名材料](third-party-attribution.md)：工作区778组件、438份去重原文、94条补充来源；新增libappindicator-sys同提交发布父包原文关联。完整文本缺项99→25→24→23→22（4未绑定＋18说明），原归档遗漏98单列。39项行为、实际注册生成、离线、独立发布归档原文、Mise/CI/Git属性检查及三脚本Exact零诊断通过；发布门禁实测退出1，releaseApproved=false。dirty规则和动态库发行边界不变；见[逐包审查](remaining-attribution-review.md)。本批 `target/probes/published-source-acceptance.json` 最终494份SHA已复核并包含clean-dev移除0文件收据；独占临时目录在最后核验后删除，历史证据保留；远端三平台CI、按二进制义务及8许可/2维护审计阻断不计完成。后续平台／stackback候选逐文件调查已核验不能补全来源，22缺项不变；39项注册离线检查通过、文本门禁仍退出1。新证据 `target/probes/platform-attribution-acceptance.json` 最终20份SHA已复核，含clean-dev移除0文件收据；独占临时目录于最后检查后删除，材料与生成规则未变。
- [候选发行范围审查](distribution-review.md)：12项行为、五目标冻结解析、独立材料/配置校验和两个脚本Exact零诊断通过。正常／额外构建依赖分列，159 npm和字体保守保留；当前host仅Linux，GNU只是对照，不计外平台构建、实际包内容或发行批准。工作区22项文本缺口门禁仍实测退出1；最终50份SHA已复核，含clean-dev移除0文件收据，见 `target/probes/distribution-review-acceptance.json`；独占临时目录在最后核验后删除。
- [行高与字距](../architecture/terminal-cell-metrics.md) 已完成 Linux 设置验收：3项指标存储、83项设置Rust（1项忽略不计）、1165项前端回归与组合全目标严格Clippy通过。字号18固定时行高150%使67×22变67×15，字距2px使其变56×15，原生ACK与内核网格对应；刷新同PTY、独立重开读取零写，分别恢复继承后56×22→67×22并保留字号覆盖；两进程原生排空与退出。20项不可变证据及SHA见 `target/probes/terminal-metrics-acceptance.json`。不计系统IME、其他平台、连字、后备字体或性能；当前字体验证链仍复用二进制，最终退出后清理。

- [原页面生产双跳路由](../architecture/ssh-production-route.md) 已完成 Linux GUI 限定验收：真实密码双跳、Ed25519 中间跳／密码叶，逐跳原页目标／身份／实际指纹确认、取消零认证、精确首次 pin 和其他 SQL 来源不变、终端 `GUI_SSH_ECHO:R`、刷新不重拨、独立重开及叶／父连接实际退出。原39份证据见 `target/probes/ssh-production-route-acceptance.json`。
- [明确选择的 Agent 身份](../architecture/ssh-agent-identities.md) 的 Linux GUI 直连及密码外层／Agent 中间／Agent 叶组合实窗通过。发现两钥后取消零写／零签名，保存第二钥和名称更新各推进一次修订，连接取消零签名，直连一次／路线两次所选Ed25519签名，密码认证无回退；固定行输入、最近连接、刷新不重认证、独立重开精确身份及叶／父资源退出已核对。每跳批准前保存本轮原生描述并与 SQL 地址／端口／指纹对应。三目录来源／CAS／依赖删除、原页／原epoch／活跃银行、恶意帧／拒签、原始路线回归和Agent中间角色拒终端／取消保槽／重解锁拒后续hop检查通过。68份功能证据已内容SHA复制并复核，见 `target/probes/ssh-agent-feature-evidence.json`；空类型检查日志保留，不把空截图或结果视为有效。合并基础验证通过（1046项默认Rust、1162项前端，7项忽略不计），基础探针仅证明headless一致性；最终绑定检查通过，77项Agent与39项路线不可变证据均校验通过；clean-dev回收24.3GiB，归档／清理临时目录清空。审计仍8项许可／2项维护阻断；SSH验收链已完整退出、归档并清理。不计CLI、跨平台、外部网络、硬件令牌、其他Agent算法或性能。下列来源／资源批为历史前置记录，当前组合状态以本条及最新计划为准。
- 生产跳板的 [传输资源层](../architecture/ssh-route-transport-ownership.md) 已完成后台限定检查：两项真实本地 TCP 握手验证有界拒绝返还、最终移交单次性、从内到外 SDK／底层流退出、取消等待不取消清理与 admission 保留；夹具零认证，无 channel／PTY／shell。`managed-store` 全目标严格 Clippy、受影响格式检查及 Rust Exact 无诊断通过；初次测试中的四处 expect lint 已移除后重验。四份证据 SHA 核验见 `target/probes/ssh-route-transports-acceptance.json`，退出后清理 1.5 GiB，任务临时根不存在。尚未接生产 driver、嵌套 direct-tcpip 或原生窗口，不计实际跳板连接验收。

- SSH 跳板逐跳后台批准前置见 [逐跳来源与原始点击](../architecture/ssh-route-hop-authorization.md)。三项 worker、真实双密文 Vault 精确类型／原 epoch 手势、首次 pin 回复推进、两项整条原始捕获取消／Drop／信用保留／同 DEK 重解锁和两项隐私 compile-fail 检查均通过。Store／App `ssh-use` 全目标 Clippy、源／捕获 Rust LSP Exact 无诊断通过；当前 SSH feature CI 已纳入新增模块。首次夹具枚举调用编译错误已修，取消反例初次 Busy 不算通过，修正后明确验证 guard 释放前不允许新动作。十份证据已 SHA 核验并归档到 `target/probes/ssh-route-hop-authorization-acceptance.json`，`clean-dev` 清理 3.9 GiB，任务临时根确认不存在。随后原 PageScope／原 epoch 读取 API 的两项真实 Vault／SQL 反例及 App `ssh-use` Clippy 已通过，日志为 `target/probes/ssh-route-original-read-tests.log` 和 `ssh-route-original-read-clippy.log`。桌面桥接的真实密封密码／私有数据库／跨页不创建目录／取消等待与换页信用保留单项已实际运行通过；最终桌面组合和 App `ssh-use` 全目标 Clippy、预检更新后的两项 App 读取反例及受影响 Rust 格式均通过。原生夹具旧 API 编译错误和一处嵌套 if lint 已修复，初次失败未计通过；九份证据已 SHA 核验归档到 `target/probes/ssh-route-original-read-acceptance.json`，任务退出后 `clean-dev` 清理 7.6 GiB，三个任务临时根确认不存在。两个新文件 LSP 请求超时，不算通过。没有开放生产路由 GUI，没有真实跳板网络／原生窗口验收，未重复全量回归或审计。

- 跳板配置前置见 [SSH Profile 跳板配置](../architecture/ssh-jump-preferences.md)：两项 core 图边界、一项共享 worker 原子生命周期与一项覆盖双身份直连手势拒绝通过；九项协议／表单、原生 wire、类型／Python、App ssh-use 及组合 Clippy 通过。core／原页解析器 Rust LSP Exact 无诊断。唯一 Linux 元数据实窗已通过精确引用原子保存、取消零写、刷新／独立重开及解除，机器／入口／身份／凭据／trust／组织不变，零认证与两次正常退出；32 份证据 SHA 核验，清理 11.9 GiB，见 `target/probes/ssh-jump-config-acceptance.json`。生产跳板与 Agent 尚未交付；不得将模型、零测试输出或既有技术探针计作生产连接验收。

- 临时快速连接的Linux密码路径已验，见[临时来源与使用守卫](../architecture/temporary-ssh-target.md)。三项SQLite临时来源反例、受影响密码／私钥来源、prepare后撤销Intent、原页账本替换／取消／换页／未注册保存拒绝，以及严格协议／保存表单／三项交接反例、类型／Python与组合Clippy通过。单条固定russh peer实窗核对取消零写、两次明确认证及终端就绪、暂不保存保留会话且不写主机／recent、Root实际注册后一次修订保存未保存目标与真实保存ID最近绑定、原记录与凭据／trust／组织不变、公开刷新、独立重开和两次正常退出。精确已存目标复用保持主机修订，不创建重复项；零测试首次命令不计通过，探针遮罩／复用夹具失败证据保留。证据归档见 `target/probes/temporary-ssh-acceptance.json`；未重复完整回归／审计，临时私钥真实认证、外部SSH互通、临时目标跨页恢复与三平台不计完成。

- 独立SSH配置名称／用户名及两类默认身份引用已通过Linux限定实窗，见[配置来源与权限边界](../architecture/ssh-profile-preferences.md)。同事务完整Host与两类Identity来源、同修订替换／墓碑／外来owner／最终撤权拒绝通过三项存储反例和受影响Host撤权检查；协议、旧表单、晚结果、默认有效／失效与种类歧义反例、类型和workspace-ssh Clippy通过。单条元数据实窗核对取消前保存零写、一次修订的独立名称和默认引用保存、完整机器／端点／旧入口值及凭据trust组织不变、原表单预选取消、公开刷新、独立重开和两次正常退场，零认证／recent。保留初次错误哈希范围的失败证据；Rust诊断超时／TS Unknown不计通过。28份证据已归档并核验SHA，`clean-dev`清理9.0GiB且任务临时目录为空，见 `target/probes/ssh-profile-acceptance.json`；不计多个Profile同入口、路由／连接后动作或三平台验证。

- 多SSH入口元数据已通过Linux限定验收，见[Host／Endpoint模型](../architecture/host-endpoint-model.md)。v2入口与机器投射读取同一SQL快照，侧栏组织投射复用该来源并核对完整图；旧库只读／取消不迁移，明确迁移、附加入口及机器重命名复用当前页单次保存票据。两个领域、四个图存储、六个受影响旧Host、15个Host协议／表单、四个组织协议与两个组织存储检查通过，类型检查与组织Rust诊断通过。Store与workspace-ssh＋workspace-cli组合Clippy已通过；主机适配器Rust诊断超时、TS LSP Unknown不计通过。单条真实元数据窗口已通过Linux限定验收：旧库读与取消零写、明确迁移、同机器双SSH入口、独立命名、删除墓碑与剩余机器保留、原入口ID预选取消、刷新及独立进程重开、两次正常退场、凭据／trust／组织／recent不变；按钮命名与select参数失败证据单独保留。33份证据已SHA核验归档，限定清理完成10.9GiB，任务临时根为空；独立SSH配置见上条，不计完整VAULT-02或其他协议交付。

- 本机最近保存主机列表已接入，见[成功注册记录](../architecture/recent-saved-hosts.md)。三个严格投射／晚结果测试、TypeScript与新组合Clippy已通过；两个SQLite/worker测试与真实SSH联合实窗已通过：取消零写、成功一次认证记一项、最近Saved Host预选取消、端点／身份／Vault／trust哈希不变、刷新／独立进程重开及两次正常退场均核对。九项受影响Vault表单测试通过，修复关闭表单前更新树disabled导致的不可达。结果见 `recent-host-result.json`／`recent-host-acceptance.json`；15份证据已SHA核验，`clean-dev`完成9.8GiB，已停止旧构建留下的精确临时根在确认无进程引用后删除，临时根为空。

快速目标预览与Saved Host链：新增地址输入支持SSH用户名／DNS／IPv4／方括号IPv6与端口，拒密码userinfo／路径／其他协议／隐藏控制字符；预览后只复用唯一目标或使用原Host CAS保存。8项针对解析／取消／歧义／预览变更／重复点击／旧页晚保存的测试、TypeScript与局部UI检查通过。Linux窄实窗通过invalid／取消零写／唯一复用零写／一次IPv6保存／原连接表单预选取消／刷新／独立重开／两次正常退场，零认证且credential与trust源不变；8项关键反例与输入就绪修正后的5项表单测试通过。模糊搜索的三项排序反例和短实窗非连续／多词／无结果／原ID预选取消零写也通过，未重跑认证矩阵或完整Rust回归；不计完整VAULT-05。证据见 `target/probes/quick-fuzzy-acceptance.json`。Host/Endpoint/SSH Profile两项纯图反例和core Clippy通过；迁移接入同SQL worker、旧Host单事务保存与完整源CAS，迁移及直接影响的Host检查仍待结果，不计完整VAULT-02。Rust LSP超时、TypeScript LSP Unknown不计通过；未安装额外工具或扩大回归。

- 嵌套布局与CLI分屏实窗已通过，见 `workspace-nested-result.json`、`workspace-nested-cli-acceptance.json`；最大化刷新隐藏叶的零尺寸捕获反例已修复，保留原失败日志。
- 三目录搜索／多选／批量删除联合实窗已通过；一次修订、取消零写、墓碑、密文不变、刷新与独立进程重开、零终端启动、两个应用正常退出均由 `catalogue-batch-result.json` 收据核对。3项SQLite、40项受影响编辑器、3项共享确认及wire反例通过，Store／组合Clippy通过。一次非零应用退场未定位且复验未复现，保留 `catalogue-batch-exit-code*` 以前失败日志，不能算退出根因修复。
- 组织分组／标签／收藏及主机树已通过 Linux 限定验收：独立持久文档、UUID／时间／HLC／墓碑、最大八层／防环／三目录原修订CAS、原页一次票据已接通。2项SQLite和相关Store／组合Clippy通过；主机树／列表／搜索收藏和Saved Host预选已接通，16项相关UI／TypeScript／Pyright通过，未知／已删目标不选替代主机，组织提交独立核对当前控件目标。组织＋树联合实窗通过：七次修订、三类归组、两行标签、收藏／树／列表筛选、分组删除墓碑、刷新／独立重开、端点与 Vault 逻辑哈希不变、零终端启动、两次正常退场与 WebView 子进程回收均已核对。非默认主机预选的只读 AT-SPI Description 与实际选择同步／关闭清空已验证；原菜单高亮和 Ctrl+R、末端滚动失败证据保留。归档见 `catalogue-organization-acceptance.json`；`clean-dev` 已清理18.3GiB，17份归档SHA核验完整保留，任务临时根为空。新原生AT-SPI选项收据发现选择不自动发DOM change，探针已用真实键盘提交选择事件，产品同时拒绝旧目标表单。见 [组织模型](../architecture/catalogue-organization.md)。Rust LSP Exact，TS LSP Unknown；未重跑完整回归或在线审计。



## 自动 Profile 参数／cwd／环境／外观编辑（Linux限定验收通过）

- 原页60秒单次编辑票据、联合实例快照／CAS与提交gate复用私库 worker；保存和取消不启动终端。参数保留顺序、空值和字面内容，cwd绝对路径，环境覆盖／移除拒绝保留名称；外观逐字段继承。启动仍只接后端发现身份，原 PTY／页面／根授权不变。见 [Profile 编辑器边界](../architecture/profile-editor.md)。
- 定向存储5项（含损坏／未来记录与旧默认联合来源）、编辑原生3项、Linux目录层2项、原未知native失败保留8项及43项renderer／编辑前端反例已通过，Pyright零错误。完整合并回归通过，忽略项不计通过。零测试过滤命令不计通过。Rust目录固定与启动监督器当前 Astrolabe Exact；首次诊断超时不计通过。TypeScript LSP 初始化仍返回Unknown（无法定位TypeScript安装），未安装语言服务；TypeScript编译及实际前端测试分别保留，不能代替LSP通过。
- 公开真实窗口链已通过七阶段：取消／导航零写、保留环境名拒绝、保存零启动、参数／空参数／cwd／环境覆盖及移除87字节原生收据一致，字号13→18实际内核网格92×31→67×22，原PTY和新会话刷新外观固定，再改Profile不影响已有会话，独立进程重开和正常退出，其他DB逻辑内容不变。结果 `workspace-profile-editor-result.json`，表单／终端／重开截图已审阅；私用户13仅为探针基线，产品14不变。
- 原嵌套滚动使保存按钮离开可见区，首次日志／截图 `profile-editor-first-scroll.log`、`workspace-profile-editor-first-scroll-failure.*` 保留；改为单一滚动并通过实际滚轮可达。Shell夹具 WINCH 打断read而提前EOF，`workspace-profile-editor-winch-*`保留，夹具改为继续等待后通过，不放宽产品授权或期限。公共任务／Linux CI 已登记。旧默认／墓碑／隐藏、v1／v2与SSH私钥公开实窗已通过。设置兼容探针仍按旧三行Tab顺序选错五行表单，首次日志与默认截图保留；已改精确AT-SPI字号焦点、SIGWINCH继续读取和/data/temp夹具，公开复验通过，当前14→18字号对应真实网格92×29→67×22，原PTY保活且正常退出；证据 `profile-editor-settings-fixed.log`。最终Profile公开实窗亦重新通过（`profile-editor-final-real-window.log`）。
- 完整test（1084前端）、probe、SSH Store（391单元＋2集成＋32doc）、CLI+SSH Native（238）、strict和bindings已通过，ignored不计，证据 `profile-editor-final-{test,probe,store,native,strict,bindings}.log`。最终check因两处forEach表达式回调返回值失败（`profile-editor-final-check.log`），已改void块；完整check与20项受影响表单补验通过（`profile-editor-check-fixed.log`／`profile-editor-callback-fixed.log`）；check仍有2项非阻断CSS specificity警告与4项原测试建议。Linux限定批次通过，归档 `profile-editor-acceptance.json`；已确认构建／probe／原生窗口全部退出、/data/temp任务夹具为空，退出后限定 `mise run clean-dev` 已成功清理36083个文件／28.6GiB，18项归档日志／结果完整且自有临时夹具为空，日志 `profile-editor-clean-dev.log`。缺失cwd新增反例发现portable-pty HOME回退；已加入Linux打开目录身份固定及缺失／非目录拒绝，native／路径替换反例已通过，见 `profile-editor-last-native.log` 首次失败与 `profile-editor-{cwd-fixed,native-final-fixed,unknown-native-retained,renderer-final-fixed,pyright-fixed}.log`。仅分配前的Typed cwd拒绝传递零native证明；未知native错误仍保持失败信用与退出屏障。两次公开audit因RustSec GitHub TLS握手失败未完成，不计审计通过；缓存 `cargo deny --offline --all-features --locked check` 再现8许可拒绝／2维护公告，bans和sources通过；advisory cache提交 `57dcc0f2192e025138907a495c1ac9a7e1e3e123`（提交时间2026-10-12T20:06:48+02:00），见 `profile-editor-audit-cached.log`／`profile-editor-audit-cache-source.txt`。不能计为最新在线审计通过，发行阻断不清除。跨平台、系统IME、主题继承完整GUI和发行审计阻断保持原状态。

## 自动 Profile 删除与显式恢复（Linux整批通过）

- 固定目标／操作原页60秒一次性票据，提交只接票据；删除保留默认身份并写墓碑，恢复只去除墓碑，不启动或关闭已有终端。墓碑变化同事务递增默认 revision，固定删除→恢复旧默认快照反例先红后绿。
- 11 默认／生命周期存储、10 旧编辑兼容、4组原页反例、默认3组兼容、386全功能存储／2集成／32文档、workspace strict 和 bindings 通过，忽略项不计；见 `profile-lifecycle-{red,store,native}.log`。
- 51 相关前端测试、tsc 与 Pyright通过，覆盖关闭、换页、迟到票据、重复提交、默认编辑隔离与取消后串行重读。TypeScript LSP Unknown 不计通过；Rust 当前诊断 Exact。
- 公开 `mise run probe-workspace-profile-lifecycle` 真实双进程通过七阶段，结果 `workspace-profile-lifecycle-result.json`：取消零写、删除默认保留活动 PTY／输入／刷新、墓碑跨进程保留、取消／换页不恢复、明确恢复零启动、其他表逻辑内容不变及两次退出回收。默认 revision 为1→2→3。删除确认、删除完成、恢复完成三帧已审阅。
- 首轮取消后同时重读默认／墓碑竞争单槽失败，零写，证据 `profile-lifecycle-first-cancel-read-failure.log`／截图保留；修复串行读取，不加槽、不延长期限、不自动重试提交。
- 原默认／隐藏公开实窗、完整 check/test/probe、1065 前端、386 SSH功能存储（6忽略不计）／2集成（1忽略不计）／32文档、235组合原生（4忽略不计）、strict 与 bindings 全部通过，见 `profile-lifecycle-merged.log`。`profile-lifecycle-audit.log` exit 5 仅8项许可／2项维护阻断，发布门禁保持关闭。完整验收清单 `profile-lifecycle-acceptance.json` 归档本轮结果与失败证据，原生／构建退出后清理日志 `profile-lifecycle-clean-dev.log`，保留 probes。证据限 Linux，不等同于跨平台验收。

## 默认自动 Profile（Linux 整批通过）

- 独立默认偏好、保留清空 revision、联合可见性快照和实例绑定 CAS；原页 60 秒一次性读/存/取消，读取与写入保留原生信用直到真正退出。隐藏／删除／未发现不回退其他 Shell。
- 5 组存储、3 组原页 Native、2 项应用层、workspace strict、26 项前端／类型检查与 Rust Exact 通过。日志：`default-profile-native-behavior.log`、`default-profile-window.log`。
- 公开 `probe-workspace-profile-default` 真 WebView 通过保存／清空零启动、刷新／双进程重开、隐藏禁止启动、明确一次启动、输入／同PTY刷新、退出回收；版本 1→2→3，除默认和显示偏好外全部数据库摘要不变。结果 `workspace-profile-default-result.json`；saved／unavailable／terminal 三张画面已审阅。保存与启动是两个独立动作。
- 原Profile探针用具名checkbox替代旧Tab序号，临时根迁到 `/data/temp`，不重定义 HOME。公开原隐藏/恢复兼容、完整check/test/probe、1036前端、380项SSH存储/2集成/32doc、231组合Native、strict和bindings全部通过（6存储/1集成/4Native忽略不计）。`default-profile-merged.log` 完成；exit5仅来自 `default-profile-audit.log` 的8许可/2维护阻断，发布门禁保持关闭。`default-profile-acceptance.json` 归档，进程和临时根均已退出，开发产物清理记于 `default-profile-clean-dev.log`，保留probes。
- 架构见 [默认 Profile](../architecture/default-profile.md)。当前证据不覆盖 macOS／Windows，发布审计门禁保持原状。

## 选中复制偏好（Linux 整批通过）

- `terminal.copy_on_select` 默认关闭，四层来源与稀疏布尔覆盖沿用原 settings 快照／授权／CAS。Settings 明确保存与继承复位已接通；78 项存储设置测试与 strict 通过（1 忽略不计），54 项前端、tsc／lint 通过，schema 已生成。
- 仅当前 Surface 的可信主按钮手势可触发原 `text/plain` 复制路径，原 4 MiB 上限和 OSC 52 拒绝不变；脚本事件、右键、隐藏页面、旧 scope／取消／释放拒绝复制。新增失焦、可见性变化和终端焦点转移退休反例通过。
- 公开 Linux `probe-workspace-selection-copy` 已验证默认不复制、原页 Settings 保存、同 PTY 刷新后真实鼠标自动复制、关闭后恢复不复制、无额外输入和实际退出；原单行／多行粘贴审批与换页撤销兼容通过，默认 Native strict 通过。终端画面以及开启／关闭设置同帧截图已审阅，字段、来源、勾选和保存成功状态完整可见；旧预绘制图保留，新像素门仅用于固定 Linux 夹具。完整 check/test/probe、972 前端、SSH 消费方 strict 与 bindings 通过；audit exit 5 仅来自 8 许可／2 维护阻断。首次误用关闭标签的观察器失败保留为 `selection-copy-first-dialog-label-failure.log`，修实际“完成”标签后通过；未放宽授权或期限。证据见 `target/probes/selection-copy-{store,ui,real-window}.log` 和 `workspace-selection-copy-*.png`。

## 右键粘贴（Linux 整批验收通过）

- 默认关闭的 `terminal.right_click_paste` 已贯穿四层来源、稀疏 Set／Remove、Settings 单字段保存／继承复位和 Surface 手势门。80 设置测试（1 忽略不计）、schema 与默认／SSH strict 已通过。
- GTK 完整定向实窗矩阵通过：单行、多行取消／批准、原页刷新退休、鼠标报告模式零读取、关闭模式后恢复读取、实际原生退出。两次真实延迟 X11 回复验证旧页零提交和关窗等待实际回调；未释放回复前应用未退出，释放后固定拒绝旧文本。5 组生产 begin/current Native 反例（含真实五秒到期仍保留额度）、102 前端反例与 tsc 通过，Rust 诊断 Exact。未采用 WebKit 许可／诊断已清理。
- 行清除触发的 `terminal.dom_projection` 实际故障定位为尾部 ASCII 空格处理不一致，已修两端一致的可见文本比较；不放宽原 ACK／绘制期限，不重放任何源字节。失败图片、观察器误读与类型错误日志保留，固定 Linux 状态行 OCR 在实际旧帧上验证后改正。
- 五行 Settings 的真实勾选／用户来源／成功状态同帧与终端恢复画面已审阅。原粘贴／选中复制、v1/v2 真实窗口兼容通过；最终默认／SSH strict、228 Native（4 忽略不计）、375 全功能 Store（6 忽略不计）、2 集成（1 忽略不计）、32 文档测试、完整 check/test/probe、1018 前端与 bindings 均通过。审计 exit 5 仅来自原有 8 许可／2 维护阻断，bans／sources 通过，不计发布通过。160 秒 Native 首轮超时的 5 组通过结果保留，未完成编译已续验通过；确认无活跃进程后精确清理遗留自有 scratch。架构见 `docs/architecture/terminal-clipboard-authorization.md`；本批记录 `target/probes/right-click-paste-acceptance.json`，合并日志 `right-click-paste-{native-baseline,compat,full-regression,full-audit}.log`，首次失败证据保留。

## 深历史精确复制（公开实窗运行中，未计验收）

新增 `mise run probe-workspace-history-copy` 与 Linux CI。后端发现的私有 POSIX sh 包装器启动固定 Python 生产者，原生网格确认 92×31 后只接收一次 `EMIT`。输出 600 条含中文／emoji／组合字符的长逻辑行，产生超过千行的软换行历史，位于现有保留／恢复预算内；不增加产品历史容量或隐藏命令。物理 ShiftPageUp／Down 和鼠标选区跨越历史，以真实 CtrlC→GTK 系统剪贴板回读对比完整源 UTF-8 字节；刷新后要求新页确认、同 PID／唯一会话、再次字节一致。每次复制前重设并实际回读哨兵，防止旧剪贴板残留冒充本次写入；只读回查可重试，复制手势与生产者输出不重发。复制／滚动不得产生 PTY 输入，关闭必须确认生产者退出及无 terminal failure。

深历史 Linux 实窗与完整恢复消费者基线已整批通过。600 长逻辑行，live 与原进程新页刷新后的系统剪贴板均为 80,425 UTF-8 字节，逐字节等于固定源；同 SHA-256 `118a0140695caaf09a36dfdc11e5fa4c0d37c4995410734726ad689a06d0ebd3`，每次重设并回读哨兵，零复制 PTY 输入、唯一会话、原生退出均确认。live／refresh 完成态画面已审阅。首轮 15 LF 和中间 1 LF 失败分别来自屏幕逐行 CUP 打断自然 autowrap，以及 xterm EL2 清掉历史到屏幕首行的 isWrapped；现以连续满行打印恢复关系，只在源 history 尾有 WRAPLINE 时对首行用 ECH 保留接缝，其它行仍逐行 EL2，避免制造历史。活动／隐藏屏软换行及不可精确空续行／底行边界反例通过，121 终端测试、strict、Pyright 和当前 Rust Exact 诊断通过。定向证据 `history-copy-seam-checks.log`、`workspace-history-copy-{result.json,live.txt,refresh.txt,live.png,refresh.png,native.log}`；首轮日志／图和中间失败字节留存。`history-copy-merged-regression.log` 在 test 阶段返回 101：随机前缀 `1049h + CUP 6;22 + a + 24 digits + CSI 2;5r` 暴露备用屏补绘继承全局局部滚动区，自然 wrap 搬移字符；不归因审计。固定命名反例先红后绿，grid 补绘前临时重置全屏 margins，绘制后仍分别恢复原 hidden/current margins。最终 126 validation 终端测试、30,000 恢复属性样本、strict／Rust Exact 与最新实际 80,425 字节深复制通过，记录 `soft-wrap-margins-{red,checks}.log`。`history-copy-margin-merged-regression.log` 已通过修改后的 check/test/probe、1018 前端、305 SSH app、228 组合 Native（4 ignored 不计通过）、strict／bindings。随后 Bash 模式重新开启的整屏 Tesseract 在 3 秒子进程期限超时，exit 1；失败画面实际有完整 `BRACKET MODE on READY`，应用正常退出，证据另存 `history-copy-bash-ocr-timeout.png`。观察器现缩到固定终端底部 300×150 区域，在既有失败和最终帧验证完整模式标记及收据 13；3 秒子进程／8 秒观察期限保持不变。补当前应用前置检查及单次 records 读取避免可空重复访问。`history-copy-consumer-final.log` 已通过当前 Pyright、Bash／Vim／右键、密码 SSH 五路／私钥 SSH 四路及 v1／v2 实窗。基础通过阶段未重复，整批 exit 5 仅来自 `history-copy-consumer-audit.log` 的 8 项许可／2 项维护阻断，发布门禁保持关闭。验收清单 `history-copy-acceptance.json` 归档全部阶段及失败证据；原生窗口／构建退出后按 `history-copy-clean-dev.log` 清理，保留 probes。固定 13px Linux 夹具等待门仍为 8 秒／窗口 80 秒，不计 4 MiB 极限、超过保留上限的历史、系统 IME、字体回退或三平台验收。

## Vim（Linux 真实应用已验收）

新增 `mise run probe-workspace-vim` 和 Linux CI，使用已安装 Vim、私有 vimrc／空文档、真实生产 Profile→PTY→TermView、GTK 剪贴板和物理键盘。关闭用户配置／插件／modeline／viminfo／swap／持久 undo／backup 读写；`/proc` 检查实际 Vim 可执行文件及私有 rc。六项行为通过：多行取消零修改、批准保原缩进和中文／emoji／组合字节、同 PID 插入缓冲刷新及新页确认、一次撤销恢复空缓冲、独立文件精确保存 31 个 UTF-8 字节、未保存尾行关窗不改已存文件且 Vim／Native 已退出。固定 F6 只观察应用自身状态，无隐藏产品入口；截图等待应用收据计数实际绘出，刷新／已保存状态同帧已审阅。Python AST、Shell、最终公开任务、注册及 diff 检查通过，已安装 Pyright 对当前源码报告 0 错误／0 警告；Astrolabe 返回旧行号及旧符号，按过期诊断保留、不计通过。

证据：`vim-real-application-final-public.log`、`vim-current-pyright.log`、`workspace-vim-{result.json,refresh.png,saved.png}`、`vim-application-acceptance.json`。保留首轮误用不存在 Profile 环境名、CtrlR 手势及全终端 OCR 漏读反色状态行的失败日志和画面；修正私有 PATH/SHELL、既有 CtrlShiftR 及行级完整收据门后通过，8 秒／80 秒期限未放宽。产品代码未改，沿用前批已通过 1018 前端／228 Native／check-test-probe／strict／bindings／v1-v2 基线，不重复未变检查；前批审计 8 许可／2 维护仍阻断发布。未覆盖系统 IME、字体回退、其他应用／深历史复制或三平台。

## Bracketed paste（Linux 整批通过，发布审计仍阻断）

独立公开 `mise run probe-workspace-bracketed-paste` 定向通过：现有 Bash／GNU Readline、私有 rcfile／inputrc／Profile 和实际 GTK 剪贴板，真实 Ctrl-X／Ctrl-B 从应用读取 READLINE_LINE，固定 13 条应用收据逐字节核对。取消零执行、批准后等待 Enter、待决输入同 PID 刷新、恢复后中文／组合字符、1012 字节成功／1013 拒绝、on→off→on 及真实原生回收通过。关闭模式按应用既有规则处理已明确批准的行；模式文件回执不能代替真实有序输出绘制，首轮观察时序失败保留。最后第 13 条收据与中性状态同帧已审阅；下一次正常粘贴仅清原 scope 自己仍显示的旧拒绝提示，不声称排队等于应用执行、不覆盖后端失败。53 受影响 Surface／PasteGate／Connection 测试及 tsc／biome 通过，全前端 1018 项和原键盘实窗通过。右键末尾多一空行定位到锁定 portable-pty 在 Unix writer Drop 注入换行+VEOF；真实 Readline 关窗反例随后证实未 Enter 的批准内容会执行。修复使写线程返回 writer，由 JoinHandle 保留到控制线程已确认子进程退出后释放，非 Unix 保持原顺序。40 local 测试、strict 与 Rust Exact 通过；实际关窗后严格保持原 13 执行收据，右键严格七行、迟到拒绝和真实退出屏障通过。证据 `bracketed-paste-close-before-fix.log`、`pty-close-order-targeted.log`、`pty-close-order-window-rest.log`。完整 check/test/probe、1018 前端、228 组合 Native（4 忽略不计）、默认存储 348（6 忽略不计）、SSH strict／bindings 及真实 Readline／右键／原键盘粘贴通过。错误 v2 任务名仅阻止末尾任务启动，失败记录保留；正确公开 v2 与原窗口回归补验通过，实际资源和任务临时目录均已退出。整批验收通过；audit exit 5 仅来自既有 8 许可／2 维护阻断，不计发布。合并证据 `pty-close-order-{full-regression,window-regression-rest,full-audit}.log`。证据 `target/probes/workspace-bracketed-paste-{result.json,final.png}`、`bracketed-paste-final-window.log`；Linux CI 已注册，其他交互应用、跨平台及系统 IME 尚待。

## ssh_config 可见预览源状态（Linux 完整验收通过）

- 原七组配置及四个新原生反例共十一组通过：借阅不消费、原期限不更新、消费／替换／导航后拒绝迟到结果，源变化保留单次确认规则；活动确认取消仍保留信用，退出等待实际资源退出。
- 默认／SSH 原生 strict、89 项配置前端反例与 tsc 通过。Linux 双进程矩阵通过并审阅画面：自动源变化保留旧展示、禁旧来源准备；重新预览更新，反复检查不消费显式确认，刷新／重开、全库不变、零终端启动和两次正常退出。源状态、原预览和保存导入三条公开兼容任务已通过；完整 check/test/probe、951 项全前端、212 项其余 Native（4 忽略不计）、组合 SSH strict 和 bindings 均通过。最终审计退出 5，仍为 8 许可／2 维护阻断。
- 公开 `probe-workspace-ssh-config-status` 已注册 Linux CI。固定状态收据不记录别名、配置或凭据。证据位于 `target/probes/ssh-config-source-status-*.log` 与独立 `workspace-ssh-config-status-*` 截图；合并日志为 `ssh-config-source-status-full-{regression,audit}.log`；本批已归档。

## ssh_config 独立保存导入（Linux 完整验收通过）

- 八组原生反例通过：真实 endpoint 重开、源变拒绝后新准备成功、SQL 写入后最后源检查触发回滚、外部数据库编辑冲突、错实例／取消／过期／复用、导航、未轮询 future 退出、活动 SQLite 的信用和槽保留到退出。
- 默认／SSH strict 和 211 项其余 Native 通过（4 忽略不计验收）；68 项预览／导入协议和界面隔离测试、tsc 通过。Rust Exact 无诊断，TypeScript LSP Unknown 不计通过。
- Linux 两步保存实窗通过：取消和源冲突零写入，只新增主机元数据，同 ID 刷新／进程重开，其他九表逻辑摘要不变；零终端启动，两次应用正常退出。主机地址、端口和用户名由独立进程直接检查持久化记录；旧截图不作为新验收证据。
- 首轮 post-refresh OCR 漏入口与无障碍动作焦点反例保留。观察器改为指定 PID 的可见、启用按钮动作，Host 输入明确聚焦后打字；配置对话框减少局部冗余间距，使 640px 高度主要操作完全可见。原 8 秒和 80 秒门不变。
- 证据：`ssh-config-import-native.log`、`ssh-config-import-strict-rest.log`、`ssh-config-import-ui-behavior.log`、`ssh-config-import-input-focus.log`、`workspace-ssh-config-import-result.json` 及完成／冲突／保存主机截图，全部在 `target/probes/`。完整 check/test/probe、930 项前端、bindings、新保存与只读预览公开任务、主机编辑兼容已通过；审计仍为 8 许可／2 维护阻断；文件系统与 SQLite 不具有共同事务，最后源确认之后的外部修改仍是限制。
## ssh_config 自动发现与前端预览（Linux 验收通过）

- 九组真实发现反例含实际文件 OpenSSH 差分、七原页反例与204其余Native通过；默认／SSH strict、bindings通过，4忽略不计验收。审计仍8许可／2维护阻断。
- 预览独立对话框只读别名、显示有界元数据、一次性核对源；40项协议／页面隔离反例与tsc通过。TypeScript LSP Unknown不计通过。
- 独立 0700 目录与两个应用进程的真实窗口矩阵已通过：Include 新成员使旧确认冲突，重新预览、页面刷新和进程重开读取到更新的用户名／端口，零终端启动且两次正常退出。SQLite 以只读方式核对全部 10 张表的行数与逻辑摘要，关键表必须存在，`app_state` 中的主机目录也覆盖；不输出行内容。
- 初次 Profile 回执超时未定位且证据保留；全页 OCR 漏识别入口已以失败图像校准按钮局部识别。原生完成回执早于 UI 状态更新，原截屏证据未计通过；改用既有 AT-SPI 名称／文本接口在目标 PID 的可见树内等待精确完成／冲突文案后截屏，节点／深度／调用时间有界，原 8 秒／80 秒截止不变。AT-SPI 编译在自有临时目录、退出后删除，不引入应用依赖。
- 公开预览任务、完整 check/test/probe、902 项前端、组合 SSH Native strict 与 bindings 已通过。旧主机兼容的首轮 OCR 把第二侧栏标签并为 `SSHSH`，修为有界侧栏局部放大与前缀识别后，真实 CRUD／刷新／三进程重开与正常退出通过；未改产品授权或截止。夹具移至 `/data/temp`，保持 HOME，使用私有 XDG；任务结束后目录不存在。
- 证据：`ssh-config-preview-final-regression.log`、`ssh-config-preview-hosts-rest.log`、`workspace-ssh-config-result.json`、可见完成／冲突画面与原生日志，均在 `target/probes/`。`ssh-config-preview-final-audit.log` 最终为 8 个许可拒绝和 2 个维护拒绝（46 duplicate 警告单独记录），exit 5 来自审计，发布仍阻断。

## 原页面 ssh_config 只读预览与源确认（原生阶段通过）

- 生产 workspace 已注册预览／一次性确认，逻辑别名之外的目录与 Include 集合不接受页面输入。只投射主机、端口、用户名、身份文件提示数量；不输出原文本或路径，不读取私钥，不写数据库或授权连接。
- 六组真实原生行为反例通过：元数据精确与外部改动拒绝、跨读取器／导航拒绝、重复／过期票据、排队撤销、实际读取后的迟到结果、取消期间额度与 shutdown 屏障。默认与 CLI+SSH 两组 strict 通过；原生 Astrolabe 超时，不计诊断通过。
- `probe-ssh-config-page`、六新增与其余204组合原生测试、check 和 bindings 已通过，4项忽略不计验收。默认与SSH strict通过；最终exit 5只来自8项许可/2项维护审计阻断。首次私有模块路径编译失败证据保留。自有临时目录已清理，全部验证已退出，已执行 clean-dev 清理13.0 GiB，保留 target/probes。
- 自动安全发现、前端预览、保存前全源 CAS、导入界面和实窗仍待验收；Unix 通过不当跨平台通过。
- 自动活动Include发现库阶段通过八新文件反例、37配置／62 SSH／4文档及strict、Exact。原页面接入和新增真实文件OpenSSH差分正在运行；本批仍待验收，不能计产品导入完成。

## ssh_config 文本与固定主文件基础（Linux 定向验证通过）

- `probe-ssh-config` 已注册 Linux CI。14 组文本行为反例、7 样本真实 OpenSSH `ssh -G` 差分和 5 组 Unix 文件反例通过；SSH 全特性 45 项单元测试、4 项文档测试和 strict 通过。Astrolabe 新解析／词法／文件模块为 Exact；最初引用查询带 stale 警示，仅按当前源码核对。
- 差分纠正 Host 大小写、Include 外层上下文和单双引号／词中 `#`。固定主文件不自行读取 Include 或密钥，缺源不当空源，拒绝父链／叶链接、硬链接、不安全权限、FIFO 和超长源。错误不回显输入、路径或命令，临时源只在 `/data/temp`。
- 初次 strict 条件写法与新文件夹具未设 `0700` 的失败证据保留；生产规则没有放宽。证据为 `ssh-config-text-foundation.log`、`ssh-config-openssh-oracle.json` 与 `ssh-config-file-foundation.log`。完整 check/test/probe、862 项前端、组合原生严格检查及 bindings 通过，audit 仍 8 许可／2 维护阻断；自动发现、原页面发布／撤销与产品导入尚未完成，详见 [架构](../architecture/ssh-config-import.md)。
- 显式 Include 集合与读取器实例绑定的源确认通过九组真实反例（跨实例、源修改、同字节替换、删除、新成员、目录替换、路径逃逸／链接／权限／预算）。目录时间戳漏检新成员的反例已实修为描述符枚举比较，总 4096 项／512 KiB 名称预算；公开 probe 29 项、全 SSH 54 项／4 项文档、strict／Exact 通过，证据 `ssh-config-source-confirmation.log` 和 `ssh-config-source-directory-stamp-counterexample.log`。当前只读显式批准的文件，临时目录已核实删除；自动发现／原页面与 SQL 导入待接。
## 冷分屏／SSH 的现有拒绝边界（Linux 定向验证通过）

- 独立 `mise run probe-workspace-cold-unbound` 及 Linux CI 已注册：合法 `split/split-pane -v/-h sh` 与已存 UUID 的 `ssh --identity`，在 CLI 端点目录缺失／私有空目录两个状态共十场景，以真实 CLI 和 `strace` 验证零 GUI exec、零端点 connect、零文件改写及后代退出。十场景全部通过，均零 GUI exec、零端点 connect、端点保持且后代退出。
- 新场景不开放 cold allowlist，也不实现冷 SSH／分屏产品能力；不能通过 GUI 拉起创建分屏目标或从后来解锁获得 epoch。共用观察器的原 43 异常场景、5 个冷传输 Rust 反例及 CLI strict 全部通过，任务临时目录已清理。证据为 `cli-cold-unbound-acceptance.log` 和 `cli-cold-unbound-result.json`。

## 已存私钥 SSH CLI（Linux 实窗与完整回归通过，发布审计阻断）

- `pulse ssh <host-id> --identity <identity-id>` 一次捕获原页面／owner／可用 epoch，用同一联合源确定唯一凭据类型；不接受原始地址、私钥、口令、角色或自动解锁参数。密码与私钥各进入强类型动作，原窗口单独批准。
- 三组真实数据库／密文存在性／活跃同 ID 冲突与墓碑／同 DEK 再解锁／页面和取消／过期／源变化反例通过；25 文档测试、模型与组合 Native 严格检查、两处 Exact 诊断通过。见 `ssh-key-cli-original-selection.log` 与 `ssh-key-cli-native-window-first-native-compile.log`；首次缺分支文档和原生方法名错误分别修复后只续未完成阶段。
- 公开 `mise run probe-workspace-cli-ssh-private-key`、原密码 `probe-workspace-cli-ssh` 通过：锁定拒绝，取消／CLI 断开／刷新／外部修改目标后的旧批准零认证；重新读取并延迟 6 秒批准后，仅一次真实签名，零密码回退，三次键盘回显、同连接刷新、92×29→97×30 布局通知、实际流／root／worker 退出。见 `ssh-key-cli-native-window.log` 和 `workspace-ssh-session-{peer,native}-key-cli.log`。
- 按模式独立保存并审阅 `workspace-ssh-session-cli-approval-key-cli-5.png`、`workspace-ssh-session-echo-three-key-cli.png`，目标、私钥身份、按钮及三条回显均可读；截图修复后的完整 key-cli 观察矩阵见 `ssh-key-cli-approval-image.log`。没有采集明文密钥或口令。Linux CI 已登记，不代表远端 CI 已执行。
- 完整合并回归通过：`check/test/probe` 完成（862 前端）；续验 305 SSH App、371 全特性存储／2 集成／32 文档、204 组合 Native 和绑定检查均通过，忽略项不计。见 `ssh-key-cli-joint-baseline.log`、`ssh-key-cli-joint-rest.log`。首次 420 秒总预算中断没有最终 App 汇总，未计通过；核实无子进程后精确清理唯一任务目录，再只续未完成阶段，产品期限不变。最终审计 `ssh-key-cli-joint-audit.log` 退出 5：8 项许可拒绝、2 项维护阻断，策略不放宽。不覆盖冷 SSH、其他私钥算法、Windows／macOS 或远端内核尺寸确认。

## 已存私钥 SSH 产品连接（Linux 实窗与完整回归通过，发布审计阻断）

- `workspace_ssh_key_session_begin/prepare` 与密码流程共用当前页启动银行及取消／超时／实际资源退出监督，身份类别由后端目录与强类型票据核验。原 epoch 和完整 host／identity／Vault／trust 源固定；认证成功不能替代 root 转移或终端绘制授权。
- 31 项协议／选择界面反例、真实生产银行四次签名与输入／关闭回收、竞争请求／错类型拒绝、密码真实 root 兼容、组合严格检查和 Rust Exact 通过，见 `ssh-key-product-peer.log`。
- `mise run probe-workspace-ssh-private-key-session` 四条公开实窗路径通过，见 `ssh-key-product-real-windows.log`、`workspace-ssh-session-{native,peer}-key-*.log` 和 `workspace-ssh-session-echo-three-key-*.png`。三个成功场景各一次真实签名、零密码回退，三次键盘回显、同连接刷新、92×29／97×30 window-change 请求与绘制确认、窗口关闭／显式关闭／EOF 最终屏两次刷新和实际资源退出均核对；取消／导航共两条握手退出、零认证／会话。
- 画面审阅确认最终三条回显无裁切，未采集密钥或口令。Linux CI 已注册，五条密码实窗兼容通过。Windows／macOS、远端内核尺寸确认、其他私钥算法、私钥 CLI 与发布门禁尚待。
- 合并检查、测试、共享探针和绑定检查通过：862 前端、302 启用 SSH 使用的应用、371 全特性存储＋2 集成＋32 文档、204 组合原生；各忽略辅助项不计验收。原密码五条实窗兼容均通过。首轮命令误写不存在的存储特性，按 manifest 以 `--all-features` 只续未执行阶段后通过，原日志保留。审计最后退出 5：8 项许可拒绝、2 项维护阻断，策略未放宽。证据见 `ssh-key-product-final-regression.log`、`ssh-key-product-final-rest.log`、`ssh-key-product-final-audit.log`。

## 当前页私钥接收式保存（Linux 实窗定向验证通过）

- `workspace_identity_key_available/prepare/submit/cancel` 独立于元数据与连接，原页面／controller／epoch／5 秒一次性票据；默认构建不启用导入。两角色密文一次事务提交，私钥与口令不返回页面，不授 SSH 使用权。
- 六项 Native 保存反例与实际解析 worker 取消额度、17 项前端协议和 20 项编辑器反例通过。空表单／UTF-8 超限禁用在加载与切换后的解锁操作中保持；输入提交即清除，未知写结果不重试。`ssh-key-joint-baseline.log` 完整基线、原页 epoch 反例、真实签名和公开导入窗口任务通过：847 前端、299 ssh-use 应用、202 组合 Native；严格及绑定检查通过。审计单独以 5 阻断，8 许可／2 维护，不放宽。TS LSP Unknown 不当通过；首次非零进程风险继续保留。
- `ssh-key-import-paste-stage.log`：两次真实 GUI 进程，锁定拒绝与错口令拒绝的原生回执、正确口令一次双密文 revision、引用不变、同页刷新／进程重开与锁定状态通过；独立 Rust 进程恢复原 DEK 并验证两份字节一致，零终端启动、正常退出。截图只在私钥字段已清空后采集。公开任务与 Linux CI 已登记。公开任务首次保存／刷新／shutdown 回执完成但进程非零退出，未定位，日志 `ssh-key-import-public-first-exit-failure.log`；增加准确退出码诊断后同一 workspace-ssh 构建、原截止下两进程复验通过，见 `ssh-key-import-public-exit-diagnostic.log`，不声称首次风险已修复。
- 独立私钥 root 会话动作已注册，四组真实 Vault／actor／native join、19 项文档测试及严格检查通过；旧页面／变化源／锁定 epoch 拒绝、取消与丢失回执保实际退出、导航后已提交会话存活。见 `ssh-key-session-root-behavior.log`；此模拟协议 sink 不替代实际 SSH 签名或产品 TermView 验收。
- 首轮禁用按钮／焦点观察假设、多行物理打字未完成正确保存的失败日志保留。实际验收改用已有 GTK 系统剪贴板粘贴并等待明确原生完成；剪贴板 owner 与应用退出后清理自有 `/data/temp` 目录。支持范围是 OpenSSH Ed25519（可选有界 bcrypt），其他算法、私钥 CLI 与跨平台尚待。

## 当前页私钥身份元数据（Linux 实窗已验收）

- 独立目录票据绑定原页面、后端快照和共享私库；只接收名称、用户名、是否需要解密口令。凭据引用由后端生成，存在状态从同一 Vault 快照派生。页面刷新、重复票据、外部冲突与取消后 SQL 资源持有的六组原生反例通过。
- 38 项相关前端、196 项组合原生和严格检查通过；Rust LSP Exact，TypeScript LSP Unknown，已运行 tsc 和行为测试，不安装语言服务器。
- `mise run probe-workspace-key-identities` 四次真实应用进程验证缺 Vault、新增、更新、刷新、进程重开、二次确认删除、稳定双引用与墓碑，零 PTY／SSH。`probe-workspace-identities` 旧密码元数据兼容通过。日志 `ssh-key-metadata-window-public-final.log` 与 `workspace-key-identities-*`。保存后同一帧选择器及字段显示新名称；初次 OCR 合词／错误点击坐标、活动 WAL 主文件头观察错误、回执前截图均保留独立证据。
- 本批只开放元数据编辑；私钥接收式导入和共享根密钥会话继续实施。审计仍 8 许可／2 维护阻断；原设置并发 Database 与首次 Profile 读取迟到未定位。

## 私钥／解密口令双密文原子保存（存储边界已验收）

- `KeyIdentityCiphertextEdit` 从原实例联合目录／Vault 快照固定身份及两个独立角色；在同一 SQLite 事务中比较完整源、核验原 epoch／原授权、应用一至两份密文，仅推进一次 revision。保存回执不授读取或 SSH 使用权限。
- 九组实际行为反例通过：真实 AEAD 双角色创建／替换、独立进程重开、数据库无固定明文、错角色／ID／Vault／可选引用、异实例和来源变化、墓碑无复活、第二次 SQL INSERT 故障全回滚、并发两 worker 仅一完整赢家、最终授权／同 DEK 重解锁拒旧 epoch、取消保实际 SQL credit。
- 完整全特性存储 368 单元／2 集成／30 文档测试与严格检查通过；6 ignored 辅助项不计验收，独立进程 helper 由 parent 实际运行并要求 1 passed。三份修改文件 Astrolabe Exact 无诊断。证据 `target/probes/ssh-key-pair-behavior.log`；首次九测试通过但 await_holding_lock strict 失败的原日志保留，锁范围改为实际同步 KDF 调用后全回归通过。
- 应用层 `PrivateKeyIdentityWriteIntent` 已注册并通过默认编译、ssh-use 全目标严格检查；六组原页面／原 epoch 行为、18 项文档与严格检查已通过，Exact 无诊断；首次完整基线旧设置双 worker 并发遇到 Database，新增固定阶段诊断后的默认存储 341 测试通过；错误未复现，根因未定位，首失败证据保留；注册独立私钥 bank 后完整基线正在续验。生产私钥导入／UI／会话接入和全产品验收仍待。
- `PrivateKeyWriteBank`／`PrivateKeyWritePermit` 为独立原页面、原控制器、原 epoch 的单次接收票据。七组真实行为、严格检查、18 文档与 Exact 诊断通过：错 controller／page／ID／重复、取消／丢弃、导航、同 DEK 重解锁、原始 5s 定时器对 ready／taken 的实际回收、close 不锁已确认 master，以及暂停真实 native SQL predicate 时取消仍 Busy 至实际退出。证据 `target/probes/ssh-key-write-bank-behavior.log`。Native 元数据接口仍是待注册候选，不计为导入或私钥产品会话验收。

## 可信后端私钥使用与真实 Ed25519 签名（本批已验收）

- 同一 worker 全目标来源六反例、原页面／原 epoch 准备三反例和受控借用七反例通过；错 DEK、同 DEK 重解锁、不同目标、锁定／换页、解码期间完整来源变化均拒绝。取消／迟到结果不提前释放 SQL 或 native 额度。见 `ssh-key-target-source.log`、`ssh-key-page-preparation.log`、`ssh-key-target-loan.log`。
- `probe-ssh-private-key-use` 使用候选适配器实际源码。真实 Vault AEAD 密文经原 epoch 解密，未加密与 bcrypt 加密 OpenSSH Ed25519 各一次已验证签名；错口令无 offer／签名，无 password／none 回退，三连接本地和远端退出确认。真实加密头 rounds 0／65／u32::MAX 拒绝，输入／口令预算反例与 strict 通过；见 `ssh-private-key-peer.log`、`ssh-private-key-use.json`。首次探针多余 drop 的编译失败和三处条件格式 strict 失败分别保留，没有放宽规则。
- 共用实际 `spawn_decode` 的两项反例，在原生 guard 检查处确定暂停：取消／runtime shutdown 时 CPU 槽仍占用、原 native guard 仍活；放行后拒绝解析、实际退出才释放／复用。共用 worker 重构后的真实签名和 strict 再验通过，见 `ssh-private-key-cpu-lifecycle.log`。不是中断 bcrypt 的证明。
- 本批完整 check/test/probe、801 TS、268 默认／282 ssh-use App、359 全特性 Store（5 ignored 不计）与 28 Store docs、CLI＋SSH／private-probe all-target strict、原密码真实 peer 与新签名／worker 生命周期均通过；见 `ssh-private-key-use-baseline.log`／`ssh-private-key-use-store-full.log`。`ssh-private-key-use-audit.log` 仍 8 许可拒绝＋2 维护阻断（paste／proc-macro-error），策略不放宽。当前只可信 Rust 后端 auth-only，无私钥输入／读取 IPC、生产私钥会话或 Agent。Astrolabe 最新引用索引 stale，不计通过，以当前源码和实际编译核对。任务／native 全退出、owned `/data/temp` 已清理后执行 `mise run clean-dev` 保留 probes；后续双密文原子保存仍候选，未计本批完成。

## 私钥身份 schema 5 与联合来源（可信后端限定验收）

- 私钥与可选解密口令为独立同-Vault／不同-ID角色，密码身份拒绝私钥。新增目录不注册私钥输入／认证 IPC。
- 默认 schema 5 新增 `key_identity_catalog`／`key_identities`，保 UUIDv7／created_at／HLC／墓碑／实例 CAS；先 `.bak-5` 后事务迁移／有界验证，坏来源回滚。trust read/write 支持 4／5，future6 和 unversioned0 拒绝，备份不覆盖。首次整库 345 pass／5 旧版本夹具失败；同步 default5／backup5／future6 后 351 pass／5 ignored（不计验收），strict unused-import 已清理，规则未放宽。
- 9 项目录／联合源行为、core/store strict、不可 Debug／serde 文档测试通过：`ssh-key-directory-joint.log`。含实际两密文 presence、口令删除后 label edit／重开不复活、旧 Vault／目录／密文／foreign store 拒绝，和三 native read／validation 取消信用到实际退出。Astrolabe 两源和 database 为 Exact 无诊断。
- 完整 check/test/probe、801 TS、353 storage（5 ignored 不计验收）和 190 CLI＋SSH Native（3 ignored 不计验收）通过；首次 strict 因 CLI 别名夹具 4 处 unwrap 失败，改成显式错误返回后 15 CLI tests／组合 all-target strict 通过，见 `ssh-key-schema5-baseline.log`／`ssh-key-schema5-windows.log`。最新 schema 5 构建的密码 SSH、身份 CRUD／刷新／进程重开、8 实际连接槽／第 9 个拒绝／单槽释放后复用及正常退出通过，见 `ssh-key-schema5-windows-rest.log`。首次实窗 exit124 停于初始侧栏观察，native/peer 另存 `ssh-key-schema5-first-ssh-{native,peer}.log`；限制 OCR 单线程后原 3s／90s 截止内通过，不据此宣称此前 Profile／绘制超时已解决。`ssh-key-schema5-audit.log` 仍 8 许可／2 维护阻断。未认证私钥、未开放 plaintext IPC，未宣称钥匙串／三平台／断电耐久通过。

## Linux 本地 CLI 别名与敏感冷启动竞争

- `pulse open <已发现本地Shell标识>` 与 `new-tab` 使用相同 `workspace.new-tab`，`split-pane -v/-h` 与 `split` 使用相同 `workspace.split`，没有新Capability、程序路径或通用alias解释器。热open及两个分屏方向、冷open四路径通过；取消／断开／刷新零创建，原页批准后单次创建、实际输入、同PTY恢复和native drain，见 `cli-local-alias-unit.log`（14项解析／strict）与 `cli-local-alias-accept.log`（公共task＋check），截图／输入以 cli-alias 和 cli-cold-open 分开保存。host alias／URL／自动身份未实现。
- `probe-workspace-cold-sensitive` 四场景以两个独立CLI同时exec、两个实际GUI启动attempt及broker实际owner PID核唯一primary，敏感secondary不能转发／借主请求批准。Cancel／disconnect／refresh零PTY，16s后明确批准只有一个PTY，精确回显／同PID恢复／GUI与native实际退出及端点删除通过，见 `cli-cold-sensitive-real.log`。
- `PULSE_CLI_PENDING_PRESSURE=1` 在实际绘制原modal后发12个真实warm open/new-tab，各四场景共48请求全部拒绝且原waiter／modal／端点保持；原cancel与approve链仍通过。证据 `cli-cold-sensitive-pressure-fixed.log` 和四个 `cli-cold-sensitive-*-pressure/pressure-result.json`，只计数不保存secret。helper最初误放只读观察器的NameError失败保留于 `cli-cold-sensitive-pressure.log`；修测试后落盘Pyright 0errors/0warnings。本批不是精确传输额度耗尽或长期压力测试。
- 未新增依赖或改变许可规则，沿用上一完整回归与8许可＋2维护阻断；单窗口selector另批验收中，不提前标完成。三平台、host alias、冷split/SSH、URI与链式执行未开放。

## Linux CLI 标签选择与恢复反例验收

- `pulse focus-tab <session-id>` 只选择当前根持有的 canonical 会话标识；一次原页面 ticket 经 native admission、实际选择／TermView ready、完成回执后才向 CLI 报成功。已通过 16 项前端协议、5 项原页模型、801 项前端回归、13 项 CLI 解析测试和 184 项 workspace-cli native 测试；三条真实窗口路径验证重复选择、两个精确 PID 的独立回显及左右／上下分屏不重建 lease。此前的 `cli-selection-real.log` 与 `cli-selection-*-pid-input.txt`、`*-rendered.png` 保留；不授启动、输入或 cold 权限。
- 本批完整回归在 `restore_roundtrip_and_continuation_property` 发现真反例，不是夹具失败：24×6 的 `a + CUP 6;22 + 中a + DECSC + IRM + CUP 6;22 + BS + 中` 会使恢复丢失首行。`cli-selection-baseline.log` 和 `cli-selection-wrap-before.log` 保留原失败；未通过换 seed／跳过断言掩盖。
- 修正为 `ClippedWideHeadPendingWrap` 明确延期：重建的活动／保存／隐藏光标 pending-wrap 位于末列被剪断的宽字符头时，完整恢复、保历史恢复及同步修复均拒绝并保留源状态。解除保存槽异常后仍要求精确恢复及续流；固定测试、一万组恢复属性和严格 lint 已通过（`cli-selection-wrap-after.log`）。live／actual Ended actor capture 拒绝测试纳入默认套件；不是该异常已经支持无损恢复，也没有修改 Alacritty。
- 最后重链接后的完整 check/test/probe、CLI＋SSH 严格检查通过：801 项 TS、268 项 App、322 项 storage（另5 ignored 不计）、123 项 terminal、184 项 workspace-cli native（另3 ignored 不计）、13 项 CLI 解析及9项实际 socket 测试，见 `cli-selection-wrap-suite.log`。三路径真实选择及默认 v1/v2 Channel 均通过，见 `cli-selection-final-windows.log`；没有将 source 输入、channel send 当绘制 ACK。`cli-selection-audit.log` 仍为8项许可＋2项维护阻断；三平台／IME／DPI、显式恢复延期及既有首屏超时风险保留。

## Linux cold 异常：真实进程／连接观察与源保留

- `mise run probe-workspace-cold-failures` 已登记公有 task 和 Linux CI；已安装的 strace 只观察 process/connect，不观察 read/write、握手 payload 或 token，不归档原始 trace/argv/environment。
- 14种端点状态（残缺 token/socket、普通文件 socket、目录/文件不安全权限、短/非十六进制 token、硬链接/符号链接、拒绝连接、握手 EOF 和5秒超时）分别运行真实 focus/profiles/new-tab；加无 broker 的实际受信 GUI 拒绝退出，共43场景。42拒绝没有 GUI exec；broker 只一次 GUI exec且后代实际退出；9次暖失败 connect 每客户端一次，无 cold fallback/action retry。既有端点设备/inode/owner/mode/link/内容保持，不删除修复。
- 每批使用当前 production CLI/GUI 的单链接私有复制；临时文件均在 `/data/temp`，socket路径预检，线程停止/join及 task 后代 wait 后清理。实际 broker 失败需要 Xvfb，不能算正向 WebView 绘制或三平台验收。
- `cli-cold-failures-public.log`、`cli-cold-failures-acceptance.log`、`cli-cold-failures-result.json` 保存限定证据，后者只包含计数和布尔，不含 secret。全 check、正向 cold focus 兼容过；未改生产代码，不重复全 suite 或审计。Pyright CLI 对当前落盘文件 0 error/warning；MCP旧buffer仍返回三条旧错误，未记为诊断通过、不安装或重启共享服务。压力、配额耗尽、敏感冷入口及三平台仍未完成，发布沿用8许可＋2维护阻断。

## Linux concurrent cold只读：双CLI真实竞争与单primary

- `mise run probe-workspace-cold-concurrent` 公有任务/LinuxCI已登记。FF/PP/FP三pair由两个独立shim同时释放后exec真实固定CLI，每pair观察实际两个task-owned GUI启动attempt（不把第二CLI实际warm误计cold）；真实brokerowner PID结合exact exe/runtime/PID/title/XID选择primary，两CLI均一次成功，无claim/action retry。结果都为2attempt、[0,0]、1survivor、0PTY；warm读取和focus后socket/token inode/权限/bytes不替换，GUI实际waitpid exit0、端点删除过。
- 证据 `cli-cold-concurrent-{real,owner,public}.log` 及 {ff,pp,fp}-client-{0,1}.log/result.txt/rendered.png。初次FF过、PP BadWindow是fixture选到短命secondary，不是放宽产品限制；改为独立gdbus broker实际owner观察后过，失败log保留。四cold new-tab公有兼容与完整check过，production未改，沿用上批783TS/178native及8许可/2维护审计，不声称本批重跑全test/probe/audit。
- 仅Linux只读三pair；敏感动作、quota耗尽、异常退出／压力和三平台全矩阵未验，不将临时启动进程数量当长期多primary，也不从双CLI成功推性能目标。

## Linux cold new-tab：原页显示就绪、明确批准与原生回收

- `mise run probe-workspace-cold-new-tab` 四独立CLI真实冷GUI/X11：默认Cancel、CLI断开和未决refresh均零PTY；16s后原窗批准仅创建一次，两唯一键盘输入与实际DOM回显、同PID刷新、GUI/native实际退出和端点删除通过。端点inode/权限/token保持；默认umask022与私有install-like sibling不绕过launcher。`target/probes/cli-cold-new-tab-*-confirmation.png/client.log/result.txt`、approve-rendered.png/input.txt留证，不记录密码/token。
- 原document Ready与presentation共享8s metadata、不占native/SQL/CPU信用或授执行权；UI监听安装、Profile读取和初始会话恢复成功后report，backend复核真实source/current nonce/WindowToken/原Arc。换页/foreign registry/close/迟到ready/cancel反例及29前端行为过，取消仅waiter局部。认领后复用Cap/GUI30s/native5s，outer45s不裁慢批准，不借新页、不重试未知结果。
- 完整check/test/probe、783TS、267App、322storage（5ignored不计）、121terminal、178workspace-cli Native（3ignored不计）、9真实socket与CLI+SSH all-target strict过；cold focus/profiles与single-instance公有兼容亦过。证据 `cli-cold-newtab-{check,accept,accept-rest,audit}.log`；Rust诊断Exact、TS Unknown不算通过。
- 首次夹具早于page Ready读取、OCR线程超时及黑底/cursor识别均留失败日志real.log/fixture-fixed.log/single-thread.log/accept.log。修唯一CLI/sh modal观察、单线程OCR及反色精确文字crop，不放宽8/30/5s；WNOHANG0不是child exit，cleanup等当前唯一runtime子进程实际reap再删owned fixture。实际第二echo截图已确认。
- audit exit5：仍8许可拒绝／2维护阻断，未改policy。只验Linux cold new-tab，cold split/SSH/open、并发cold完整矩阵、三平台和性能仍待；原Profile首屏timeout风险保留。LinuxCI和公有task已登记。

## Linux cold Profile只读发现

- `mise run probe-workspace-cold-profiles`首次独立CLI拉起唯一GUI并只返回闭合固定bash/sh JSON；原页scope/Cap/ProfileReader与5s native read不变，0PTY、3warm focus、端点identity/token不变、完整Profile UI及WM关闭实际exit/端点回收通过。证据 `cli-cold-profiles-client.log`、rendered.png、result.txt；共享fixture按mode分别归档，cold-focus公共兼容亦过。
- 9startup模型／11Bank（原scope等待/once/refresh/敏感拒绝不占quota）、12CLI、172workspace-cli Native（3ignored不计）、check/CLI+SSH strict与公开single-instance三页通过；model.log/bank-public.log/fixed-public.log/acceptance.log各阶段留证。首次public失败是Bank仍focus-only而非等待超时，修真实consumer；旧phase测试将profiles列为拒绝已更新。首次MCP超时不计，后续startup/bootstrap诊断Exact。
- 同一metadata mask用于CLI/Root/callback/Bank/认领后检查，不授执行权；确认型cold new-tab/split/SSH仍拒绝，15s metadata预算不扩大reader5s。777TS旧基线未变沿用，非本批重复全test/probe。audit.log仍8许可／2维护阻断；原首屏/并发冷拉起完整矩阵/三平台未完。LinuxCI登记。

## Linux cold focus：原文档、单实例与私有数据初始化

- `mise run probe-workspace-cold-focus`：独立pulse拉起唯一GUI，原nonce Ready后一次UUID认领、三warm focus、真实X11焦点、端点dev/inode/token不变、0PTY、只读固定Profile JSON、WM关闭actual waitpid与端点消失。`cli-cold-focus-rendered.png`显示Bash／POSIX sh和空终端；同字节ELF复制私有单链接install-like布局，不放宽launcher。
- 默认umask022反例发现WebKit先建0755使Profile拒读。SDK构窗前复用Unix prepare新建0700，既有0755/symlink拒绝且不修权限/内容；3native反例通过，directory-modes.log/discovery.log证实700/readOnlyExit0/bash+sh。此前focus成功但仍初始化的截图不算workspace-ready。
- `probe-workspace-single-instance`：三页secondary不替换原primary/PTY/端点，安全focus与确认型元数据拒绝；插件异常另核实际D-Bus owner PID，broker双进程foreign/释放反例通过。
- `cli-cold-focus-regression.log`完整check/test/probe、777TS通过，尾部误用不存在的cli feature记失败；`regression-rest.log`以真实cli-control续未执行段：8socket、168workspace-cli Native（3ignored不计，brokerchild由父测试实际执行）、12CLI、CLI+SSH strict与两个公有实窗通过。Root/app_data诊断Exact。
- checkpoint拒refresh/关闭/同nonce新页，8s metadata等待不授scope；UUID认领仍走CLI Capability，15s bootstrap不改native5s；warm失败/部分/占用端点不冷重试。当前cold仅focus，其他cold动作/open/并发冷启动完整矩阵/三平台待交付；audit仍8许可／2维护阻断，原Profile首屏风险未归零。LinuxCI登记两任务。

## Linux warm CLI：原 epoch／保存目标联合批准的 `pulse ssh`

- 只接受 `pulse ssh <保存主机UUID> --identity <保存身份UUID>`；独立 CLI Confirm/Window，CLI-only 构建拒绝 SSH，Palette／Keybinding 不获得 CLI 权限。GUI只显示有界名称、目标、端口及用户名；不返回密码、密钥或 Vault 读取权。
- 等待前捕获keyless原页／Vault epoch gesture；单次消费后启动再次复核原epoch。真实密码与SQL反例覆盖同DEK重新解锁也拒旧意图，取消仍保native read credit到退出。原页／registry／operation／冻结Host+Identity source与真实peer pin先于凭据使用；密码仅瞬时借给受监督适配器。
- `cli-ssh-accept.log`：完整check/test/probe、777TS（20审批协议反例）、271App cli-control+ssh-use通过。`cli-ssh-accept-rest.log`：153Native（另2ignored不计通过）、7CLI、组合与CLI-only all-target strict通过。组合目录固定4动作测试旧期望已修正为feature/enable精确4或5，不放宽policy或schema。
- `mise run probe-workspace-cli-ssh`与LinuxCI登记。`cli-ssh-focused-source.log`及accept-rest真实peer/WebView：锁定、默认取消、CLI EOF、未决refresh auth0；独立进程只改同库固定host label后旧批准拒绝（source-mutation.log／result-4.txt），新source等待6s后明确批准仅auth1（result-5.txt为session 1）；三次实键可见回显、同连接refresh、SSH通知型resize、SDK/native/root/端点排空。公开split两方向兼容通过，完整任务终态在 `cli-ssh-split-public.log`。
- 原source-check及独立重测在首次profiles_listed观察超时，source变更未执行，旧result不是当前证据；`cli-ssh-source-check-failure.md`及两失败日志保留。增加PID/唯一XID/超时截图与原80×0.1s预算内原窗早激活后成功，不能解释或抹除原bootstrap风险。300s外层续跑已完成两方向GUI但未获得完整终态，记超时失败；缓存复用独立公开任务53.72s完成，未扩大native5s／GUI30s／client45s。
- Rust catalog与fixture诊断Exact；TS语言服务不可用不计通过，实际TS检查仍通过。`cli-ssh-audit.log`仍8许可拒绝、2维护阻断。cold-start、single-instance、open CLI、真实远端与三平台均不计完成。

## Linux warm CLI：原窗格确认的双方向 `pulse split`

- CLI只接受 `pulse split -v|-h <已发现Shell>`，独立Confirm/Window动作；原单pane/session/实例在询问、批准和新session通知时复核，换标签后切回、busy、已split、Ended和旧页拒绝。Pane ID不授native权，仍走既有Profile/root启动。
- 15个前端审批／目标隔离反例、772TS、146Native（另2ignored不计）、6CLI、8真实socket通过，完整check/test/probe与CLI＋SSH组合strict通过：`cli-split-acceptance.log`。仅GUI批准动作完整客户端响应45s；hello/auth/write/native启动仍5s、GUI30s不延长。真实socket和GUI均覆盖等待6s后明确批准，而非快速自动批准掩盖原客户端5s预算遗漏。
- `mise run probe-workspace-cli-split`及LinuxCI已登记，独立进程请求左右／上下分屏各一轮：默认Cancel、CLI EOF、未决refresh零创建；批准后恰好两PTY、可见双pane及第二pane实键回显、refresh不重启任一PID、有序root drain和端点删除通过。原 `probe-workspace-cli-new-tab`公开路径兼容通过。截图 `cli-split-{row,column}-{layout,rendered,confirmation-4}.png`、input.txt、请求收据归档。刷新仅恢复选中会话，不宣称布局持久化。
- 原始native测试unwrap触发strict已改为Result返回；最初客户端split响应预算漏用原5s，已修正并增加慢批准反例。图像已人工检查两方向真实双终端。Astrolabe诊断超时，不算Exact；`cli-split-audit.log`仍8许可拒绝/2维护阻断，未放宽任何策略。
- 新session的root移交不等于layout/drawing完成，显示目标失效时会话保留供重新选择，不自动关闭或重试；native/SQL信用跟实际任务退出。cold startup、SSH/open CLI、嵌套分屏、布局持久化及跨平台仍未完成。

## Linux warm CLI：原窗口明确批准 `pulse new-tab <Profile>`

- 可选 `workspace-cli` 单独登记 Confirm/Window `workspace.new-tab`。只接受后端已发现且未隐藏的固定Shell标识，不接程序路径、参数或环境；Safe发现和hello认证不是启动权。批准前后复核同一原Ready page/registry/local WebView，不拿新页面替换原请求。
- 原窗口单次票据、完整PreparedWindowAction、GUI30s和原页Cancel默认；票据不能自证权限，取消/EOF/换页/过期/占用/跨页/foreign均拒。批准后重新读取当前私有Profile偏好和发现，复用5s native启动及root collection。仅后端登记的该operation具有45s总等候预算，帧/hello/写回和其他操作仍5s。信用跟随实际SQL/native任务；root接管后回执遗失不回滚、不自动重试。
- `mise run probe-workspace-cli-new-tab` 已实际通过，LinuxCI已登记。既有focus/profiles三页与第四次final-capture复验后：modal截图确认正确sh和Cancel默认；按默认Cancel、断开独立CLI及modal未决时实际reload均保持一个PTY。独立第四请求明确批准后只有一次新建、两个原生PID并存，第二PTY真实输入和第六页refresh恢复完整回显；关窗两个PID与socket/token确实退出。frame ACK/Commit与source恢复链保持原机制，不把Native回执当绘制证据。证据 `target/probes/cli-newtab-public.log`、`cli-new-tab-confirmation-{1,2,3,4}.png`、`cli-new-tab-rendered.png` 与 `cli-new-tab-input.txt`。
- `check/test/probe`、765TS（8协议反例）、144Native（2ignored不计）、5CLI二进制参数/回执、6真实socket及all-target strict通过：`cli-newtab-regression.log`。`workspace-cli,workspace-ssh` 组合 all-target strict 亦通过（`cli-newtab-ssh-combined.log`），只证明组合构建／注册兼容，不替代真实SSH或跨平台验收。Astrolabe本轮diagnostic超时未通过，不宣称Exact；审计 `cli-newtab-audit.log` 仍8许可拒绝/2维护阻断。最初实窗计数漏算已有第四页，已修复探针精确六页；最初截图早于modal/最终字符绘制，公开任务已加强实际modal覆盖检测并留完整图像，不改产品deadline。不是cold startup、其他CLI动作、系统IME或跨平台验收。

## Linux warm CLI：只读 `pulse profiles`

- 独立 CLI 只接受无附加参数的 `profiles`，通过单独 Safe/Window `workspace.profiles` 调用既有私有 `ProfileReader`；OS 路径、PATH/SHELL 由后端提供，不接受客户端路径、argv、env。不是启动、修改偏好或隐藏项恢复权限。默认／其他surface不获得CLI动作。
- 原 Ready scope 在真实 GUI 线程读取前／发布前核验，取消、未确认、跨窗口／registry、换页和关闭均拒；新页不能接旧结果。原SQL/discovery监督与根退出屏障继续持有信用直到真实退出。输出只包含版本和最多五个未隐藏的固定Shell enum；重复、未知、控制字符、额外字段和参数拒，失败不fallback。隐藏过滤的新增反例为codec测试，三次实窗本身不计新的隐藏／恢复或跨进程重开验收。
- `mise run probe-workspace-cli` 包含三次独立 `pulse profiles`：真实三代WebView都返回 `bash/sh`，与focus/exact键盘输入/退出drain兼容，同一PTY的创建计数不变。无server与额外路径／program／all参数拒绝，未创建runtime。证据：`target/probes/cli-profiles-native-window.log`、`workspace-cli-profiles-{1,2,3}.json`。
- 3个共享codec、1个CLI语法、1个原页核验反例，严格lint、140项原生（2 ignored不计）、757项TS与完整 `mise run check/test/probe` 通过：`cli-profiles-core.log`。Rust修改文件诊断Exact无错误；最初strict失败是测试unwrap，已修而未放宽规则。审计仍8项许可拒绝/2项维护通知、exit 5：`cli-profiles-audit.log`；没有调整deny策略。
- `new-tab/open/ssh`、GUI高风险确认、冷启动／单实例、其他平台尚未由这个只读发现入口开放；不计CLI-01或Alpha整体完成。

## Linux 生产 warm CLI：固定 `pulse focus`

- 可选 `workspace-cli` 注册固定 CLI surface `workspace.focus`（Safe/Window、闭合空参数）；Palette／Keybinding 不因此获得执行许可。独立 `pulse` 二进制只接受 `focus`，从 OS XDG runtime 读现有私有端点，不创建目录、重置 token、启动 GUI 或自动重试未知结果。
- handler 先捕获当前 Ready 原窗口 scope，再在真实 GUI 主线程 effect 前核对原 Arc 身份、当前 generation、实际本地 WebView 来源、取消及单次参数消费。返回 `focusRequested` 只表示 native 调用完成；不能当最新画面、compositor 激活或其他动作权限。根持有 server，退出先同步撤页与服务，再 await listener／请求任务实际结束，随后与 PTY／SQLite 原有屏障一起完成。
- `mise run probe-workspace-cli` 公共入口通过：三次独立 CLI 进程从 Xmessage 干扰窗口恢复实际 Openbox 焦点，三页同 PTY PID 与三条 exact 键盘输入，关闭后 socket/token 消失、旧客户端不能重连。未知动作、`--program` 和无 server 也拒绝，缺席客户端未创建 runtime。证据：`target/probes/workspace-cli-{public-task,window}.log`、`workspace-cli-focus-result.txt`、`workspace-terminal-cli-native.log`。首次 Xmessage 未设 `_NET_WM_PID` 使测试 lookup 失败，独立 Xvfb 证实后改固定标题查找；公共 wrapper 首次失败因证据未清空重复 append，已每批清空后通过，不放宽三次数量。
- `mise run check/test/probe`、六个真实 Unix socket 测试、136 项 workspace-cli 原生测试（另 2 ignored 不计通过）、757 项 TS、CLI 全 target 严格 lint 通过：`workspace-cli-full-regression.log`。五个 Rust 修改文件 Astrolabe diagnostics Exact 无错误。`mise run audit` 仍失败：实际八项许可拒绝、两项维护通知（`workspace-cli-audit.log`）；新增 `interprocess` 依赖的 `doctest-file`、`recvmsg` 均为未允许的 0BSD。最初沿用历史六＋二的汇总有误，本轮核对完整报告后更正；未放宽 deny 策略，未获许可批准不能计发布通过。此前 SQLite 首次 BUSY／Profile 超时风险仍保留，不用本次通过抹除。
- 仅当前 Linux 环境受控验收；其他 CLI 动作、GUI 高风险批准、冷启动 `startup-actions`／Tauri single-instance、跨平台／Windows 管道 ACL 仍未交付，不能据此标 Alpha 完成。

## Linux warm CLI 传输基础（历史前置验收）

- `pulse-app/cli-control` 可选 feature 的私有 UDS 测试六例通过：成功 hello 后仅固定 mock `workspace.focus`，错 token、未认证请求、重复参数、超长帧、已占端点、unsafe 目录／符号链接均拒绝；EOF 和被取消的 shutdown 观察者不会提前释放 native 模拟任务或 socket。`cargo clippy -p pulse-app --features cli-control --all-targets --locked -- -D warnings` 通过。证据：`target/probes/cli-warm-foundation-socket.log` 与 `cli-warm-foundation-lint.log`。
- `mise run check`、`mise run probe` 通过。此次 `mise run test` 的 `pulse-store::edit_tests::independent_workers_merge_and_stale_snapshot_is_rejected` 在 `independent_workers.edit_a` 报 `DatabaseBusy`（extended 5），该次完整测试 **失败**，不能计为通过；独立私有临时目录的单例重检 1/1 通过，随后同配置 `mise run test` 完整复验通过（含 757 项前端测试）。首次 BUSY 的来源仍未定位，后一次通过不证明并发风险消失。证据：`target/probes/cli-warm-foundation-{check,test,probe}.log`、`cli-warm-store-isolation.log` 与 `cli-warm-foundation-test-recheck.log`。Astrolabe 新文件索引 Unknown，未计语言服务通过。生产 workspace 尚不注册此 listener，也没有真实 Capability dispatcher/CLI 二进制或冷启动单实例验收。随后添加受信后端原窗口 `ProductDocuments::current_native`，未确认/foreign/旧generation/close 反例 1/1 与六个 socket 测试、strict lint 再通过（`cli-native-page{,-lint}.log`）；瞬时返回的 scope 不是 effect 授权。

## 独立 SSH 会话动作与原生根移交（受控 Linux）

- `mise run probe-ssh-owned-session`：私库合成密文经已确认 DEK／原 epoch，绑定实际对端后明确会话动作认证两次。原页退休后第一根连接继续 actor 回显，有序 EOF；第二连接在 native read 挂起时 cancel，channel tasks／SDK／owned stream／actor 均实际退出。证据 `target/probes/ssh-owned-session.json`、`ssh-owned-session.log`。
- 真实密文到 typed root 的反例覆盖 foreign page 预约 auth0、认证后 lock 不移交、setup 取消占信用到 native 退出，以及已完成 owner 回执丢失仍关闭并继续回收。普通 target-use auth bool 仍 drain-only；不允许升级或返回明文。
- 受影响 check／默认246 application、ssh-use249／SSH22／TS612／probe／原auth-only／bindings／strict lint分别通过（`password-session-remaining.log`）；完整 optional storage317 与 doc／integration复验通过（`password-session-store-followup.log`），ignored 不计。
- 原组合 `password-session-full.log` 在独立 worker edit_b 返回 `store.database_failed` 停止，不算通过且来源未定位。带不含记录内容的 SQL stage/code 诊断的完整复跑又在 full-queue cancel 的2s join超时；该fixture原将数据库初始化与迁移计入取消截止。仅把准备阶段移出计时，2s取消预算、生产实现与250ms SQL锁等待不变，针对性与完整存储复验通过。诊断中 BEGIN BUSY5来自预期锁定测试，不能据此归因原失败。
- 第一次两连接 probe 曾因误把 page retirement 当作 lock 而对仍可用 Vault 再 unlock，正确返回 Busy；fixture改成明确 lock 后重新认证，不修改产品锁策略。
- 共享生产 `SessionRoot` 已接协议 owner 与工作区授权后的 typed close／退出 drain，local API不变且同一64容量／retiring／双预算。4个native-latch＋18原local回归、默认250／ssh-use253 app、125原生（2ignored不计）及check／probe／bindings／strict lint通过；实际SSH probe要求 sameSharedRootAdopted 并验证EOF/cancel真实reap。MCP曾超时，独立Root Exact诊断通过；并行网络任务曾耗尽180s在Cargo artifact锁而未执行，native完成后只补行为通过（protocol-shared-root-network-retest.log），不能计原任务通过。
- 此适配器固定32×8、合成远端 PTY；没有 OS远端PTY、真实WebView、Tauri会话启动入口或 resize验收。window-change 发送入队不是kernel ACK；当前拒绝resize。审计仍6许可证＋2维护公告，不自动安装LSP或解除签名边界。

## 工程基础

- `.mise.toml` 固定 Rust 1.95.0、Node 22.23.3、pnpm 10.34.6、cargo-deny 0.20.2；任务统一使用 `mise run`。
- workspace 实例化 `pulse-core`、`pulse-ipc`、`pulse-term`、`pulse-pty`、`pulse-route`、`pulse-app`、`pulseterm-desktop` 与 `xtask`；原生 GUI 依赖受 `desktop` feature 控制，默认仍可做 headless 核心检查。
- `cargo xtask check-deps` 检查已实例化 crate 的直接依赖方向、Tauri 隔离和领域层已知 I/O 依赖；这是初始规则，不是完整的传递依赖/源码安全证明。
- Rust 禁止 unsafe；生产代码禁止 unwrap/expect；TS 开启 strict 和 noUncheckedIndexedAccess，使用 Biome。
- GitHub Actions 配置 Linux/Windows/macOS 检查、测试和探针；VT 样例禁止换行归一化。

## 二进制 IPC 编解码

`pulse-ipc` 编解码与 `ui/src/ipc/frames.ts` 解码共同使用 `tests/fixtures/frames/data.hex`。

- 6 字节头：kind、保留 flags、little-endian u32 sequence；payload 不超过 1 MiB。
- 拒绝短头、未知 kind、非零 flags、超长数据；TS 正确处理非零 byteOffset，payload 引用原 buffer。
- Rust 属性测试覆盖随机载荷往返、任意输入不 panic。新增 sans-I/O `CreditWindow`：同时限制帧数和载荷字节，只接受已发送序号的累计 ACK；重复、过期、未来 ACK 不增加额度，支持 u32 回绕。
- 真实 Raw Channel 与本机 WebSocket 复用此帧格式和信用窗口；尚未实现生产会话重绑定、顺序恢复或桌面载荷分片。

## 终端双解析器探针（S2、ADR-026 的部分证据）

```sh
mise run probe
# target/probes/terminal.json：Rust 观测、VT 重放、应答和内存下限
# target/probes/xterm.json：跨解析器比较结果，探索性失败同样记录
```

- 使用 alacritty_terminal 0.26.0 / vte 0.15.0 和 xterm headless 6.0.0；主探针使用项目 SharedUnicodeProvider，宽度表从锁定 unicode-width 0.2.2（Unicode 17）生成，公开 experimental API 注册，无第三方 fork。
- 通过 7 个样例（包括原探索性 ZWJ 现已提升为严格门禁）：ASCII、颜色/样式、中文与基本 emoji/组合音符、备用屏、返回主屏、光标位置查询。比较内容、宽度、颜色、部分样式、光标位置和备用屏标志。
- 前端策略阻止常见 CSI/DCS/OSC 查询应答及 OSC 52；新增公共 DECRQM（`CSI $ p`）拦截，修复只拦截私有模式而遗漏公共模式的重复应答缺口。单独测试先观测 xterm 原有应答，再安装 hook 验证应答消失，并在释放 hook 后验证模式写入未被吞掉。
- `tests/fixtures/terminal/queries.json` 的 23 组共享用例覆盖 DA1/DA2、DSR、CJK 光标位置、公共/私有 DECRQM、字符尺寸、多查询应答顺序及 OSC 52 读写拒绝。Rust 检查每种固定块大小（1 到输入字节长度），逐块消费应答；前端检查 1/2/3/7/整流分块，必须没有协议应答且屏幕与 Rust 一致。查询矩阵写入 `terminal.json`，跨解析器结果写入 `xterm.json`，由现有 `mise run probe` 和 CI 执行。
- OSC 4/10/11/12 由 backend 使用主题 fallback 与 remote OSC override 应答；在 BEL/ST 边界先收取事件，再继续解析，防止后续 set/reset 改变较早查询的回答。actor 提供有界 palette 与 cell geometry 控制命令。像素尺寸只有实际测量且乘法不溢出才应答；未知时不编造尺寸。这不是完整查询兼容性证明，真实 UI 配色/测量同步仍待接入；矩阵中 OSC 52 无应答仅证明探针的拒绝策略，不证明应用已实现完整的剪贴板权限流程。
- 原先 ZWJ 的格子布局还随 xterm raw Uint8Array 分块改变。每流 `Utf8Stream` 先保留不完整 UTF-8，空解码不调用 xterm；再使用同一 Unicode 17 scalar provider。`zwj.vt` 已改为严格门禁。16 组共享文本以 1/2/3/7/整流分块检查格子、光标和分块稳定性，shared provider 均通过；Unicode6/11/15 与实验 grapheme provider 仅作差异对比，不用于主链路。
- `unicode-table-check` 重新从实际 Rust 表生成内容并只读比较；TS 穷举 0..0x10FFFF 的分类指纹和 Rust 生成值一致。表与适配器用的是**标量宽度**，并非现代 grapheme 宽度：例如家庭 emoji 仍占多个宽格子，没有宣称完整 EGC 字形/字体/IME 支持。实验 grapheme addon 只作为 MIT 开发依赖，不进入产品渲染路径。
- `mise run probe-snapshot` 是完整恢复门禁，并输出双解析器分歧矩阵与恢复体积（`target/probes/snapshot*.json`、`token-matrix*.json`）。已按用户批准增加唯一的非活动网格只读getter，恢复隐藏主屏及备用屏保存光标，保留发布包Apache许可证、VCS来源和SHA-256，211文件来源校验、178项上游测试接入CI。源代码故意漂移探针确认校验失败且不覆盖。三万组release属性测试无失败（延期和近似没有算成精确通过），指纹加入隐藏主屏格子/current/saved和备用屏saved cursor。新增shrink/grow、退出/重入及超链接笔回归。不能无损编码的隐藏光标分离和非行尾pending-wrap返回明确RestoreGap，不再静默丢主屏。xterm矩阵有545个可编码非近似场景，条件M1|M2为299/300，random-85仍记录为分歧；不能声称补丁根治解析差异或S2整体通过。本轮仅有Linux验证，尚未接入生产attach。原有10/62个解析序列分歧仍需同步策略。数据、方案对比与建议见 [完整恢复评估](../architecture/terminal-restore-evaluation.md)。尚未接入生产 attach。
- 非规范 UTF-8/C1 在浏览器的实际行为、组合簇边界的更多控制序列场景、live 字体/IME、历史同步与真实程序录制流仍待验证，总体 S2 未通过。

### 已修复：UTF-8 分块时漏字符

全流解析和分块解析对 `e + U+0301 + 空格 + 🌍` 曾产生不同结果。vte 0.15.0 在补全前一块的部分码点时，可能把紧随其后的 ASCII 字节计入消费长度，却只输出第一个码点。

适配层在进入解析器前保留最多 3 字节的未完整 UTF-8 尾部，下一块补齐；不修改非尾部的非法字节或 C1 控制字节。新增所有固定块大小的状态一致性、组合音符后空格、分块中文和非法字节边界回归测试。

`TerminalProbe::finish_input()` 明确 EOF 策略：一个未完整的尾部 Unicode 标量替换为一个 U+FFFD；重复调用不重复输出，完整输入与未闭合的 CSI/OSC 不被强行补全。6 种不完整标量前缀及幂等性有回归测试。该方法不会恢复/序列化 ANSI 解析器状态，生产 transport 的关闭路径尚未接入。

新增 proptest 混合流检查：ASCII、CJK、emoji、组合音符、SGR、光标移动、备用屏切换和查询按随机长度分块，逐块收取应答后，最终屏幕和应答顺序必须与整流输入一致。范围限于这些合法 UTF-8/控制序列，不等于任意恶意字节流已完整验证。

### 视口重放不是生产快照

`TerminalProbe::screen_replay` 仅用于可见格子重放，不得用于真实 detach/attach；完整恢复使用 `TerminalProbe::restore_sequence`，覆盖范围与缺口见评估文档。

### 重连协议、压缩与最近历史（新增证据）

详见 [事务协议](../architecture/terminal-attachment.md)。已实现二进制 Attachment v1、generation/offset/额度校验、TS有界事务接收和渲染callback ACK监督，禁止Commit前live/部分渲染。共享固定hex fixture检验u64极值与旧generation ACK。

`diagnostic_bootstrap` 在同一actor命令边界捕获历史/屏幕；源状态不变、超限不返回部分结果。最近2000物理行/4MiB cap，软换行cutoff前移至合法逻辑行边界（可能少于请求）；主屏历史在alt激活仍可读取。不支持的宽字符边界明确拒绝，未做deep paging/resize reflow完整验收。

恢复按连续pen压缩和省略默认尾格，并增加不含RIS的screen-only方式保护历史，显式恢复两buffer的tabstop和scroll margin。发现并修复SGR颜色直接写buf绕过Emitter预算的旧漏洞，新增多色预算回归。`probe-snapshot`现在559个可编码场景，条件M1|M2=311/311，无RIS与完整恢复也必须一致（不能据此宣称所有解析序列一致）。纯文本120×40恢复从约62KB降到5625 bytes，最坏159950 bytes未明显下降；性能时间需release报告，debug约8ms不作产品指标。

3个`probe-attachment`真实headless案例通过：183/196/161个片段，primary中文/emoji不完整UTF-8、alt部分CSI、soft-history；检查10/10/8行历史的每格颜色/属性/wrap，2字节live续流无查询泄漏。新增前端取消、慢sink、额度过载和stale generation测试；仍未接入Tauri窗口/PTy。

EchoTransport及screen_replay已限制到test/validation，独立默认构建负向编译门禁通过，防止Cargo workspace feature统一掩盖错误。全部新任务加入三平台CI，仅Linux本地验证，其他runner尚无执行证据。workspace实际57项pulse-term + 12项IPC + 3项PTY + 1项core + 2项xtask测试，TS50项通过；以实际运行数字为准而非历史总数。

### 分歧检测与同边界摘要

372个token/base组合中18个差异（10种token）均被保守风险提示覆盖，漏标记会使CI失败；11个可精确编码的状态已在真实headless消费器中验证screen-only修复，另外7个不可表示/延期不能算修复通过。发现并修复恢复器对“宽head的spacer已擦除”和“origin域外current cursor”的精确性误报，actor拒绝这些帧，未修改解析器。

digest v1按统一UTF-8/u32投影计算，372个跨语言摘要fixture均匹配；checksum相等只证明当前visible投影，非hidden/history/全样式/安全认证。actor只在同revision/parsed_bytes/size且digest相同时清risk，旧boundary/错误digest不清；REP跨分块观察和RIS不得静默清风险都有回归。所有新Rust检查通过；快照严格分类现557，条件M1|M2=311/311（此前559中的2个新增不可表示状态没有伪装精确）。可见窗口的idle/attach/resize限频监督仍未接入生产driver。

### 历史内存预算仍未满足

120 列、40 行视口、保留 10,000 行历史时，按 `size_of::<Cell>()` 计算的格子存储下限为 **28,915,200 字节（约 27.6 MiB）**，不含 extra、分配开销或进程内存。

当前是有上限的热历史，不是冷热分层/压缩归档。10 万行历史目标需要独立压缩存储，不能直接沿用 alacritty 原始格子。此数值也不是整棵进程树的内存测试。

## 真实 WebView 数据通道（S1 的部分证据）

```sh
mise run desktop-check
mise exec -- cargo test -p pulseterm-desktop --features desktop --locked
mise run probe-channel
# target/probes/channel.json
```

- Linux Debian / WebKitGTK 2.54.0 / GTK 3.24.49，Xvfb 1280×800；debug Rust 构建、production Vite 资源。没有关闭 WebKit sandbox。
- 每种传输运行：64 KiB × 256 帧（终端流大小）、256 KiB × 256 帧（桌面块大小）、64 KiB × 32 帧（75 ms 延迟累计 ACK）。Rust 发送确定性载荷，前端逐字节检查数据、长度、序号、总量，ACK 仅在消费后发送。
- 实测完整性通过；最大在途载荷不超过 `window × chunkBytes`，最大帧数不超过 window。取消测试通过，取消后新 Channel 能重新运行。另有信用不足等待、伪 ACK、重复 ACK、独占运行槽与无令牌 Socket 拒绝测试。
- 两次本地完整运行均通过。示例测量：Channel 终端流 21.40 MB/s、桌面块 52.43 MB/s；WebSocket 对应 37.62 / 49.93 MB/s。Channel 送达 p95 为 24.19 / 20.88 ms，WebSocket 为 5.66 / 4.28 ms；数值存在运行间波动，不能作为基准或性能承诺。
- Socket 仅绑定 127.0.0.1 随机端口、单次连接、随机令牌及固定子协议；接收数据消息上限 4 字节，关闭/无 ACK 均有超时或取消保护。probe command 只向本地 main WebView 注册，没有开启远端权限。
- 送达时间通过 7 次时钟往返选择最小误差样本校准；报告含时钟不确定度、ACK RTT p95、JS 16 ms 定时器延迟及在途峰值。定时器响应不等于输入到绘制延迟。
- 合成数据并不经过 terminal parser、Canvas、WebGL 或 RDP 解码，不测 GPU/端到端渲染，也不证明生产缓存/整棵进程树内存有界。三个平台的真实显示环境、系统输入负载、多会话公平性及持续高吞吐仍待验证；S1 尚未整体验收通过。

## 类型、命令与事件生成（S21 的 Linux 控制面证据）

- 精确固定 `specta` / `tauri-specta` 2.0.0-rc.25 与 `specta-typescript` 0.0.12；只启用在桌面适配器，headless 核心没有 Tauri/Specta 依赖。ADR-018 暂保持 Proposed，未自动转为正式采用。
- 最初8个、现12个命令与 `ProbeEcho` 事件共用 `command_bindings()`：同一份注册表生成 `ui/src/ipc/gen/commands.ts` 并注册 invoke/event handler。DTO 包含嵌套对象、枚举、数组和 nullable 字段；前端不再手写重复的配置、统计、测量与报告字段。
- `RawFrame` 不实现 Serialize，仅通过 `IpcResponse` 返回 `InvokeResponseBody::Raw`；反射映射为 `ArrayBuffer`，生成参数为 `Channel<ArrayBuffer>`，不存在 number[] 或 JSON 字节序列的替换。Rust 单元测试和实际 WebView 的逐字节检查均通过。
- 增加带随机 nonce 的生成命令 → Rust 类型化事件 → 生成事件监听器回环。监听先注册，随后发命令，超时/成功都释放监听；最初原生报告版本改为2，后续窗口组合探针升级为3（详见下文），必须有 `typedEvents: true`、取消证据及 7 组测量才可标记成功。Linux 实测回环与全部传输组通过。
- `mise run bindings` 写入绑定；`mise run bindings-check` 比较重新生成的临时内容但不覆盖源文件。手动改变绑定注释的行为探针确认检查失败且源文件哈希不变；随后恢复原文。三平台 CI 已加检查，但尚无远端运行证据。
- **已验证的限制**：rc.25 的递归 `serde_json::Value` 反射会栈溢出，因此未启用 Specta 的 serde_json feature，报告改为显式 DTO；不依赖 generic JSON 反射。`usize` 被精度策略拒绝，控制 DTO 改为有界 u32、内部乘法用 u64 验证，没有放宽 BigInt 禁令。Serde 的浮点输出可能为 null，生成器如实导出 `number | null`；前端仅在有限数运行时检查后用于算术，不用手写接口掩盖 null。
- 新增 `paste 1.0.15` 维护公告来自 Specta 的函数反射，详见 [依赖审查](./dependency-review.md)。没有添加 ignore 或宏补丁。S21 功能链路的 Linux 证据不代表依赖、三平台或正式采用门禁已通过。

## 本地 PTY 原语与真实交互探针（S3 前置基础）

```sh
mise run probe-pty
# target/probes/pty.json
```

- 新增独立 `pulse-pty`：封装 portable-pty 0.9.0，使用显式 executable / literal argv、cwd、环境覆盖与默认 TERM；提供单 reader、完整写入、字符尺寸读写、非阻塞退出观察、等待、关闭输入、终止并回收直接子进程。不依赖 Tauri、解析器或前端。
- 原语是**阻塞接口**，不隐藏队列或线程，不能直接运行在 UI/async worker 上。已新增pulse-app监督native read/write/control到独立actor的有界桥，但窗口订阅、任意阻塞写入取消、进程树/Job Object回收和平台Shell探测尚未实现。Drop 是直接子进程的尽力清理，不是取消任意阻塞 I/O 的保证；Windows 调用方必须在关闭 ConPTY 期间持续排空 reader。
- Linux 真实 PTY helper 检查 stdin/stdout 为 TTY、literal argv/cwd/env 未变、输入逐字节未变；resize 到 91×37 后既检查 master readback，也由子进程 `stty size` 观测。中文与光标查询进入 Rust `TerminalProbe`，应答 `CSI 1;7 R` 原样写回 PTY，并由子进程核验，最后检查 Rust 屏幕及尾部输出、退出码 7 和 EOF。
- 使用固定 `/bin/sh -c` 脚本作系统 Shell 冒烟：确认 TTY、输出 VT 流、Rust 解析结果和正常退出。不是交互/login Shell、OSC 133、vim/tmux 或 Shell 环境探测的兼容性验收。
- 慢消费者暂停读队列，观测 reader 的 send attempt 停止推进，子进程仍未退出；恢复消费后按顺序逐字节验证 **1 MiB** 确定性输出和结束标记，没有丢弃数据。探针限定 2 个排队块，每块 ≤4 KiB；计入消费者当前块、producer 待发送块与固定 reader scratch，分块暂存上界为 **20 KiB**。这是代码结构的缓冲预算，不是 RSS/整个进程树内存测量，排除 OS buffer、线程栈其余部分、握手捕获及解析器状态。
- 检查关闭输入幂等、单 reader、重复 terminate 的退出状态稳定、直接存活子进程 Drop 后 EOF、所有正常 reader join、不存在可执行文件时 spawn 失败。本机直接终止 + 排空约 50 ms，用 2 秒上限检查；不能外推到其他系统或子孙进程。
- helper 独立 10 秒截止时间，worker 外层 25 秒监督，失败不读取上一轮结果；报告要求完整布尔观察、版本、字节/队列预算、精确应答/退出与取消上限，负向单元测试拒绝仅有 completed 标志等不完整报告。
- 已接入 Linux/macOS CI 的 `probe-pty`，三平台CI新增finite Rust helper和native actor桥测试；本轮仅取得Linux结果。**S3/S10 尚未通过**：随附 ConPTY 加载、win32-input-mode、Win10/11、PowerShell/WSL 及真实 UI 兼容性待验证。

## 原生actor监督与三平台PTY探针

- 新建既定应用组装层 `pulse-app`，setup在blocking pool，常驻reader/writer/controller专用线程；`ChannelTransport::bounded_driver`提供固定8块output/input通道，每块上限64KiB（另有native reader scratch/current块、writer在途、command/reply预算与OS缓冲）。无Mutex<Term>，无Tauri和新增第三方依赖。
- native resize由controller确认后才修改parser尺寸，失败保持原state，超过5秒缺ACK明确Failed并cancel；等待ACK时暂停解析（不停止native reader的有界读取）。旧尺寸排队输出仍可能在新size下reflow，这不是跨窗口attach或resize完全顺序性的验收。
- writer阻塞时controller仍可kill/reap直接child；renderer取消后reader不停止，而是discard/drain直到EOF，避免ConPTY master关闭等待无人排空的output。组合receipt join actor/direct child及三worker；Drop receipt请求close，不能保证进程树或OS-hung syscall的退出。input成功仍只是admission，不是physical write receipt。
- 6项native-driver单测验证成功/失败resize、pending ACK取消、正常EOF vs fatal I/O、32次fail/EOF竞态，以及真实5秒deadline。`probe-local-session`在Linux用raw helper证明输入/query回写/child尺寸观察、exit7/EOF、native actorrestore、direct cancel/join和receipt Drop关actor。Windows使用canonical Rust helper，不冒充raw模式/child尺寸query/Win32键盘覆盖。
- `probe-pty-portable`有限Rust helper不依赖stty，验证真实TTY、argv/cwd/env、Unicode canonical输入、native尺寸回读、exit7/EOF和幂等direct termination；helper15秒截止、外层worker40秒监督，report不复用旧成功。本轮Linux通过，已注册三平台CI；Windows/macOS结果待执行，也未引入微软DLL打包决定。

## 独立会话 actor / EchoTransport（S11 前置基础）

- `pulse-term::session` 由一个 Tokio task 独占解析器/网格，外部只有有界命令和小型 watch 元数据；没有 `Mutex<Term>`、Tauri、窗口或 DOM 依赖。`SessionTask` 提供可监督的 join，终态屏幕仅供诊断，不是恢复快照。
- Echo 驱动的输入/输出队列各 8 块，块上限 64 KiB；命令队列 8 条；协议应答积压载荷上限 256 KiB，另有字符串/队列开销。超量消息拒绝；满队列明确返回 Busy（未接受），不伪装写入成功。调用方已构造的消息、无界并发调用数量、内核缓存及网格不在此预算内。
- 后端查询回复单独写入 upstream，不再当作程序输出回送。输出解析每轮有界，选择循环公平处理控制命令；取消在每轮先检查且走带外 watch，不排在数据后面。正常 EOF 完成 UTF-8 尾部；取消/协议溢出与正常 Ended 分开标记。
- 11 个本地测试覆盖无视图输出、观察者释放、最后控制 handle 放弃、字节 roundtrip、尺寸变化、协议应答、输入/输出超限、满队列关闭、持续输出下 inspect/close、公平调度和最终 EOF 替换字符。合成驱动的结束 join 包括 driver 回收；2 秒测试 deadline 不等于 OS I/O 的退出保证。
- 新增 `SessionHandle::diagnostic_restore`：在 actor 命令序列点返回 `SessionState` 边界（size、revision、parsed_bytes）与恢复字节。`parsed_bytes` 仅是解析进度，不是传输 ACK 或 attach epoch；palette/cell pixel 控制变化也递增 revision。
- 默认序列生成和 actor API 的有效载荷上限 1 MiB，actor 请求可进一步收紧预算（大于上限时收敛到上限）。Emitter 的普通追加、格式化写入与字符写入都在生成期间检查预算，待补充 CSI/UTF-8 尾部计入同一预算；超限返回 OutputTooLarge，不返回截断序列。此预算不覆盖 grid、分配器容量/元数据或调用者保留的多个已完成结果；它也不是执行时间上限。
- actor 诊断 API 显式拒绝已知 approximations，单独返回 RestoreGap，不终止会话；可在程序解除条件后重试。满队列、关闭后的请求不接受，已取消的请求若尚未开始会跳过生成。新增 7 个 actor 测试及 2 个 Emitter/精确尾部预算测试，包括备用屏 + 部分 CSI 续流、主屏退出、零/精确/超大预算、revision、已知近似/非行尾 wrap 的解除和重试。
- 已修复 random-85：xterm 的 scroll margins 是每屏状态，Rust 是全局状态；恢复隐藏主屏时也设置 margins，再保存光标/切屏。新增最小命名用例 `primary-scroll-margins-after-alt-exit`。当前 546 个可编码无已知近似样例：M1 381、M2 301、M3 430，条件 M1|M2 **301/301**。TS 断言现在覆盖所有命名/随机 `strict && M2 && !M1`，不再只报告随机恢复失败。负向探针临时省略隐藏屏margins时，命令退出1，同时列出命名回归和random-85；源码已恢复后重验。条件计数不涵盖原流本身已有跨解析器分歧的样例，也不是总体 S2 通过。
- actor 使用 VTE 的真实同步刷新 deadline，在没有新输出时也会主动排空缓冲，经同一 tracked handler 更新状态与 revision；parsed_bytes 保持原始接收/解析进度，不制造额外输出字节。正常 ESU 或超时会解除 SyncPending。deadline遵循VTE，远端重复BSU会延期，未引入独立总时长限制。EOF 也排空未关闭同步刷新和不完整UTF-8；存在不能送达的上游回复则标记Failed，不伪装Ended。
- 批量 flush 内的颜色 query/set/reset/RIS 经 handler 捕获查询当时的override，避免较早查询使用后来的动态颜色；RIS遵循上游保留色覆盖，OSC110才明确重置。6 个回归覆盖 idle超时、deadline前不执行/只执行一次、上游应答、EOF尾部与未应答失败、正常ESU和EOF颜色顺序。
- 扩展30,000组 release 属性测试无失败：日志 exact=18,298、deferred=8,377、approx=2,459、alt-active=4,606（精确续流检查4,018）、alt-reentry=3,967；日志桶不是互斥的全部三万分类，延期/近似不是精确通过。生产 attach、历史/reflow、跨窗口和协议额度仍未验收。
- `pulse-app` 已接入有界真实 PTY async bridge（见 S3/S10 一节），原生 resize 收到成功回执后才更新 parser；IO错误明确 Failed，resize回执超时5秒失败且可带外取消。Echo只在validation构建可用；input成功仍仅代表队列接受，不代表物理写完。传输额度已在pulse-app窗口无关driver贯通；Registry、多视图关联、落后快照策略、生产生命周期事件节流和OS进程树回收仍待实施。
- S11 仍未整体验收：没有实际窗口迁移或 UI 销毁/恢复结果，视口投影禁止充当 attach 快照。此实现只建立与视图无关的基础，不宣称已经实现完整会话恢复。

## 真实终端软件录制及恢复回归

`mise run probe-software` 启动 vim/less/htop/tmux 原生PTY：每个程序记录一个键盘输入和一次80×24→91×37 resize边界，共8个受控样例；Rust严格比較restore前后完整已承诺结构及续流，真实headless xterm比较M1/M2/M3与query抑制。本轮8/8均匹配后端。不是WebView/窗口验证，也不是重新同步频率的代表性统计；风险位可有false positives（样例都有MODE/CELLS/GEOMETRY提示）。原.vt只作来源记录，尺寸转换需要software.json。

- 新发现并修复：alacritty的ED2会把当前主屏推入history，即使主屏空白。旧恢复在vim备用屏重建时制造1行空history，后续扩大窗口会把隐藏current/saved cursor都下移1行；现在恢复用逐行EL，不使用ED2。2项回归验证fresh history为0、grow结构一致、live恢复保持原history每格及wrap。
- 再验三万组release属性无失败：exact=18,602、deferred=8,518、approx=2,048、alt-active=4,538（检查4,395）、alt-reentry=4,073；桶非互斥完整分类。当前Rust/xterm矩阵557个可编码无已知近似样例，M1=391、M2=311、M3=442，条件M1|M2=311/311；基线解析漂移和不可编码案例没有算成功。
- 采样每程序日志≤2MiB、reader仅2块4096字节，父进程40秒deadline；拒绝不能idle或正常quit的程序，不静默跳过。htop只观察采样进程本身；tmux只用独立socket和12秒有限shell，受控kill后client退出1是明确记录的server-exited，其他程序需0。故障保存prefix/suffix/恢复帧与结构，旧成功报告启动前删除。
- Linux CI安装这4个工具并注册探针，只上传JSON，不上传原.vt；测试样例不含用户session内容。原依赖门禁不变。当前仍需原子live订阅/流控/窗口绑定、自动idle同步限频、深历史和reflow UX及Windows实际执行。

## 原子bootstrap/live订阅与Checkpoint

- 新增公开 SessionStream/StreamRecord/StreamStatus 与 diagnostic_subscribe：同actor回合建立history/screen边界和唯一bounded live lease；full先停止解析，再沿transport/native队列回传背压。drop订阅恢复无视图处理，不关闭session。成功替换旧lease，cancel/错误capture保留旧lease；id不是授权/global generation。
- 9项单测覆盖partial UTF-8/备用屏续流、full下控制close/detach、取消/失败capture和替换、pending/native resize、EOF替换字符及sync Fence。控制配置/成功resize结束旧geometry lease，driver须建新generation。
- wire Checkpoint=6携带u64 revision，offset为当前live cursor，同32位sequence和发送额度。Receiver拒绝旧revision、错误cursor/phase/长度；sink处理完成才ACK，缺实现取消。新增4项TS及1项Rust验证精度、callback及phase/geometry/credits。
- probe-local-session真实2MiB输出在Linux填满8条lease后，80ms停读parsed_bytes不前进，inspect仍可用；原子重新capture后旧leaseReplaced，连续offset重放全部边界后流。本轮paused=29,743、最终parsed=2,097,221、live=508块（OS分块会变化），最终marker/Ended/direct child join通过。不是RSS/进程树、网络ACK或窗口验收。
- probe-attachment第四个案例：actor原子lease→binary→TS/headless，在部分UTF-8和备用屏后续流，sync Fence映射Checkpoint，sink严格比较后端屏幕后ACK。check/77项term/13项IPC/57项TS、API隔离及vendor211文件门禁通过；Astrolabe Rust无错误，无依赖变化。
- 已新增 `pulse-app::attachment` 的窗口无关wire/ACK监督：config先验、bootstrap转移所有权、固定frame/byte预算、oldest ACK deadline、正常End与callback契约，取消/失效只释放lease不关闭session。7项测试覆盖零/不合理config保留旧lease、2.5MiB输出无ACK冻结→timeout恢复、EOF截断UTF-8、正常final ACK、End缺ACK不能完成、wrong generation/duplicate ACK、receiver drop、reconfigure与旧binding替换；TS3项覆盖End渲染后ACK/缺finish拒绝/取消无迟到ACK。
- 新probe-local-session wire测量真实2MiB helper→actor→AttachmentSender；1帧、8KiB分片、9000bytes credit，暂停ACK时parsed_bytes冻结且inspect可用，最终2,097,221字节/Ended及child join通过。一轮521帧、credit峰值6,587 bytes、暂停边界24,623（OS分块会变化，不能固定该计数）。ACK来自Rust validator不是前端callback。第五个probe-attachment案例把driver输出回放到headless，End后decoder生成一个替换字符并匹配后端。native压力与headless callback分别实测，不冒充实时WebView/网络往返。
- CI沿用三平台probe-local-session/probe-attachment任务，已包含新路径；本轮Linux执行。77项term、14项IPC、7项app、60项TS及API隔离/check通过。Cargo.lock只增加pulse-app→pulse-ipc内部edge，无第三方版本/许可证/补丁变化。
- 仍非production attach：一个诊断订阅、未覆盖非UTF-8双坐标和任意用户会话。Tauri窗口/命令授权与真实WebView callback ACK→PTY已有下文组合证据；生产旧surface管理、自动idle同步限频和多视图策略仍待实现，不代表三平台/S2/S11整体通过。

## 应用注册层授权与撤销（后端基础）

- 新增 `pulse-app::registry`：32个可信window incarnation、64个session、64个监督task上限及per-session capture try-lock；generation backend单调分配，id耗尽/容量满拒绝且不替换当前binding。元数据短锁不跨await，capture future编译期验证Send。
- 13项新增测试涵盖同actor重复注册/替换watch不能绕过owner、外来window/跨registry token权限、unknown session统一拒绝、旧ACK/detach/晚完成不能影响替换、failed/busy/取消前capture保留旧view、窗口关闭时并发capture不复活、同label重建不继承授权、EOF final ACK清理、应用Drop关闭session、window/session/task容量及ID耗尽。原7项driver测试同样通过。
- probe-local-session的2MiB native wire流现在通过registry注册/input/ACK与监督completion；Linux实际helper连续offset、EOF/child join、暂停ACK时parsed_bytes冻结和inspect通过。测试使用可信in-process WindowToken，不能视为Tauri command授权/WebView窗口事件验收。CI现有三平台任务自动包含此路径，Windows本轮未实测。
- window关闭撤销view但保留application-owned session；新window不会凭相同label继承访问权。跨window迁移/重新授权尚未实现；注册者必须监督native receipt，shutdown不是process-tree join。无第三方依赖、锁文件或parser补丁变更，发布门禁不变。

## Tauri窗口授权与真实PTY/Channel组合（Linux）

- `mise run probe-channel`已升级schema 3：在真实WebKitGTK WebView内连续运行两次native PTY→actor原子lease→registry→Raw Channel→AttachmentConsumer→headless xterm callback→生成ACK命令→backend credit→ordered End链路。四个新增命令由同一Specta表注册/生成；u64控制整数为canonical十进制string，数据始终ArrayBuffer。
- 固定helper接受唯一`go`输入，输出Unicode、颜色和截断UTF-8后正常退出；客户端在一个live ACK延迟250ms，driver最多1帧/8212 envelope/body credit。一次第二轮实际parsedBytes=3,112,985、frames=780、peakCreditBytes=6092、generation=2、exit=0，前后端最终visible digest/输出边界一致。数字为单次测量，不作为性能门槛。
- 创建实际隐藏peer WebView，由runtime关联window token；跨owner input/ACK拒绝。实际Destroyed撤销窗口权限，原native actor仍Running，经显式close/join后回收。相同label连续重建无权限继承；第二轮实际拒绝旧nonce、旧generation及错误generation ACK。origin限制只接受无port/credential的本地tauri host，外部/伪造host拒绝。
- 新增可信registrar `reap_closed`：窗口销毁后只清已结束actor，不可清Running session；70轮关闭/回收单元测试验证window/session容量不累积。失败路径guard关闭peer/helper，有界5秒等待终态回收，child自带40秒硬截止；未知或逾时的join不算完成。
- 组合测试发现EOF后actor已不接受diagnostic_digest命令。新增`LocalSessionTask::join_with_screen`从actor终态receipt取visible projection后一次计算摘要，与driver End边界比较；该screen不是完整snapshot，不改变SessionState或二进制协议。
- native控制器原先优先检查stop，actor EOF与已退出child同时就绪时可能错误标记cancelled；现先try_wait，再决定是否终止仍存活child。确定性回归保留已退出child的零/非零状态，stop仍终止存活child，native I/O失败仍按失败处理。修改后3个独立WebView进程全部通过，各执行两次窗口重建；少量重复运行不代表长期稳定性证明。
- 原7组Channel/WebSocket完整性、慢消费者credit和取消恢复测量仍全部通过。11项native adapter测试、24项pulse-app测试、77项pulse-term测试、14项pulse-ipc测试、60项TS测试、clippy/tsc/Biome与bindings drift通过。Astrolabe用于调用范围，native diagnostics请求超时，不能视为LSP通过。
- 限制：浏览器内使用headless xterm callback，不测DOM绘制、IME、真实用户shell、生产view重建与跨窗口迁移。自动idle/resize重同步、多视图、深历史与Windows/macOS实际组合仍未验收。Cargo.lock只增加现有workspace crate的桌面依赖边，未升第三方版本、改变补丁或放宽审计。

## 绑定摘要比较与有限idle检查（Linux）

- 新增`SessionRegistry::compare_visible`与`RenderedScreen`/`VisibleComparison`：先验证当前window owner和generation，再进行只读actor摘要捕获，返回前重查授权。每session单请求、500ms最小间隔、250ms截止；取消和替换不绕过限频。当前actor状态或捕获边界变化返回Stale；Mismatch和Equal都不清risk。7项Rust回归覆盖一致/差异、不合法输入、foreign/旧generation、输出推进、替换/撤销、取消和single-flight。
- `IdleComparison`以600ms空闲、350ms证据期限、最多3次尝试调度；新输出入队、隐藏、替换或销毁拒绝迟到结果。未完成IPC仍占single-flight，失败不变成分歧。8项TS受控时钟测试验证有限重试、限频、隐藏、Data边界失效和取消。Commit/Checkpoint（及下一节新增Marker）提供revision；普通Data后必须等待新的有序检查点，不能猜测revision。
- 新生成`probe_session_compare`命令使用canonical十进制u64 counter（允许0）和显式小DTO，数据仍Raw Channel；generation必须非0。真实WebKitGTK完整probe连续两轮：错误/旧generation拒绝，人工改变摘要返回Mismatch，紧接重复请求因限频拒绝，600ms后真实headless回调摘要Equal。`channel.json`要求`session.idleChecked=true`，原7组运输与EOF校验同时通过。
- 此阶段仅受控Commit idle验证；后续Data空闲Marker见下一节。仍不自动重绘，同generation/boundary原子恢复授权及生产DOM生命周期待接入；Equal不证明hidden/history/parser一致。未更改第三方依赖、alacritty补丁或审计门禁。

## 普通Data后的有序只读Marker（Linux）

- v1新增Part::Marker=8，与Data/Fence/End使用同一CreditWindow和ordered binary Channel；body是LE u64 revision，offset是当前原始parsed_bytes，未新增旁路事件或JSON终端数据。Marker不同于同步Fence，不写2026l、不finish解码器或更改backend revision，不能打断半条CSI/UTF8。
- actor `diagnostic_marker(lease_id)`只向当前lease的有界队列try-reserve；满队列返回Busy、旧/已detach lease返回StaleLease；处理前取消不注入Marker。active BSU/pending resize明确Busy，不提前flush/resize。`AttachmentHandle::request_marker`/`SessionRegistry::request_marker`贯通，owner/current generation前后校验；请求每session 500ms限频、single-flight和250ms截止，换绑/取消不重置频率。IPC成功仅证明队列admission，不能拿它当边界或ACK。
- `IdleComparison`对新Data入队先作废旧证据，600ms空闲请求Marker（最多3次），等待ordered Marker callback且没有后续Data入队才观察精确边界，再按原调度比较。Consumer必须显式支持只读marker callback且完成后才ACK；不支持、取消、迟到回调均不能释放信用。marker可早于IPC返回，仍不并发发起第二请求。
- 真实WebKitGTK连续两轮native helper输出约3.1MiB后停在半条CSI，前端自动Marker→callback/ACK→摘要Equal，再发送固定m输入续流，最终EOF/边界/digest一致。PTY本身回显m输入也属于原始输出，未过滤或伪造流量。实测第二轮parsedBytes=3,113,003、generation=2、frames=777、peakCreditBytes=8212；schema3要求`markerChecked=true`，数字非性能门槛。peer实际拒绝Marker权限，原7组transport、两次窗口重建和EOF证据同时通过。5个headless attachment场景、真实2MiB native registry/wire探针、native clippy/test与binding drift也通过。
- 此阶段未实现自动重绘和同generation/boundary原子compare→restore；下一节补受控精确修复。生产DOM/user shell、多视图或Windows/macOS实际验收仍未完成。连续BSU、持续新输出或队列满可以延期/拒绝，不承诺总会得到可重绘边界。未修改第三方依赖、vendor补丁或审计门禁。

## 原子精确屏幕修复事务与受控自动重绘（Linux）

- actor `diagnostic_repair`同一回合核对lease、取消信号、全部输出/尺寸revision及digest，Equal/Stale不排帧；Mismatch才尝试精确恢复。gap/approximation、满队列、pending resize或取消明确拒绝，风险标记不清除。`SessionRegistry::request_repair`验证window owner/current generation，single-flight、500ms限频与250ms截止；返回后重新校验generation，换绑不能让旧恢复进入新绑定。
- 新Part9/10/11分段修复与Data共用Channel和CreditWindow：最多1MiB暂存，Commit前无部分渲染，期间不交错Data/End；修复不推进原始parsed_bytes。单份repair permit从actor捕获持有到最终Commit ACK；未ACK时后续Data不会发出，第二次修复明确Busy。receiver/sink不支持、错offset、取消和中断事务fail closed。
- 专用非RIS同步序列CAN→2026h→screen restore→2026l→pending CSI/UTF8，结束同步不能放到待续parser尾部后。sink重置decoder而非terminal，保留scrollback。新增3项core回归含半CSI续流、完整UTF8中、history颜色/cells、旧lease/取消、nonedge wrap拒绝和active BSU拒绝；新增driver/registry两项授权/晚完成/ACK限流回归，1项IPC大整数/NoCredit/section offset回归，3项TS Commit/ACK/unsupported/取消/中断回归。
- 真实WebKitGTK两轮受控自动修复：fixed helper在半条CSI暂停，Marker callback中故意修改xterm可见内容及cursor；idle检测真实Mismatch，重新捕获同边界精确Repair，Commit后再次摘要Equal再续流。修复前后history文本完全相同，最终EOF/native exit0/digest及原始字节边界一致，peer修复权限拒绝；schema3要求`repairChecked=true`。实测第二轮parsedBytes=3,113,003、frames=783、generation=2、peakCreditBytes=8212；原7组运输检查同时通过，数字不作性能门槛。
- `mise run probe-snapshot`默认600个随机样例及既有完整续流回归通过；条件xterm矩阵311/311（只包含原输入解析能与Rust一致的案例），分歧矩阵仍18/372，11个可精确修复、7个明确不可编码/延期且全部风险标记。不是重跑30,000样例或完整S2验收。5个binary/headless场景及2MiB真实native授权/ACK/backpressure探针也通过。
- 自动修复在此probe每绑定最多一次，不能因仍Mismatch循环重绘；持续输出/Stale/近似状态的生产重试UX与visible-view全局调度仍待接入。当前只验证真实WebView里的headless callback，不是DOM绘制/IME/selection。未宣称S2/S11、history全部恢复或Windows/macOS验收；vendor/依赖锁和发布门禁未变。

## 可见DOM终端组合验证（Linux）

- `probe-channel` schema4保留两轮headless基线，新增两轮可见`@xterm/xterm` 6.0.0（MIT）DOM renderer；固定PTY 40×12、128行history。同一session probe/consumer走窗口owner/generation授权、Raw Channel、250ms延迟ACK、空闲Marker、半CSI显示损坏→精确Repair→摘要Equal→续流/EOF。每绑定只修复一次，不开放任意shell、键盘/粘贴输入或外部链接。React只注册host，输出及decoder留在imperative controller。
- DOM sink先等待xterm write callback，再等两次animation机会（1秒有限截止）；隐藏、卸载或取消不绕过等待、不返ACK。最终DOM renderer `.xterm-rows`的12行文本与同viewport buffer逐行相同，屏幕位于可见viewport、几何非零、onRender事件及frame fence非零才算完成；不是GPU/compositor回执，也不是input-to-paint时延。screenReaderMode开启只说明选项接入，未测真实读屏器。
- 严格CSP阻止xterm动态stylesheet曾造成文字不可读；已用本地HTML空style anchor的Tauri每页nonce与公开documentOverride隔离样式创建，确保nonce先于CSS文本/插入。全局document不改、只授nonce给renderer style；缺nonce或未替换token明确拒绝。不加unsafe-inline、不改tauri.conf.json、不引入xterm fork。原生窗口截图已读回确认文字/中文/emoji可见及布局不溢出；开发界面沿用现有Radix tokens，机械检测零发现。
- 初轮再次出现真实EOF早于child.try_wait收据被标为native_cancelled。reader只在成功read0记录EOF；controller只在真实EOF且无I/O failure时提供单次100ms退出发布宽限，截止不续期（正常20ms监督采样），非EOF stop及I/O error仍立即terminate，过期继续终止。不把cancelled+exit0当成功。新增回归覆盖99/100ms界限、非EOF/失败/未stop、重复调用不延长期限；真实2MiB PTY→registry/ACK压力、resize、EOF及取消仍通过。
- 两次成功的独立Linux运行各含2轮headless+2轮DOM及原7组transport/cancel测量，最后`generation=4`、`parsedBytes=3,113,003`、`frames=780`、`peakCreditBytes=8212`、digest=`b17f4532b0d32770`；DOM `renderEvents=776`、`frameFences=776`、`rowsChecked=12`、320×192 CSS px。时间/帧数是单次证据，不是吞吐或延迟门槛。此前一次报告因错误地把固定样例行数写为8而被拒绝；已改为真实12并保持逐行全量验证。前端70秒会话deadline保留首个renderer错误，后续取消不能覆盖根因；后端仍有既有60秒relay/10秒ACK/helper40秒限制。
- 完整check/test、native clippy -D warnings、12项native测试及bindings-check通过；85项TS含9项新增fence/nonce回归。`pnpm audit`无已知漏洞；lock仅新增@xterm/xterm 6.0.0单包。未更新Cargo.lock/vendor/CSP策略或依赖豁免。生产DOM交互/用户shell、ResizeObserver/fit、IME/selection、全局resync/隐藏暂停UX以及Windows/macOS仍未验收；不宣称S2/S11通过。

## 有界generation输入与DOM键盘回写（Linux）

- 新增`SessionHandle::input_for_lease`、`AttachmentHandle::input`与`SessionRegistry::input_bound`：runtime owner/current generation检查后，actor处理回合核对当前lease、取消watch、subscriber仍在及reply未abandon。pending resize/replies或满transport队列拒绝，旧lease不能把晚键盘输入发送到新view。返回前再次核对generation；准入后的input无法撤回，成功不是native write收据。常规后端`input`仍用于非viewer可信控制（固定go/resume等），未错误声称它已经generation-bound。
- `InputQueue`按UTF8实际字节计费：单条≤1024B、活动+排队≤8192B/32项、一个IPC。空/孤立surrogate/单条超限不调用native；overload/取消/错误作废所有pending，一秒截止视为uncertain，不能自动retry或在late success后继续drain。取消/错误向后端请求generation-scoped detach，旧cleanup不能撤销新绑定；detach本身不关闭会话，开发finite helper仍由自身supervisor负责资源回收。
- 固定helper在半CSI修复后显示INPUT_READY，等待精确`typed中文🌍`一行再显示INPUT_OK、EOF及截断UTF8尾部。有限`probe_session_input`只允许该固定字符集与CR、≤1024 UTF8 bytes，并拒绝LF/ESC/其他字符；不是任意shell/input端点。peer窗口跨owner、wrong/old generation及非法LF均在真实WebView拒绝。
- 两轮headless通过同队列但不宣称键盘；两轮可见DOM临时启用disableStdin=false，textarea合成keydown/keypress/keyup输入ASCII `typed`、composition提交`中文🌍`及Enter，经xterm公共onData→InputQueue→typed invoke→actor→real PTY。onData序列必须精确为`typed中文🌍\r`（漏字/重复/查询泄漏失败），helper回显INPUT_OK且最终digest/EOF相同。DOM最终仍只读；capture paste listener须阻止合成paste且不产生onData。screenReaderMode保持开启，但合成composition不是OS IME输入/候选框/无障碍读屏器验收。
- schema4新增session.inputChecked及dom.keyboardChecked/compositionChecked/pasteBlocked门禁。两次独立完整Linux运行成功（每次2headless+2DOM+原7运输/取消场景）；最后generation=4、parsedBytes=3,113,061、frames=789、peakCreditBytes=8212、digest=`2ba9204154c3b86c`，DOM 12行文本一致、320×192、renderEvents=787/frameFences=785。数值为证据，不作性能承诺。
- 2项actor回归、1项registry回归、7项TS队列回归及1项native输入validator回归新增；全workspace check/test、native clippy -D warnings、13项native tests、bindings-check、2MiB真实授权/ACK压力通过。没有升级依赖、变更锁文件/CSP/vendor或豁免门禁。生产输入快捷键/广播/粘贴确认/fit/ResizeObserver、真实OS键盘与IME、Windows/macOS仍待验收。

## 自动fit、原生resize回执与新generation恢复（Linux）

- `resize_for_lease`/AttachmentHandle.resize/SessionRegistry.resize_bound在owner+current generation入口及actor回合验证lease/取消watch/调用reply；same-size不发native、不推进revision、不撤销lease，pending resize明确Busy。native ACK成功才更新grid并发Reconfigured；失败保旧grid/lease，既有5秒未ACK失败关闭会话语义保持。准入之后native不可回滚；不能因不确定结果自动retry。resize成功必终止旧binding，registry返回时只重检owner而不误要求旧generation仍存在。
- `measuredGrid`按实际DOM cell大小与host content box去padding计算整数grid：hidden/0/负/nonfinite/小于2×2不发native，最多160×60。FitScheduler 50ms合并observer风暴，单IPC、6秒不确定截止（native最长5秒），错误/取消不optimistic改renderer、不重试、不允许late结果重绑。9项TS回归覆盖最后观测、等待ACK、无效直接geometry、no-op、hidden、取消和deadline；2项core native-ACK/取消/abandon/失败回归及1项registry跨owner/旧控制/同session新generation回归通过。
- fixed helper在go前输出已解析`RESIZE_SEED 中文🌍`。两轮DOM初始Commit触发真实ResizeObserver适配40×12→104×12；native ACK后relay只对一次Reconfigured重新capture，同session的新Begin使用新consumer/decoder/DOM renderer，按输出顺序恢复seed和精确尺寸，再执行原query/Marker/Repair/keyboard/EOF门禁。禁止旧ACK/input/resize/detach，新receipt回调校验自身generation。headless两轮无resize；report schema4要求DOM fitChecked及resizeCount=1，并与NativeExit真实resize_count对照，最终columns/rows从actor终态提供，不只相信frontend标志。
- 综合检查意外发现已有capture发布竞态：actor先oneshot.send新stream，旧lease.status稍后才Replaced，多线程下caller可在间隙醒来。修复增加私有InstalledStream commit oneshot，public diagnostic_subscribe必须等待actor撤销旧lease/安装新lease后才返回。捕获前取消保旧lease；send之后/安装后的取消仍已换绑，不承诺rollback。新增确定性回归人为延迟commit10ms确认API仍pending；原2MiB native冻结/重新捕获的Replaced断言保持不变并通过，不放宽门禁。
- 三次独立完整Linux WebView运行成功（最后一次包含commit gate，每次2headless+2DOM+7运输/取消场景）；最后DOM generation=6、104×12、832×192 CSS px、parsedBytes=3,113,086、frames=783、peakCreditBytes=8212、digest=`00b088cbb609382c`、renderEvents=780/frameFences=779，12行DOM/buffer逐行一致。截图已读回确认auto-fit可见、不溢出；数字为单次证据，不承诺性能。完整check/test、89 core/38 app/16 IPC/13 native/101 TS、bindings-check、native clippy -D warnings及真实2MiB native probe通过。
- 这里只验一次初始列宽适配（容器高度当前由固定12行决定），不宣称连续拖动/ResizeObserver持续换绑、独立布局高度改变、跨屏DPI/zoom或生产TermView/IME完成；正式策略应新generation重新测量、合并最新geometry，并保留精确恢复拒绝/延期UX。没有新增依赖、改锁文件/CSP/vendor或放宽发布门禁。

## 多阶段observer缩放、容器隐藏恢复与迟到view隔离（Linux）

- fixed DOM每新generation重装ResizeObserver/FitScheduler，容器独立208px高度不再从旧terminal行数倒推。初始40×12→104×12；连续写入3个width/height样例（464×192、480×184、496×176）由observer/50ms合并后只提交一次60×10；恢复自然宽+208px后第三次提交104×12。native ACK后精确capture、旧surface销毁与decoder重建，seed/尺寸必须相同；finite relay cap3，native完成的resize_count须等于重捕获次数，普通错误不得重试。
- 新增显式owner/nonce/generation授权的pause/resume控制：VisibilityGate先在短元数据锁内完成registry.detach，再发布pause票据；driver Detached只能对同一票据、至多一次、5秒内显式resume重新capture。暂停不关闭registry session，Rust继续处理输出；无票据Detached、旧/重复resume、wrong generation或owner撤销不能授权新view。watch先注册后查票据，早到resume不会丢失；3项native回归覆盖未pause/准入失败、generation/单次消费及显式等待。没有持有terminal grid锁或引入轮询native控制。
- 每轮DOM第三次resize后显式pause、隐藏host、销毁旧consumer/renderer；固定helper接受trusted内部hidden控制并输出HIDDEN_SEED，后台parsed_bytes继续增加且Lifecycle=Running。100ms期间Raw Channel帧数不增加、旧input/ACK拒绝；重新显示host并显式resume，以新generation恢复RESIZE_SEED/HIDDEN_SEED及对应parsed boundary，然后继续原query/Marker/Repair/键盘/EOF验证。隐藏恢复是受控DOM容器，不是OS窗口minimize/hide或真实document visibility事件。当前hidden-output证明请求最多等2秒，不开放用户命令。
- `SurfaceGeneration`要求requested replacement后才接受更大Begin，保留U64精度；已撤销generation帧不触发idle活动或ACK。AttachmentConsumer旧Promise完成/错误按consumer identity过滤，late callback错误也不取消新surface。共享attachmentEnvelope负责bounded length/version/flags/zero generation，不复制codec；8项TS回归覆盖无Begin/重复/未请求替换、连续resize/resume、stale frames、late success/error及64位边界。
- 两次独立完整Linux运行成功（最后含新router/late callback guard，每次2headless+2DOM+原7transport/cancel）；DOM最后generation=12、3次native resize、1次pause/resume、104×12、parsedBytes=3,113,119、frames=794、peakCreditBytes=4115、digest=`00b088cbb609382c`，DOM 12行buffer投影一致、832×192、renderEvents=791/frameFences=790。report schema4要求visibilityChecked、resizeCount=3/pauseCount=1；headless计数均0。数值只是单次证据，未承诺性能。
- 完整check/test、89 core/38 app/16 IPC/16 native/109 TS、native clippy -D warnings、bindings drift、原2MiB native压力及5个跨语言binary/headless场景通过，机械UI检测零发现。没有新增依赖/锁文件/vendor/CSP或ignore/许可放宽。尚未验收真实OS窗口拖动/minimize、跨屏DPI/zoom、长期隐藏/恢复失败UX、生产ViewHost/用户shell与Windows/macOS；精确恢复拒绝/延期仍是硬门禁。

## 真实Tauri native hide/show与Destroyed暂停撤销（Linux）

- 新增finite `probe_session_window_cycle`控制，仅owner/nonce/当前paused generation可单次调用；wrong/duplicate cycle拒绝。Rust先回读window可见且非minimized，再hide、有限2秒内回读is_visible=false；hidden_output在native隐藏期间注入固定helper控制、parsed_bytes推进且session仍Running，后台等100ms后show、有限2秒内回读is_visible=true且is_minimized=false，然后标记native收据。窗口恢复由Rust监督，不依赖隐藏期间JS/rAF/document.visibilityState的执行。resume在native_inflight中拒绝，show回读完成后才允许新generation捕获。
- NativeVisibilityGuard在错误/取消路径仅对仍匹配caller WindowToken的同runtime窗口尝试show；不会重建已销毁窗口、自动resume或关闭PTY。native操作有不确定副作用，不能盲重试。finite gate仍最多一次pause、5秒监督，失败会由probe guard最终清理样例，非长期生产隐藏策略。报告schema4新增nativeWindowChecked，由后台gate而非frontend填写，DOM必须true、headless false；checked=true与pauseCount=0冲突被拒绝，generated命令/DTO与bindings drift已注册。
- Destroyed回调撤销窗口时立即关闭当前owner gate、清除票据并watch通知；等待者不再要等5秒。closed拒绝所有后续pause/resume/native finish；CloseRequested可被应用阻止、Focused(false)只是失焦，不做同等处理。锁poison也要发布closed、唤醒watch并撤销registry owner。5项新增native回归覆盖已poll waiter、关闭早于wait/晚于early resume、native在途destroy、show收据前禁止resume及metadata panic；报告fixture增加计数/布尔一致性回归。
- 一次完整Linux WebView运行成功，内含2headless+2DOM及7transport/cancel。每DOM有3native resize、1pause/native hide-show/resume，最终generation=12、104×12、parsedBytes=3,113,119、frames=786、peakCreditBytes=4115、nativeWindowChecked=true、digest=`00b088cbb609382c`；12行DOM投影一致，832×192px、renderEvents=783/frameFences=782。hidden期间Channel无新帧，重绑后包含HIDDEN_SEED，并继续输入、repair、EOF。完整check/test、21native/109TS、native clippy -D warnings、bindings-check及UI机械检测通过；最后仅增强close-poison/报告冲突负向门禁，不重复已通过的native成功链。
- 环境Xvfb没有WM（openbox/xfwm4/fluxbox均不可用），因此未声称最小化通过；native.hide/show不是minimize，也不是实际用户触发的document.visibility事件。生产自动可见性adapter、OS最小化/恢复、长时间隐藏失败UX、跨屏DPI和Windows/macOS仍待实测。未增加依赖、修改锁文件/CSP/vendor或放宽release gate。

## 自动容器/页面可见性合并与单generation恢复（Linux）

- 新增可复用SurfaceVisibility（不绑定React）与bindSurfaceVisibility；每generation一个控制器，container/document两来源AND；页面初测/事件测量原子提交两值，不能在容器show、document仍hidden之间制造短暂resume。focus不是来源。host.hidden变更由真实MutationObserver驱动，visibilitychange读取真实document.visibilityState，不篡改浏览器状态；任意ancestor CSS遮挡/DOM解绑并非自动覆盖，解绑必须由ViewHost AbortSignal退出。
- 收到不可见通知同步取消input/idle/consumer/renderer，再异步pause IPC；即使show提前到达，也等detach成功收据再resume。phase只允许visible→pausing→paused→resuming→handoff；每个控制IPC1.5秒不确定截止，错误/timeout/取消不重试，已准入副作用不可回滚。resume期间又隐藏会fail closed，不拿旧generation发送第二次pause。恢复callback在IPC前只授权一次新Begin，准确恢复门禁不变，controller在handoff后忽略旧来源/结果，新generation负责之后生命周期。
- fixed DOM探针移除直接pause/resume调用：绑定controller，设置实际host.hidden=true，等待controller.paused；native backend执行窗口hide/隐藏期输出/show后，host.hidden=false让AND测量自动resume。observer在handoff/失败/abort时断开并移除visibilitychange监听；React宿主传入的dom.signal通过lifetime.abort立即取消控制器、renderer及generation detach，而不是等observer猜测卸载。
- 17项新增TS回归覆盖同步retire/可见no-op、AND、early show必须等pause、来源风暴single-flight、handoff旧通知无效、调用前取消无IPC、在途取消/late结果不动新view、pause/resume错误和1.5秒timeout不重试、resume在途hide、retire失败、already-aborted、真实事件订阅接口与监听清理，以及原子snapshot切换。126项TS、全项目check/test（89core/38app/16IPC等原门禁）与tsc/Biome/Vite通过。本轮不改Rust/DTO/依赖/CSP/vendor，原21native测试证据仍来自前一轮。
- 两次独立完整Linux WebView通过（最后含原子snapshot和自动监听清理，各2headless+2DOM+7transport/cancel），最终DOM generation=12、3resize/1pause/nativeWindowChecked=true、104×12、parsedBytes=3,113,119、frames=787、peakCreditBytes=8212、digest=`00b088cbb609382c`、renderEvents=785/frameFences=783、12行DOM匹配。期间无旧Channel帧，恢复seed/输入回显/repair/EOF均通过。native shutdown出现GLib-GIO name释放连接已关闭warning，不影响完整report/退出收据，未把它隐藏或作为失败重跑。
- 浏览器的visibilitychange来源合并在单元模拟源中验证；真实WebView证据是host属性observer自动pause/resume和native hide/show，不声称浏览器必在OS最小化时发事件。系统minimize、应用多pane ViewHost、跨屏DPI/IME/Windows/macOS及长期hidden策略仍未完成；release gate未放宽。

## Linux X11/Openbox真实native最小化与恢复（受控样例）

- 新增`mise run probe-window`→cargo xtask probe-window→新建私有Xvfb/DBus/Openbox（即使已有DISPLAY也隔离），CLI --probe-window选择backend模式，JS不能选择minimize。普通probe-channel保持原无WM路径与报告channel.json；最小化单独生成window-minimize.json/WM log。Openbox/xprop属于验证环境系统前置，不引入Cargo/pnpm或分发依赖；Linux CI安装并新增独立执行步骤/证据上传，注册CI不当作远端已运行证明。
- native cycle准入/owner/generation/pause票据不变：minimize→最长2秒确认runtime is_minimized=true→后台产生固定HIDDEN_SEED、parsed_bytes推进且Running→100ms→unminimize/show→最长2秒确认visible&&!minimized→finish native receipt。controller之后AND确认容器/页面可见、授权新Begin并精确恢复；不是CSS隐藏或失焦替代。guard失败/取消仅对同WindowToken窗口尝试unminimize/show，不自动resume/新建窗口/盲重试。
- 新增后台DTO nativeMinimizeChecked，必须与nativeWindowChecked/pauseCount一致；server与xtask各自核对mode：minimize DOM=true、headless=false，普通channel二者false。2项native测试覆盖mode错配/缺失/headless claim与全部visible/minimized布尔组合；SessionEvidence fixture拒绝不具备window收据的minimize true。xtask新增2项验证缺失/字符串/数字不是bool收据，现23native/4xtask/126TS通过；bindings generation/drift、native clippy -D warnings及完整check/test通过。
- wrapper等待EWMH支持窗口属性最多50×100ms，owned WM失败立即退出、未ready明确deadline失败；EXIT/INT/TERM清理自己启动的Openbox，外层180秒timeout覆盖完整进程组。直接负向probe用stub openbox退出17确认wrapper失败1而非fallback；用stub xprop使ready永远失败确认5秒timeout；真WM下app退出7保留7；三种退出后pgrep无遗留Openbox。script sh -n及git diff --check通过。
- 一次独立带WM完整运行成功（内含2headless+2DOM+7transport/cancel）；nativeMinimizeChecked=true、generation=12、3resize/1pause、104×12、parsedBytes=3,113,119、frames=792、peakCreditBytes=8212、nativeExit=0、digest=`00b088cbb609382c`；DOM12行匹配、832×192、renderEvents=790/frameFences=788。最小化期间无旧Channel帧，恢复HIDDEN_SEED，后续输入/repair/EOF全通过。最后普通无WM Channel另一次完整运行也通过，其nativeMinimizeChecked=false、headless=false，后续check/test通过；没有重复已通过的WM正向样例。
- Openbox启动log有系统菜单文件缺失message，minimize运行退出时有X授权释放warning；完整runtime状态回读/report/exit均成功且WM无泄漏，没有放宽WebKit sandbox或忽略门禁。数字只是单次受控证据，不承诺性能。尚未验收用户点击最小化的自动事件捕获、其它X11 WM/Wayland、Windows/macOS、长时间最小化/关闭竞态实际UI、跨屏DPI与生产ViewHost；所有dependency/release gate保持。

## 原生事件提示采样、typed可见性信号与三来源合并（Linux）

- 新增NativeWatcher，在runtime install时创建每窗口独立UUID incarnation与ledger；Focused/Resized/ScaleFactorChanged只作hint，永不按Focused(false)直接判hidden。watch mailbox合并hint，初测及每批50/50/100ms有限查询visible/minimized；无idle持续poll。runtime getters在spawn_blocking，不等待GUI阻塞Tokio core；单window一份owned permit留在实际job中，500mstimeout只结束观测、job未结束不能释放信用，busy/error/invalid发invalid并关闭observer。状态不变不推进revision、不重发相同revision；Destroyed/shutdown关闭worker，late commit失败。
- 注册typed定向NativeVisibilitySample与owner/nonce授权的native snapshot命令；采样后重验WindowToken，不把incarnation当权限凭据。前端先targeted typed listen再snapshot，使用NativeVisibilityFilter只接当前incarnation且更大u32 revision；validate nonempty/bounded identity、integer revision与bool字段。bootstrap最多4份identity、每份只保最高revision、clone对象，foreign或乱序不能覆盖当前；overflow明确fail closed。snapshot后清空buffer，invalid样例取消当前view、不fabricate visibility。
- SurfaceVisibility新增native来源，原子DOM snapshot只更新container/document并保留native值；native.valid&&visible&&!minimized为第三份AND。unit证明native false单独同步retire/pause、native restore不能绕过document hidden。实际finite probe订阅真实event并要求hidden→restored均收到，2秒限期，report schema4新增nativeSignalChecked由DOM阶段验证；命令返回本身不算事件证据。
- 7项native回归覆盖hint不修改projection、重复状态/真实minimized转换、closed拒绝late结果、invalid不是visible/minimize凭据、每window独立identity、u32溢出拒绝、1000 hints合并与close唤醒、真实blocking read timeout后permit仍为0且下一read不执行直到释放。9项TS回归覆盖old/duplicate/reordered过滤、invalid/bounds、三来源AND、bootstrap乱序foreign/容量/clone。完整check/test、30native/135TS、4xtask、89core/38app/16IPC、native clippy -D warnings、bindings drift及UI机械检测通过。Astrolabe依赖索引标记stale，只用于参考，未作为LSP诊断通过。
- WM模式与无WM默认模式各一次完整Linux运行成功（各2headless+2DOM+7transport/cancel），两份report nativeSignalChecked=true。WM nativeMinimizeChecked=true、generation=12、104×12、frames=788、peakCreditBytes=4115、renderEvents=785/frameFences=784；默认nativeMinimizeChecked=false、frames=792、peakCreditBytes=8212、renderEvents=790/frameFences=788；二者parsedBytes=3,113,119/nativeExit=0、digest=`00b088cbb609382c`、12行DOM匹配。最后仅强化超时job信用与bootstrap乱序负向分支，确定性新增测试/完整门禁通过，未重复已通过正常GUI路径；单次数值不承诺性能。
- 实际信号来自Tauri窗口事件hint后的runtime采样，native cycle命令没有伪造observer hint或native event；但实际probe仍先host隐藏pause，然后minimize/hide，因此只证明采样/定向事件/AND集成，不是用户点击最小化作为唯一来源独立驱动pause验收。通用WindowEvent没有可靠desktop minimize专用事件，超过有限settling窗口或无hint的WM变化可能漏判；其它WM/Wayland/Windows/macOS、production多pane ViewHost、长时间hidden策略/跨屏DPI/IME仍未完成。未新增依赖/改锁文件/CSP/vendor/release忽略。

## 原生信号作为唯一来源独立暂停/恢复（Linux）

- 新增finite probe_session_native_cycle并复用原来GUI恢复guard/状态回读，共用run_native_cycle；新入口先runtime owner/nonce/current binding只读准入、external generation单次预留，然后只调用hide/minimize，绝不主动detach或伪造native hint/event。最长2秒等待frontend controller真正pause票据（registry.detach成功）后，才让helper产生HIDDEN_SEED并继续Rust parsed_bytes；100ms后Rust show/unminimize并严格读回可见、finish native收据。
- finite gate.external预留本身不授权resume/hidden或排pause；pause仅对应generation并成功revoke才设置native_started/inflight、watch发布；old/duplicate/zero reserve失败。native finish前resume拒绝，Destroyed可即时唤醒尚未pause的等待者，early frontend pause先于wait注册不会丢失。4新native回归覆盖预留无pause/错误generation/单次、关闭未pause action、失败revoke/早到pause及已paused不得reserve。
- public SessionRegistry.check_binding(owner,id,generation)是元数据只读准入，不ACK、不输入、不resize、不改变信用，也不保证asynchronous后来仍current；真正pause仍经原owner/generation门禁。新增1app回归覆盖未attach、foreign owner、换绑old gen、detach/close拒绝，controller/SessionTask仍Running直到explicit shutdown，driver输入为空。
- DOM native-only阶段移除host.hidden=true/false与bindSurfaceVisibility（document/container来源保持true），不直接pause/resume。typed native false通知里记录nativeHidden，controller retire要求nativeHidden且host.hidden=false，cancel旧ACK/input/render然后自身pause；native restored只触发controller resume，resume callback先等待同次native动作Promise完成并核对hiddenBoundary推进/旧Channel帧数不变，再期待new Begin及resume。这样visible event早于finish_native不会造成过早IPC错误，scope内仍1.5秒不确定截止、无重试。
- report schema4新增后台nativeOnlyChecked与DOM nativePauseChecked，分别要求external.reserve+native.finish真实收据和native作为唯一source退役；两类探针DOM必须true、headless=false，原nativeSignalChecked保留。controller数据不进React state，数据仍单ordered binary channel。wrong future generation native cycle与duplicate调用被真实WebView拒绝。
- WM最小化与无WM hide/show各一次完整Linux运行通过（每次2headless+2DOM+7transport/cancel）。WM nativeMinimizeChecked=true/nativeOnlyChecked=true/nativePauseChecked=true，generation=12、3resize/1pause、104×12、parsedBytes=3,113,119、frames=792、peakCreditBytes=4115、renderEvents=790/frameFences=788；默认minimize=false而另外两项true、frames=786/peak4115/render783/fences782。二者nativeExit=0、digest=`00b088cbb609382c`、12行DOM匹配，隐藏期HIDDEN_SEED在new generation恢复，后续keyboard/repair/EOF仍全通过。
- 完整check/test、34native/39app/135TS、89core/16IPC/4xtask、native clippy -D warnings、bindings drift/真实新command DTO注册与UI机械检测通过；Openbox无泄漏，锁文件/CSP/vendor/依赖门禁不变。实际minimize/hide由runtime测试命令而非人工标题栏点击，所以证明native信号可独立驱动整条pause/resume，但不证明真实human事件、所有WM/Wayland/Windows/macOS、长期hidden/production ViewHost或跨屏DPI/IME验收；无hint/延迟超过采样窗的风险仍存在。

## 外部WM动作独立触发最小化/恢复（Linux X11）

- 新增mise run probe-window-external/xtask/CLI --probe-window-external与独立X11 controller脚本，复用新Xvfb/Openbox而非用户桌面。模式下应用正常cycle不调用window.minimize/unminimize/show，只在wrapper提供的umask077私有临时dir atomic rename固定动作:decimal generation marker。controller是独立进程，按自己直系app子进程PID寻找唯一可见XID、核对getwindowpid，执行timeout2s xdotool windowminimize/windowactivate --sync；不是CSS隐藏、失焦替代或由应用自身GUI API完成动作。
- 原runtime readback、native event hinted sampler、native-only controller pause/hidden output/恢复收据/精确新Begin链路保留。新增后台externalWmChecked且必须nativeOnlyChecked+nativeMinimizeChecked；server与xtask核对CLI mode/DOM true/headlessfalse，普通probe-channel/probe-window=false。独立window-external.json不覆盖另外两模式；controller动作日志window-external-actions.log纳入CI上传与xtask门禁：≤128B、恰好4行、两轮minimize→restore同gen、generation正整数规范/不同递增，拒绝缺失/错配/重复/倒序/非法文本。2项新增xtask单元回归验证这些规则，现6项通过；native fixture拒绝无nativeOnly收据却external=true。
- 一次完整外部WM实测成功（2headless+2DOM+7transport/cancel），动作日志minimize:6/restore:6/minimize:11/restore:11；DOM externalWmChecked/nativeOnly/nativeMinimize均true、generation=12、3resize/1pause、104×12、parsedBytes=3,113,119、frames=794/peakCreditBytes=8212、nativeExit=0、digest=`00b088cbb609382c`、12行DOM一致、832×192px、renderEvents=791/frameFences=790。controller/WM pgrep无遗留；xdotool显示两条_NET_ACTIVE_WINDOW初始属性警告，实际动作/状态回读/退出完整成功，未隐藏警告或放宽门禁。新增日志gate对应实际4条收据与确定性测试，不重复已通过外部WM正向运行。
- 4项直接shell negative probes确认非法generation/动作/未请求restore/歧义window均失败1、app停止且私有dir移除；第五项app退出7保留7。wrapper sh -n、git diff --check、native clippy -D warnings、34native、6xtask、bindings-check已通过。Rust/pnpm依赖锁、CSP、vendor及release gate未变；Linux CI新增xdotool系统前置和新task，注册不是远端执行证据。
- 最后普通probe-channel（无Openbox/xdotool依赖的路径）完整运行也通过，DOM externalWmChecked=false、headlessfalse、nativeOnly/pause证据仍true、frames=792/renderEvents=789/nativeExit=0；之后全项目check/test通过，39app/89core/16IPC/6xtask/135TS保持。已有external JSON动作log未被覆盖，分别核对mode false/true互斥；本轮不重复已通过的外部WM正向样例。
- 该测试证明独立外部WM动作能驱动现有native-only pause/resume，仍由有限测试marker协调时间，不声称人工鼠标点击titlebar、真实长时间外部最小化、其它WM/Wayland/Windows/macOS或production ViewHost验收。失败路径恢复guard可尝试同窗口GUI恢复，成功external收据必须由外部controller日志和实际runtime状态共同证明。

## Generation ViewHost作用域与资源生命周期基础（Linux）

- 新增features/terminal/view-host.ts的GenerationViewHost/ViewScope，按pane管理后端ordered Begin提供的u64 generation；拒绝零/负数/旧/重复/越界，不损伤当前scope。begin先退役旧scope再安装新scope，retire只匹配current generation；retirement后last仍单调，父Abort/卸载dispose同步撤销。transition内abort callback重入begin拒绝，防覆盖并泄漏中间scope；resource按身份去重、WeakSet disposeOnce、倒序release，一个异常不跳过剩余，当前cleanup失败闭锁并用稳定错误报告。
- create提供scope自己的AbortSignal，只在current scope dispatch factory；取消立即reject caller，不等factory，仍观察尾部。晚到成功/注册立即清理一次，晚到失败/cleanup不能杀新scope；already-aborted factory不派发，当前factory失败清理已经登记资源。partial分配尚未返回仍须factory作者清理，不能保证未知factory内部副作用自动回滚。19项TS regressions覆盖完整创建/替换/退休/父abort/late success+failure/cleanup异常、去重/倒序/U64精度/重入及在途创建即时取消与尾部释放，154项TS全部通过，无unhandled rejection。
- finite AttachmentConsumer实际每Begin建立scope，DOM/headless异步import/创建经scope.create，renderer/headless/query mask/onData/consumer/input queue/idle登记统一cleanup。onData与idle repair旧任务错误按捕获view.isCurrent检查，禁止cancel后来新consumer；frame callback同时比较consumer identity与scope current。native visibility/controller listener跨surface退役仍活到resume并归pane lifetime，不能提前dispose导致丢失restore。renderer dispose只移除自己的element，不再host.replaceChildren误清新renderer；重复dispose幂等。
- schema4新增DomEvidence.viewHostChecked及server/xtask门禁，真实每DOM3次resize+1native pause/resume共4次scope重建。在new root完成后再次调用priorRenderer.dispose，要求当前root identity和scope current不变，否则失败；末次scope必须current且重建数4才填checked。这不是只验证bool存在；两轮DOM及headless仍保持PTY、old generation拒绝、精确history/repair、input、EOF/12行DOM全部原门禁。
- 初版scope接入默认hide/show与external WM最小化各一次完整运行通过（每次2headless+2DOM+7transport/cancel），viewHostChecked=true/generation=12/frames=787、parsedBytes=3,113,119/nativeExit0，external mode日志两轮4动作通过。最后补input/idle登记与迟到错误守卫后，默认组合再次通过：frames=785/peakCreditBytes4115、renderEvents=781/frameFences781、104×12/832×192px、digest=`00b088cbb609382c`/12行DOM匹配，viewHostChecked=true；未重复外部WM（新增部分用最新默认真实路径与154TS覆盖）。完整check/test在基础集成后通过39app/89core/16IPC/6xtask/154TS，最后JS资源扩展经tsc/Biome/154TS/默认GUI及bindings-check验证，native全套34/clippy -D warnings与新report fixture通过；Astrolabe依赖索引stale，仅参考，不是LSP clean。
- 未修改Rust parser/vendor/dependency locks/CSP/许可证审计；未开放user shell。当前是每pane generation资源作用域，不是完整按key复用的ViewHost pool，parking/LRU/time eviction、React ViewSlot/StrictMode双挂载、跨pane reparent、多window长期恢复/IME/DPI及跨平台仍待验收；生产attach精确恢复门禁不变。

## ViewHost稳定容器池、ViewSlot票据与park/淘汰基础（Linux）

- 新增view-pool.ts的ViewHost/PooledViewController/ViewLease与view-slot-ref.ts/view-slot.tsx。pool由main在React树外创建，稳定key复用controller和element；factory只同步构建controller、不open PTY，复杂async surface创建仍归GenerationViewHost。mount返回独立ticket/signal，React19 ref cleanup释放该ticket而非按key；直接same-key移到新slot使旧ticket失效，旧cleanup不能park新挂载。same-document/connected/非parking/cycle与slot独占先验证，非法key/slot不创建resource。
- park使mount signal同步abort、element进入hidden+inert parking，同步setVisible(false)，controller lifetime仍活着；重挂同key只setVisible(true)、复用element/controller。默认总32份（可配1–64）/parked最多6份/5分钟TTL，朴素有界Map+monotonic time/BigInt park次序，单次timer与sweep按TTL/LRU只淘汰parked，mounted永不受TTL/LRU淘汰。factory/visibility错误清理自己element与lifetime，父abort收尾全部controller；单disposer错误不跳过其它entry。
- dispose可返回Promise，1.5秒不确定截止只报告，不释放仍执行任务的额度。误用async setVisible adapter明确拒绝/观察尾部并计入同entry pending；两个visibility/dispose尾部均实际settled才从retiring移除。pendingDisposals也占maxViews，超时/错误不能无限重建堆积后台任务；旧entry失败或完成不remove同key的新entry，旧mount ticket abort/release不伤新挂载。factory未返回partial仍需自己清理。
- 25项池单测（minimal fake DOM，不当浏览器证据）覆盖key复用/同元素移动、StrictMode式mount-release-remount、hidden+inert/controller lifetime区分、TTL/LRU/mounted保护、容量、constructor/visibility/park/cleanup异常与parentabort、pending timeout额度、双尾部/误用async与slot占用、late cleanup、新旧ticket/ref callback。另3项view-deadline回归覆盖6秒明确到期/旧generation timer清理/parentabort与非法参数。182TS、全项目check/test（39app/89core/16IPC/6xtask）、tsc/Biome/Vite、34native/clippy-D warnings与bindings drift通过；mechanical UI检查通过，Astrolabe依赖索引stale仅参考。
- 实际App改为ViewSlot包稳定terminal-host（内层只有pool控制，不让React管理xterm子树），pagehide父lifetime回收pool/parking。两DOM samples结束要求主pool size=1/factoryCount=1。finite adapter park选择取消运行样例，不冒充生产session pause/resume；factory从未直接open backend session。
- 新EOF-only真实DOM测试用当前xterm root在两个slot间移动同entry、检查old lease.release无效、park到真实hidden/inert container且controller未dispose、reacquire相同element/root/factoryCount1、辅助parked LRU/TTL eviction而mounted root仍存活，dispose后root返回原host。注入monotonic测试clock只验证TTL逻辑，不等待真实5分钟。原backend PTY/native-only暂停/恢复在EOF之前走原门禁，停车DOM测试期间没有活跃输出，所以不算backend活跃session parked recovery。报告schema4新增viewPoolChecked、server/xtask硬要求true（native fixture补false拒绝）；scope的viewHostChecked仍保留。
- 初次组合失败：两headless完成后首DOM挂起，错误session.frontend_deadline，未取得DOM evidence。随后只加stable phase/r/b诊断的默认运行通过，原始停顿未复现且尚未根因定位，不能归因为CSS或称已修复。保留阶段/resize/rebind计数（不含terminal内容）诊断；finite expected fit→new Begin用6秒scope deadline，旧scope退役清timer，不把没有measurement的无限等待当成功。样例取消现在立即结束前端race并观察nativePromise尾部，70秒总截止不增加/不自动retry。以后再现可明确定位哪个stage，最初失败仍记为未解释的稳定性风险。
- 最新上述有界监督版默认hide/show与external WM最小化各一次完整运行通过（各2headless+2DOM+7transport/cancel），viewPoolChecked/viewHostChecked=true，generation12、3resize/1pause、104×12/832×192px、parsedBytes3,113,119/nativeExit0/digest=`00b088cbb609382c`/12行DOM一致。默认frames782/peak4115/render780/fences778/externalWm=false；external frames793/peak4115/render791/fences789/externalWm=true，四动作日志minimize6/restore6/minimize11/restore11且无Openbox/xdotool遗留。
- 本轮没有新增Rust/pnpm依赖、改锁文件/CSP/vendor或audit白名单。池和React ticket基础已实现，不标生产PTY停车/长时间hidden恢复、开发React StrictMode完整系统循环、多pane并发实际后端状态、cross-window DOM迁移/IME/DPI/Wayland/Windows/macOS或三平台性能完成；首次未解释DOM stall需持续跟踪。

## 活跃pool停车、fenced resume与真实后端长期detach（Linux）

- ViewLease新增resume(slot)，只保留该ticket自己park后的同entry claim；挂载被替换/TTL/evict/已用claim不能重新factory或窃取新view。ViewSlotOwner分开React mount scope与内部pool lease，imperative park/resume更新当前lease，最终React cleanup跟随最新lease；旧ref cleanup/旧claim不伤新owner，mount observer失败rollback park。8项新增pool/slot回归，现33项通过。
- PaneVisibility绑定当前attachment控制面并同步pool false/true通知；false先SurfaceVisibility.retire本地consumer/render/input/idle，当前pane lifetime及backend SessionRegistry不关闭。old stop/abort与已取消晚bind不解除新observer，snapshot/notification失败明确清理，重复来源不重复detach/resume。9项bridge回归；没有活跃control bind时park仍fail closed取消finite样例，不虚构可恢复状态。
- 实际主React pool在第三resize后park，检查真实hidden ancestor/PaneVisibility=false，等待owner/nonce/generation pause票据，旧input/ACK拒绝，后台native PTY接受hidden控制后parsed_bytes增加、100ms旧Channel无新帧；原slot.owner.resume经claim重挂相同controller，container true驱动controller授权准确new Begin，恢复HIDDEN_SEED且pane signal未abort。随后native hide/WM minimize作为唯一native来源独立第二pause/resume，window状态严格回读；之后原query/repair/keyboard/EOF全门禁继续。
- finite driver cap由1 pause→2，gate.wait_resume已消费并清空票据，未放宽registry owner/gen检查或rollback；新2项native回归验证pool receipt未消费不能再pause、消费后更大native generation/early resume/old重复拒绝、Destroyed后不得开始下一cycle。DOM report activeParkChecked/pauseCount2/five rebuilds硬要求，headless0。
- 默认及外部WM各一次完整Linux组合通过（每次2headless+2DOM+7transport/cancel），activeParkChecked/viewPoolChecked/viewHostChecked=true、generation14、3resize/2pause、104×12/832×192px、parsedBytes3,113,152/nativeExit0/digest=`00b088cbb609382c`、12行DOM一致、主pool factory仍1。默认frames791/peak8212/render788/fences787；external frames794/peak4115/render790/fences790/nativeMinimize+externalWm true。有限停车不是生产任意长时间隐藏、真实React开发StrictMode整链、多pane/跨window验收；前次未解释初始化stall继续保留phase/6秒fit诊断，未在这两次再现。
- probe-local-session schema4新增detached：独立固定Rust helper保持stdin待命，真实registry binding Commit后detach，旧input exit被拒绝，backend trusted控制注入DETACHED_FIRST Unicode marker，等待实测6000ms仍Running，再注入DETACHED_NEXT并准确新generation capture，Rust replay全部cells与actor inspect一致，之后current bound exit/有序End ACK/nativeExit0/直接子join成功。证明backend无view本身不受GUI5秒有限策略关闭，不是跨平台/长期GUI恢复通过。report.detached renderer明确Rust validator而非frontend callbacks，Windows注册不是ConPTY执行证据。
- 全项目check/test 199TS/39app/89core/16IPC/6xtask、36native/clippy-D warnings、bindings-check与GUI及长期native probe通过，机械UI检查随后通过；未新增依赖或修改locks/CSP/vendor/审计ignore。生产attach精确性门禁保持，未给不支持状态输出近似帧。

## S4初始隔离SSH协议探针（Linux）

- 新增mise probe-ssh与xtask optional ssh-probe feature，russh0.63.3 default-features=false/ring，rand0.10配匹ssh-key0.7RC的rand_core0.10（0.9示例混用不符合真实API，实际编译修正）；仅xtask test feature，不连接用户endpoint、不启desktop生产SSH。纯控制server只listen127.0.0.1:0，固定synthetic身份/密码/exec，不执行任何shell；临时Ed25519 keys，client pinned完整PublicKey且明确拒绝host certificate，changed key必须设置mismatch并在任何auth handler被调用前失败。
- 真实TCP验证wrong password拒绝、正确password/publickey认证、PTY成功回执、window-change91×37被server观察、Unicode+字面shell符号数据回显精确、exit_status7后继续drain至EOF/Close且尾部SSH_END完整；nonfixed exec请求failure回执，client断开。server正常shutdown/listener task join并等待active handlers归零，失败Drop也请求shutdown/abort listener，不能以仅listener stop当连接排空证据。
- 本机系统OpenSSH10.0p2作为独立client连接controlled russh server：-F空配置、BatchMode/IdentitiesOnly/IdentityAgent=none、StrictHostKeyChecking=yes、独立临时known_hosts、host key ssh-ed25519、清SSH_AUTH_SOCK、不读用户config/agent。private key仅在tempdir（Unix file0600）写盘，读取stdout上限1KiB、5秒不确定期限，kill_on_drop；fixed exec stdout SSH_EXEC_OK及nativeExit7、stdout排空/join通过。未生成已存在用户凭据或改用户~/.ssh。
- 成功报告schema1全部安全/阶段字段经validator再写，开始运行先删旧成功报告；3feature单测验证pinned key匹配、username+credential拒绝、completed单flag不足，xtask feature9 tests与-Dwarnings通过。默认check/test 199TS/39app/89core/16IPC/6xtask通过；最新真实probe成功，server/client/系统child都清理。CI Linux新增explicit feature运行/检查/单测；仅注册不算远端CI或跨平台验收。首次timeout构造放runtime外导致panic，已修正在async block内部；lint test模块搬到EOF，无忽略生产warnings。
- 新crypto包完整cargo-deny全特性审查仍fail：license原6拒绝、paste/proc-macro-error维护2；sources/bans通过，无新增拒绝或漏洞项，详dependency-review。没有扩大allow/ignore，SSH-key RC风险单列；桌面树不含russh。S4未通过：只取得受控russh双向和OpenSSH10client证据，不是真实sshd7–9/dropbear/旧设备、keyboard-interactive/证书、跳板/forward/Agent/FIDO2/GBK、真实remote PTY或完整transport actor集成。

## S4真实OpenSSH服务端/原生PTY尺寸验证（Linux）

- 新增probe-ssh-system，复用xtask可选ssh-probe依赖，未加生产crate或新Cargo包；Linux当前/usr/sbin/sshd10.0p2/stty/既有/run/sshd。临时config/Ed25519 host+authorized keys/随机loopback端口，固定当前测试account；不改/etc/sshd_config、~/.ssh、passwd或用户权限。UsePAM=yes只使用现有系统account/session策略，认证只publickey；临时StrictModes=no为私有temp授权路径，不是生产策略。ForceCommand仅固定Rust helper，remote原命令非空拒绝、stdin只3类固定test命令且字面shell符号从不执行；agent/tcp/X11/tunnel/用户env转发禁用。
- russh pinned host key连接真实sshd，错key明确拒绝；公钥认证成功、PTY与shell request回执成功。实际helper通过stty size读取内核slaveTTY，初始24行80列；SSH window-change91×37后再读37行91列，不能只凭send queued或本地payload标记通过。固定Unicode/字面shell符号SYSTEM_ECHO验证；exit命令helper输出SYSTEM_END然后native退出7，client必须继续drain EOF/Close及尾部marker后断开。
- 正向退出在仍运行的master /proc/pid/task/pid/children上最多3秒等所有owned session children归零，再停止并wait master，分别记录nativeSessionsDrained/nativeDaemonJoined；只操作创建的daemon，不kill现有系统daemon。所有标准请求/等待3–5秒，总25秒；失败child kill_on_drop，fixed helper12秒watchdog，但不把该兜底或master kill冒充正向session排空。临时目录结束清理，report没有实际username/host key/密码/端口。首次未知startup占端口竞争由pinned随机hostkey拒绝，不能连接非owned daemon并假成功。
- 两次真实运行通过（第二次加入native child drain/完整validator），报告serverVersion OpenSSH_10.0p2/publicKeyAuth/realNativePty/nativeSessionsDrained/nativeDaemonJoined/EOF均true、[80,24]→[91,37]、exit7。后续新增1项真实report负向fixture（false nativePTy/child/eof/master、错尺寸/exit、missing limitations拒绝），feature10tests/clippy-Dwarnings通过；默认全check/test199TS/39app/89core/16IPC/6xtask及原probe-ssh仍通过。
- 新mise/xtask CLI/CI entry与报告字段已注册；CI Linux系统依赖openssh-client/server及/run/sshd目录明确，但无远端执行证据。本轮Cargo.lock/deny/vendor/CSP不变，原全audit6license/2maintenance阻断继续。S4仍不全通过：实际只有OpenSSH10server与10client，7–9/dropbear/legacy设备、keyboard-interactive/证书、跳板/SFTP/forward/Agent/FIDO2/GBK及生产actor transport尚未验收。

## S4受控keyboard-interactive挑战/回答/取消（Linux）

- probe-ssh controlled server新增K-I handler，每连接challenge布尔状态，仅当前synthetic user的start发1个固定prompt/echo=false，回应必须exact单答且当前challenge有效；回应时先消费challenge，错误答拒绝、没有challenge不能接受。client校验name/instructions/数量/文本/echo都为受控值，unknown/额外prompt/echo真拒绝且不记录远程内容/回答。所有值是fake测试凭据，不访问实际MFA或用户secret。
- 真实encrypted loopback先挑战→错synthetic答返回Failure(partial_success=false)，重新start fresh challenge→固定正确答返回Success；另一独立connection start后不给答案直接disconnect/drop。handler Drop对尚未答复challenge计数，必须在全server shutdown之前3秒独立出现cancelled=1（而非用global shutdown冒充取消）；started3/denied1/accepted1硬验证，最终所有active connections仍归零。
- ssh.json新增keyboardInteractive五字段且validator硬要求，明确noRealMfaHardware=true；受控protocol已通过不代表真实MFA/PAM/前端Prompt ID生命周期或FIDO2验收。新增1项prompt负向单测，11 SSH-feature tests/clippy-Dwarnings通过；probe-ssh与真实probe-ssh-system都再运行成功（real PTY仍80×24→91×37/nativeExit7/session children drain）。未改Cargo依赖/locks/CSP/allow/ignore。
- OpenSSH7–9/dropbear/legacy设备、证书/jump/forward/Agent/FIDO2/GBK和生产SSH actor仍待后续验证，S4不标全通过。

## S4逐跳独立host-key校验与direct-tcpip relay（Linux）

- 新probe-ssh-jump使用两个controlled russh endpoints，仅127.0.0.1随机port。outer先pinned/authenticate，direct-tcpip white list仅owned inner exact host/port、不得DNS/外部IP/错port；单owned Semaphore permit从TCP connect准入到worker退出，不在timeout取消后提前释放，第二forward明确拒绝。6秒copy_bidirectional监督和8KiB test packet/output cap；这不是完整SDK缓冲/RSS/backpressure高压证明。
- inner KEX/client::connect_stream通过outer SSH channel.into_stream，使用不同expected host key，wrong inner key必须实际触发inner Pin hook并失败；cannot inherit trust from authenticated outer。首次实现outer Handler.data对forward ciphertext误Disconnect，错误早于key验证，未把它算成hostkey拒绝；修复为outer数据交channel stream、只有inner应用channel echo，后实际hook reached/negative gate通过。
- 先拒外部目的地，再wrong inner key/取消outer，要求outer relay+两边connections在listener still alive时独立归零；重新outer/inner authenticated，第二并行forward拒绝；内层应用回显4400 bytes UTF8及字面shell符号精确，EOF/Close均明确收到后，两SSH连接断开、relay+connections在server shutdown前归零，最后join两个listeners。不是发送exit或仅listener停止便声明所有任务已排空。
- 成功report schema1包含逐跳pins/inner mismatch/destination拒绝/singleForwardCredit1/secondForward拒绝/nestedBytesExact/innerEofAndClose/both connections+relays drain/listener join并validator逐项要求。新增2单测验证exact owned loopback/port与单completed不足，SSH feature13 tests/clippy-Dwarnings通过；全默认check/test199TS/39app/89core/16IPC/6xtask与原K-I/real OpenSSH10server probes再运行成功。CI Linux新增调用，未取得远端CI；Cargo依赖/locks/CSP/audit配置未变。
- S4仍未全验收：两个controlled endpoints不代表OpenSSH7–9/dropbear/真实多级跳板实现矩阵，生产Route连接池、任意目标/remote-forward、SFTP/Agent/FIDO2/证书/GBK、实际slow consumer SSH窗口压力与actor集成待验证。仍不开放用户endpoint或production credentials。

## S4限制性用户证书认证/CA与subject持钥（Linux）

- 新probe-ssh-certificate，临时Ed25519 CA/subject与独立pinned host，loopback controlled server仅certificate auth（其它方法不开放）、fixed exec。政策用Certificate.validate_at验证CA签名/可信SHA256指纹和时间，另补User类型、当前expected username、exact principal存在与critical_options空。仅verify_signature不能当信任，库validate也不自动验证principal/critical options；本test policy拒所有critical选项而非忽略force-command等限制。没有生产CA信任库/撤销政策/用户UI采用承诺。
- 真实加密SSH连接依次使用6invalid cert：unknown CA/expired/future/wrong principal/Host type/critical force-command，全部authenticate failure；随后wrong username/不同leaf私钥搭配valid cert也明确失败，确保CA签名不等于client私钥持有证明。所有negative后accepted=0，再1valid cert authenticated、固定exec CERT_OK、exit7+EOF/Close/output drain，所有connections在server stop前独立归零，最后join listener。CA/keys只在内存，不采用户credential或写CA secret。
- 2新增feature单测确定性timestamp100..200：100 inclusive/199 valid/99未来/200 expired、wrong CA/username/principal/type/options拒绝；显式Builder.all_principals_valid生成golden ticket也被此policy拒绝。report必须negativeCertificates6、wrongUsernameRejected/leafKeyOwnershipRequired及所有CA/time/type/principal/EOF/drain字段，单completed不足；15SSH-feature tests/clippy-Dwarnings、真实cert probe通过。默认check/test199TS/39app/89core/16IPC/6xtask及K-I/OpenSSH10server/jump probes再通过。
- 新mise/xtask/CI及ssh-certificate.json已注册，本轮无新依赖/locks/审计配置变动，原6license/2maintenance release gate保持。受控认证证明不代替OpenSSH7–9/dropbear/实际企业CA/撤销UX、主机证书信任、Agent/FIDO2/PKCS11/密钥加密、GBK或production SSH actor/SFTP/转发验收，S4仍未全通过。

## S4私有软件Agent签名/SSH signer认证/identity清理（Unix/Linux）

- 新probe-ssh-agent仅Unix可编译运行，ssh-agent -D使用私有temp sock、explicit AgentClient.connect_uds，不connect_env/读取用户identities，不改全局SSH_AUTH_SOCK；子ssh-add只override其私有socket/无SSH_ASKPASS/无stdin prompt。初始Agent列表为空，加载1份临时Ed25519软件key（0600/目录0700/30秒TTL），身份必须匹配generated pubkey。
- 真实Agent raw SSH Signature验证用signature::Verifier（与SSHSIG namespace文档签名不同）；真实SSH authenticate_publickey_with only publickey+Agent signer，不把subject私钥交给SSH client。controlled loopback pinned server固定exec AGENT_OK/exit7/EOF/Close/输出排空与connection-before-server-stop归零，listener正常join。
- remove_all_identities实测earlyEOF，上游0.63.3请求前4字节零header问题单列dependency-review，不忽略失败或修改upstream。改用scoped remove_identity同key，Tokio duplex test确认frame length/message18/key string长度；实际Agent identities为空、removed key签名明确AgentProtocolError而非任意IO失败、随后再次list成功且空证明Agent仍活着。最后ownedAgent kill/wait join，tempdir清除，用户Agent无操作。
- report ssh-agent.json/isolation/software-vs-hardware/签名/auth/移除拒签/EOF/server+Agent cleanup全字段validator硬要求，缺completed-only拒绝；2新增单测，17SSHfeaturetests/clippy-Dwarnings、实际Agent probe通过。全默认check/test199TS/39app/89core/16IPC/6xtask与此前K-I/real OpenSSH10server/jump/certificate4探针再次成功，CI Unix-Linux调用已注册。无新增依赖/locks/audit修改。
- 这只是软件Ed25519 Agent路径，不算sk-ssh-ed25519/FIDO2真实触摸/PIN、hardware失联/用户确认、remote agent forwarding/Pageant/windows pipe/macOS矩阵、production credential vault或actor SSH adapter完成。S4仍有真实设备/实现矩阵未验证，release gate6license/2maintenance不放宽。

## S4受控Agent forwarding显式channel grant（Linux）

- 扩展私有软件Agent场景，system OpenSSH client -a与-A分别连owned pinned russh server，仍-F空config/private known_hosts/private IdentityAgent/socket/key；不接用户Agent或其它identity。server按当前ChannelId记录agent-request授权，不把connection一次授权泛化到任意channel；未grant的forward-fixed-exec明确channel_failure并denied计数1，不能把客户端generic网络失败算作权限拒绝。
- -A明确开启请求后，server handle.channel_open_agent经SSH认证通道反向拿Agent流，request-identities必须仅唯一synthetic publickey、raw sign固定数据并本地Verifier验签。只签名和public身份通过隧道，不传private key给server。一个owned task permit贯穿3秒query/sign/reply直到实际task结束，Guard计active与completed，最终completed1/denied1/active0与全部SSH connections0在listener stop之前证明；系统client exact FORWARDED_OK/exit7/stdout drain，之后仍scoped移除synthetickey并拒签/ownedAgent daemon join。
- report.forwarding新增五字段（explicitChannelGrant/disabledRefused/signVerified/credit1/taskDrained）validator硬要求，新增permit/Guard生命周期单测，18 SSH-feature tests/clippy-Dwarnings通过；实际probe-ssh-agent、原4SSH probes和全默认check/test199TS/39app/89core/16IPC/6xtask再次成功。没有新dependencies/lock/audit/CSP/vendor修改，上游remove-all帧bug仍记录、没有忽略。
- 仅受控OpenSSH-client→russh-server/private软件Agent转发，尚未实现/验收Pulse客户端reverse-channel bridge或生产默认禁用/按host确认UX、realFIDO2触摸/PIN/断开/复杂Agent操作取消、Pageant/Windows pipes/macOS、任意remote forward或长期pressure。单task额度不承诺Agent硬件操作会随网络timeout取消；S4仍部分证据。

## S4旧KEX/MAC的显式test-profile opt-in（Linux）

- 新probe-ssh-legacy复用private real OpenSSH10 daemon，仅其临时config offer diffie-hellman-group14-sha1/aes128-ctr/hmac-sha1。client普通Preferred默认连接在host-key hook/认证前typed Error::NoCommonAlgo(kind=Kex)拒绝，不能用generic网络/timeout/主机key错误伪安全拒绝。
- 仅此显式CLI/profile构造Cow owned列表1旧KEX/1MAC/1cipher+Ed25519 hostkey，复用真实sshd正确pinned key/publickey/native PTY/stty80×24→91×37/UTF8/exit7/EOF及owned children/master join；wrong pinned host仍拒绝。之后再次ordinary default连接同legacy-only daemon仍typed拒绝，说明opt-in没有修改global defaults或auto retry/downgrade。现代profile完整再通过，flag分别false/true并存独立ssh-system.json/ssh-system-legacy.json，mode_matches禁止错模式、缺bool或string冒充。
- 2新增单测检查create legacy后默认KEX/MAC相等且不含SHA1、legacy只Ed25519，以及report互斥/缺flag拒绝；20 SSH-feature tests/clippy-Dwarnings与modern+legacy实际运行通过。其余K-I/jump/cert/Agent-forward4探针及default check再成功，CI追加明确legacy调用；没有新依赖/lock/audit/CSP/正常daemon配置改变。
- 注意russh Preferred默认safe KEX/CIPHER/MAC排除旧SHA1/CBC，但key列表仍含RSA{hash:None}（ssh-rsa）；本profile RSA feature关闭且只有Ed25519，不能据此承诺启用RSA后的host-key列表也完全现代。生产legacy-per-host政策需另行过滤旧host-key算法并明确用户opt-in，不照搬全DEFAULT key列表。未启group1/DSA/3DES/RSA-SHA1，不验证真实旧设备及用户警告UX；S4矩阵未全完成。

## S4嵌套SSH二进制GBK回显与严格分块转码（Linux）

- 新test-only gbk_probe复用owned双SSH跳板通道，输出固定中文/ANSI/DSR fixture编码为GBK实际33 bytes（UTF8则43 bytes）；真实channel传输、回显原始bytes完全相同，不将SSH Data强行from_utf8。返回gbk报告并纳入jump成功validator，server应用bytes计数同时包含UTF8 4400和GBK33。
- 编码strict检查GBK.encode had_errors，unrepresentable emoji明确拒绝而非numeric entity替换；decode使用有状态new_decoder_without_bom_handling/decode_to_string_without_replacement，bytes≤4096/out-cap16KiB/完整consumed&InputEmpty，最后empty last=true排空。所有chunk sizes1..33（含leadbyte边界）均得到exact43 UTF8 bytes，与reference backend TerminalProbe cells/DSR replies一致；非法0xff/半个lead0x81EOF均fail而非U+FFFD隐藏损坏。
- 新3单测覆盖all splits、malformed EOF/unsupported input、bound/缺report；23SSHfeaturetests/clippy-Dwarnings及真实jump+GBK通过。全默认check/test199TS/39app/89core/16IPC/6xtask与其余5SSH模式再次成功；CI既有jump调用即强制新GBK报告。新增encoding_rs0.8.42与3transitive包，完整audit仍原6license/2maintenance失败，sources/bans通过无新增拒绝，原锁定包版本未升级。
- 严格边界：只是controlled GBK echo+后端解析归一化测试，不是实际GBK交换机/自动侦测/所有GBK扩展/生产codec。raw source bytes33与logical UTF8 bytes43不同，不能直接把现有attachment parsedBytes/raw-offset定义当已经适配；actor original-byte/reconnect/history/input query transcoding及Zmodem在转码前分类仍未接。原协议精确性/flow-control门禁不放宽，生产charset支持未标完成；S4仍部分矩阵。

## ADR-029双坐标v2与validation-only GBK actor恢复

- 用户明确选择版本化双坐标，保留原始SessionState.parsed_bytes并新增utf8_bytes/source_is_utf8；协议v2独立于原v1，未改旧parsedBytes公开语义。Rust attach_v2 + TS AttachmentV2Receiver/Consumer以28-byte envelope/36-byte Begin分别追踪source与display。part IDs/ordered channel/credit共用既有FrameKind8，互相reject mixed versions/unknown flags，bigint保持大于2^53精度。
- source消费无显示（半GBK lead）发布pure Marker，decoder在backend actor且跨lease存活。v2 Data按实际body长度推进UTF8，source独立monotonic；Checkpoint/EOF/Repair保持两个cursor一致，sourceRegression和source正确/display错误均failclosed。renderer摘要SessionState比较包括两个计数及mode；当前desktop仍UTF8-v1-only，未增加GBK窗口命令。
- pulse-app attach_v2显式准入，复用同一Driver的ACK/timeout/cancel/EOF/repair监督，driver livecursor改为state.utf8_bytes；Fragment末记录原source终点，不推算一对一raw映射。v2limit在atomiccapture前校验；v1先拒source_is_utf8=false，哪怕source/display均0，保留旧lease。3app回归证明invalidcredit保旧lease、wronggeneration只退视图新view继续EOF、noACK实际停actor且timeout不关session。
- validation feature才可调用spawn_gbk_text_session，未接productionbinary/Zmodem分类。rawchunk≤64KiB，单次最大3×64KiB+4 scratch并按≤64KiBrecords有界分发，source在第一份decoded record消费一次，后续source不增加；满lease阻止pending解析/继续读transport。4core tests覆盖半GBK原始1/UTF80跨capture→trail原始2/UTF83，64KiBsource→98304UTF8两记录，input中文反编码/emoji拒写/query反编码，malformed及halfEOF=Failed不当normalEOF或replacement。
- 共享attachment-v2.hex由Rust sender byte-for-byte校验，包含独立bigintsource/display、source-onlyMarker、中文展开Data、samecursorRepair与EOF；TS8tests检验mixedversion/duplicatedsource/sourceRegression/repair+EOF mismatch/oldgen/queuedcallbackACK/cancel+latewrite/unsupportedmarker/overflow。Rust额外pre-copybody bound与Commit释放bootstrapVec容量回归；v1原全部用例保持。
- 新mise/xtask/CI probe-encoding-attachment两份actualactor→v2→headless样例（含history），准确半GBK捕获与新lease重建，EOF source7/UTF89和source82/UTF884均与Rustscreen一致，无frontendquery泄漏。生成阶段ACK先保存wire以运行actor，TS独立callbackgated消费再ACK，不称同进程roundtrip或realWebViewGBK证据。默认prod-feature独立consumer禁止test-onlyGBK入口，existingreplay/Echo门禁保持。
- 验证记录：核心组合check/test207TS、93core/42app/22IPC（后续pre-copy回归增至23）、v1五headlesscase与v2两case通过，默认API隔离通过。原生UTF8-v1真实Channel/DOM复验completed=true（2sessionRuns/2domRuns/7measurements），bindings-check/native clippy-Dwarnings/36native tests通过；encoding_rs没有新版本，仅lock增加pulse-term可选依赖边，审计许可/公告门禁没有豁免。首次运行在2headless之后/首DOM期间native_query_failed，root cause未定位；补static分类日志但500ms/failclosed不放宽，后续通过不宣称根因已修。
- 未完成：productionbinarypreclassifier/临时encoding切换、多charset/真实GBK设备、Tauri窗口v2适配/DOM/三平台编码acceptance，S2/S4仍不标全过。UTF8原有invalid字节/EOF replacement行为不改，strictGBK拒绝政策目前只对已分类测试文本有效。

## v2注册层授权与实际GBK bound input/单坐标摘要拒绝

- 新SessionRegistry.attach_v2显式选codec，复用相同attach_version实现，不新增授权token或generationcounter。先owner/session校验，预留monitor/capture锁，再mint generation，captured后再次核对owner才install，oldbinding detach和latecompletion只清当前matching generation。默认attach仍v1；只有可信backendcaller选择v2，未增加Tauri前端参数/任意会话入口。
- 3新增registry tests：foreign v2capture/ACK/input拒绝、windowclose撤view（appshutdown才关actor）、v1→v2 replacement后oldACK/oldDetach/lateCompletion不能影响newbinding、invalidv2credit保旧v1。45app tests、wholeworkspaceclippy/check与真实probe-local-session native ACK/2MiB pressure/detach/EOF链通过。
- 两份actualGBK headlessprobe由裸attach迁移为registry-owned，外国window capture/ACK明确Forbidden，owner旧generation ACK Stale；boundInput中文真实传输driver收到GBK D6D0CEC4，raw相同而UTF8cursor错误的compare必须Stale，不能清risk或误判Equal。报告三个新增必需bool和TS validator强制检查，headlesshistory/屏幕/EOF source7/utf89与source82/utf884仍精确；没有把producerstaging ACK说成liveJScallback roundtrip。
- no new deps/locks/capabilities/audit变化；此前v1真实WebView复验属于1214679检查点，当前registrywrapper变更采用45unit+realPTY与GBKprobe targeted checks。Tauri窗口v2选择、实际DOM和生产charsetbinarypreclassifier仍未验收，源码不能因backendregistry API存在就开放未批准的credentials/shell。

## v2固定UTF8样例的真实Linux WebView/DOM验收

- 新mise/xtask probe-channel-v2、backend --probe-v2明确选择codec2，仅owned fixed UTF8 PTY helper，没有GBK/任意shell/remotecredential入口。readonly generated probeSessionProtocol只在local origin可调用，frontend选择必须1或2，不从浏览器参数替换后端codec；allcapture/resize/resume用同registry.attach_v2。默认probe-channel/window/external仍codec1，reports独立channel-v2.json。
- SessionEvidence attachmentVersion由backend产出，xtask/headless/DOM全部必须匹配请求mode；新增模式wrong/mixed/缺version unit，v1完整report不当v2。信用精确按fragment8192+envelope20/28，因此v1≤8212/v2≤8220，而不是全局放宽。native unit覆盖8213-v1拒、8220-v2允许/8221拒、unknown3拒，native报告复用sessionvalidator消除重复8212常量。
- 新ProbeConsumer UTF8-only适配器底层为真实AttachmentV2Consumer，私有legacy renderer只在控制边界source==utf8时投影，source-onlyGBK marker/不等Begin明确在render/ACK前拒绝；不让shim冒充GBK显示支持。一般GBK客户端仍直接用双坐标sink。3TS回归mixedversion/flags/unequalbootstrap不reset不ACK/equal边界renderACK/unequalmarker拒，合计210TS，native36tests通过。
- 实测channel-v2 completed=true：2headless/2DOM/7原测量，session attachmentVersion2/dom2，headlesspeak8220/dom8101，DOM每轮resize3/paused2/input+repair+idle+native visibility与EOF一致，实际Raw Channel仍ArrayBuffer。不是性能三平台或GBK DOM声明。第一次因budget误用8212被capture前拒绝；修到8220后两DOM完成但总report重复v1常量拒，改复用版本化SessionEvidence后最终成功。两处都保留hardgate，未扩超时或skipchecks。
- 最终default-v1真实probe-channel completed、typed bindings-check、workspacecheck/tsc/Biome/nativeclippy-Dwarnings再次成功，先前v2有36native tests/210TS，xtask新mode单测通过；CI新增Linuxv2但保留v1/WM测试。无newdependencies/locks/audit/CSP改变。既有native_visibility采样一次失败仍rootcause未定位，不能因最终通过宣称全解决。
- 当前完成v2固定UTF8真实窗口组合，不是productioncharset；GBK DOM、binary/Zmodem预分类、temporary charset切换、真实设备、Windows/macOS依然pending。release6license/2maintenance门禁不放宽。

## 显式验证特性的GBK双坐标真实Channel/DOM样例

- 新desktop encoding-probe feature显式启用pulse-term/validation，default/desktop不自动启用。默认compiled命令只返回encoding.validation_feature_required，--probe-encoding不带feature直接exit1、不启动GUI；terminal-api-check仍确认默认外部consumer无法访问GBK/replay/Echo。generated command surface在两feature中一致，避免drift，但生产textclassification尚未实现。
- 新mise desktop-encoding-build/probe-encoding-window与xtask独立encoding-window.json，only固定已分类文本，不创建用户shell/远端连接/任意charset入口。复用WindowSessions可信owner/nonce/独占slot/RunGuard和registry.attach_v2，fixedinput只中文/确认，旧generation ACK不能影响新binding，任何error走guard撤销/关闭并清理。CI显式build模式与原默认v1/v2/WM验证分别保留。
- 实测两轮GBK actor→Raw Channel ArrayBuffer→AttachmentV2Consumer→可见DOM→callback ACK成功。history15row+半lead初始capture source76/display75，两generation准确恢复；Commit渲染后才输入握手，后端driver收到D6D0CEC4/C8B7C8CF反编码。续流lead产生pureMarker source78/display78，只render fence不写VT，cursor/baseY不变；trail续流中文/GBK_OK，最终source89/UTF891/revision5/digest78682c0fbbb6bc30与backend一致。report replacement1、145frames、peak64、DOM12行、history/sourceOnlyMarker/reverseInput/callbackAck全true；原2headless/2UTF8DOM/7测量也完成。
- 不走等坐标UTF8 shim，renderer只接columns/rows用于几何，所有V2控制事件保留完整两计数；双坐标final/digest及history实际检查，不能把headlessproducer预ACK称为这轮真实ACK。2新native证据回归raw==display伪成功/缺DOM/无ACK/信用65/旧replacement/非法counter拒绝；xtask新增缺字段/无renderACK/同坐标负向。第2case失败保留第1成功和原error，但不能completedtrue；额外native报告回归证明该边界。
- 最终普通v2重建（encodingRuns0、encodingnull）/check/fulltests/GBKheadless/encoding-probe严格clippy全通过；核心93term/45app/23IPC、39native、210TS、8xtask。defaultAPI隔离/GBKmode明确拒绝及bindings-check通过，Astrolabe Rust新模块无error诊断；TS证据来自tsc。无lock/newdependency版本/CSP/audit豁免；only featureedge启用已有encoding_rs。
- 未完成：真实GBK SSH/PTY传输与设备端画面、binary/Zmodem预分类、临时charset切换、生产入口、三平台/IME。已批准strict策略仅对分类fixture有效，不能标S2/S4全通过。

## S4受控双跳SSH到GBK actor/registry/v2桥接

- 既有probe-ssh-jump强制新增独立authenticated session，reuse两hop单独pinned hostkey和temporary synthcredentials。realSSH收到leadD6才喂boundedDriverEndpoint，source1/display0时capturev2，v1拒绝且旧lease被commit替换。trailD0在重建后续接，boundInput中文经actor反编码D6D0CEC4再发送SSH，原样echo回actor，不做from_utf8强解。
- 远端echo DSR触发backend专有TransportWrite::Reply，明确与用户Input区分；收到ESC[1;7R并实际回写SSH，再按raw字节echo解析，backend仅答一次。remoteEOF/Close两收据都成功后才drop driver，actor正常EOF排空，wire.Data UTF8offset按bodylen连续/source单调，唯一End source16/UTF819，与SessionState及UTF8 reference全Screen相同；PeakCredit64。SSHserver总应用bytes增16，原4433变4449，双连接/relay仍drained/listeners joined。
- actualSSH probe通过；2新增metadata negative tests拒同raw/display、假pendingUTF8、65credit和缺query/input/EOFstage证明。27 SSHfeature tests、SSHfeatureclippy-Dwarnings、全workspacecheck/tsc/Biome通过。无newdependencies/lock/desktoppermissions或audit变化，既有CI jump现在强制gbkActor证据。MCP首先找出新增testmod与innerdoc顺序错误，已修复并经编译/clippy确认；一次随后诊断仍返回旧line5错误，缓存不能当最终通过。
- 限制：每阶段有限echo并施加timeout/bytes cap，尚非SSH slow-consumer端到端窗口/ACK压力验收；没有生产SSH transport、GBK远端设备、同条网络到DOM组合、binary/Zmodem预分类或charset切换，不标S4完整通过。先前GBKDOM来自固定本地driver，此处是另一独立真实网络检查。

## 真实远端SSH发送窗口背压与共享连接HOL限制

- 新mise/xtask/CI probe-ssh-pressure，仅私有回环russh端点/合成credential。remote fixedexec产生4193280 GBK source（8190×512），通过真实ChannelWriteHalf.data按SSH窗口reserve发送，utf8显示4892160。client receivewindow32768/channel_buffer2/packet8192，actor transport queue8和lease/wire有界，frames1/credit8220。初版同步Session.data echo将4MiB排到server内部pending，即使actor/reader停也不阻producer，失败已确认，改为window-aware生产者重测，未放宽停止断言或修改第三方源码。
- ACKhold后700ms与后续窗口稳定：actor source57338、bridge读114660、remote生产155610，三者均停止，remote producer未完成；actor diagnostic_digest300ms回应，UI无ACK不阻actor控制。恢复ACK后逐UTF8字节pattern/offset/source monotonic和唯一EOF检查通过，backend全Screen同reference、normalEnded、远端EOF/Close完整，private producer/连接/listener全drain/join。原jump（UTF8+GBK+actor16bytes）再次通过。
- 共享connectioncontrol350ms不能完成，独立connectioncontrol500ms回应，恢复ACK后原control3s内成功。源码client/encrypted.rs的boundedchan.send.await确认HOL；handler callbacks在这之后执行，不能仅改应用Callback消掉阻塞。该失败语义在报告显式oldConnectionControlBlocked=true/limits中，意味着生产SSH复用/混合SFTP+shell+forward仍未验收，不是复用通过。
- 另两条real SSH压力路径Detach及AckTimeout精确匹配理由，均仅撤view，remote继续完整4193280/4892160，actor normalEnded全Screen同reference，同连接control恢复，producer/连接/listener清理。超时3s只用于该受控样例；未改变生产超时或命令队列。禁止把正常cancel当完整有序viewEOF，但底层SSH EOF/Close与actor EOF均完成。
- 3新增负向用例拒remote全部提前完成、bridge超1MiB、假HOL/缺独立连接证明、错releasekind/误关actor/信用8221。30SSHfeaturetests、SSHfeaturestrictclippy、jump兼容与workspacecheck/tsc/Biome通过，Astrolabe新pressure文件无error诊断。all-featureaudit重跑仍原6license/2unmaintained阻塞，sources/bans通过，未ignore/变白名单/升级依赖或锁版本。
- 未完成：生产SSH adapter/每通道credit调度与复用隔离、真实设备/多版本压力矩阵、GBK同条SSH到DOM组合、binary preclassifier/charset切换。此处已确认的同连接HOL必须进入后续设计门禁，不能用headless/受控回环推断真实网络性能。

## S5本地CLI协议与独立Unix socket子进程（非完整生产CLI）

- 新pulse-ipc::cli公共模块：4-byteLE长度+64KiB JSON body，header0/过大在bodyallocation之前拒；Decoder每push最多一frame/返回usedbytecount，任何截断EOF明确失败。boundedwriter防serialize巨大中间Vec，Zeroizing<Vec>所有wire/暂存body在drop清理。JSONRPC2严格object/stringid≤64/method≤96/objectparams，不接受batch/notification/numericID/未知字段，serde拒重复字段/非法UTF8。
- SessionAuth第一消息hello params={protocolVersion:1,token}，固定64hex token用subtle恒定时间比较，expectedtoken/消费hello字符串零化，无Debug/Display或请求内容错误。failure锁死gate，重复hello/id或128requestbudget拒，auth本身不等于Capability/窗格权限，不将method当shell。10新单测/property覆盖所有分块/header/最大body/EOF/JSON subset/版本/密钥/重放/有限额度。
- 新optional xtask cli-probe/mise probe-cli，用既有Tokio nativeUDS作有限可重复验证（不是规划interprocess productionadapter）。随机私有runtime0700、token+socket0600；启动token是两次OS随机UUIDv4组成64hex，不在argv/env/report；客户端和服务端均核验peer UID，secret只经私有token文件读取。实际10个新子进程证明合法hello/readonlyping（含中文emoji）/错误token+版本/握手前调用/oversizeheader/partialEOF/idle500ms总截止/unknown shell.exec拒绝；symlinktoken/文件0640拒，临时目录/连接/listener/子进程全部清理。
- CLIreport强制8失败/成功精确观察序列、10process及所有安全bool，不接受flags-only。2新probe单测验证目录/文件mode与symlink拒；cli-feature10xtask tests、strictclippy、真实probe两次通过，MCP cli.rs/cli_probe.rs无error诊断；workspacecheck/33IPC/93term/45app/210TS、v1五headless/v2两GBKheadless/API isolation通过。
- fulltests发现旧GBK expansion test在close()之后立刻drop driver，让EOF和cancel同时ready，公平select偶发normalEnded而非预期Closed。独立签名72a2bb7先await有限1s close终态后drop，Closed断言不放宽，生产actor不改；再跑完整suite通过。serde/subtle2.6.1/zeroize1.9.0/uuid只新增已锁定依赖边，没有版本升级/newcrate；all-featureaudit仍原6license/2maintenance拒，sources/bans通过未豁免。
- 未完成：生产interprocess端点/WindowsNamedPipeACL与真实跨用户拒绝、clap独立pulseterm入口、共享Capability授权/风险确认、单实例/冷启动--startup-actions/50ms窗口创建、S5三平台验收。这里的ping RTT/原生UDS和Linuxpeer证据不能当上述能力已完成。

## S5真实CLI到Tauri native窗口收据与加载时序（Linux）

- 新显式desktop cli-window-probe/mise desktop-cli-build/probe-cli-window，仅固定probe.window且emptyparams，nativeUDS/token/peerUID准入后在Tauri mainthread创建480×320本地页面，无shell/任意URL/窗口label输入。main page Finished后才启动private服务/三个独立helper（wrongtoken+两次good）。wrongtoken返回拒、窗口数不增加；两次真实窗口visible+runtime注册+pageload+Destroyed全部确认后windowcount回初始，client/listener/private runtime清理。
- 第一版JSONRPCresult等待load和destroy，CLI完整回执346–396ms而servernative9–11ms。改唯一result在真正native注册/visible检查后发，不仅回accepted；page-load与destroy仍独立await。最终sample1 servernative7004us、clientrequest/native8022us、load330244us；sample2 native10680us/client11872us/load333854us。前两个是受控native状态收据，不是compositor首像素/完整terminal可用，不能声称S5生产显示50ms全通过。未运行的客户端拉起/冷启动竞争还没接。
- main-thread回调先检查oneshot接收端是否已取消，build后投递失败主动关闭latewindow；正常/错误都有WindowGuard关闭，page/load/build/destroy各有限deadline。runnerLinuxGNUtimeout40秒监督整个Xvfb/dbus/GUI进程组，parent50秒观察上限，既有DISPLAY也有超时。CLIhelper仅good/denied，mode缺report或与自动benchmark混用拒。defaultdesktop二进制直接--probe-cli-window exit1/cliwindow.validation_feature_required，不启动GUI。
- 两次真实probe（最后修改只报告/negative/监督）通过。2新增xtask negative tests拒假visible/倒序native或load时间/缺清理/单sample，并允许nativeACK早于pageload（不混两个phase）；CLIfeature12xtask/native39tests、两featurestrictclippy、bindings drift与workspacecheck/tsc/Biome通过，MCP cli_window_probe Rust无error诊断。CI新增该native入口，保留oldchannel/v2/GBK/WM。锁文件只多desktop tempfile/zeroize已锁包边，无newversions/CSP放宽；allfeatureaudit仍6license/2maintenance门禁失败。
- 限制：仅Linux/privatefixture nativeUDS，不是productioninterprocess/clap/Capability或跨用户/WindowsACL；当前重建WebView页面约330ms，完整显示与50ms目标仍需独立评估；未测试冷启动/single-instance/真实user窗口/IME/跨平台。主进程配置窗口与spawn同runtime并不等于productionCLI启动流程。

## S5真实进程启动竞争/存活端点保护/崩溃恢复（只读daemon）

- 新mise/xtask/CI probe-cli-startup，复用cli-probe/ownedUnixUDS/token/peerUID和只读ping，nativeGUI仍是独立检查。5个candidate加3个正常认证CLI和1个旧token拒绝CLI，共9个实际新进程。两个候选tokio并发launch，std1.95 File.try_lock原子竞选保证只有一个primary；stdin空/stdout仅PID+role/status，secret不在argv/env/report。
- 额外live-secondary持相同election.lock尝试失败，在退出前后socketinode/token字节完全不变。primary完成2授权ping后受控退出，锁及endpoint清理。随后真正SIGKILL primary，OS自动释放锁而磁盘留下socket+token；recovery新实例拿锁，只有root0700/sockettype+UID+0600/tokenfile+UID+0600/nlink1/64hex检查全部完成才unlink旧endpoint，不误删普通文件/symlink/hardlink。
- 恢复token真实轮换；父进程将旧token保管在另0600文件，由独立子进程实际提交hello，新服务认证拒（denials1），然后新token正常ping成功，证明坏认证只关闭该连接、不关闭恢复服务。server最多4个连接尝试、每个有限accept/serve deadline；临时listener/client/process/runtime均清理。未把uuid/token bytes写入报告。
- 3新增单测拒flags-only、unknownregular/symlinksocket被误删、unsafe/hardlinkedtoken触发前确保旧socket仍在。CLIfeature15xtask tests/strictclippy，真实startup+原CLI回归与workspacecheck/tsc/Biome通过，Astrolabe startup模块无error诊断。无newcrate/lock/version/audit策略变化，既有release6license/2maintenance仍阻断。
- 限制：只读daemon的冷启动election与IPC安全，不是Tauri single-instance插件、GUI未运行时拉起/--startup-actions转交、生产runtime路径/owner文件防TOCTOU、WindowsNamedPipeACL或真实跨用户威胁验收。文件检查依赖当前用户受信任0700目录，不能抵御同UID恶意进程；生产bootstrap仍须单独验收，不暗中改变已有Tauri插件架构。

## S18通用Direct/HTTP CONNECT/SOCKS5组合字节流基础

- 新独立pulse-route库，默认workspace及层级检查纳入。公共Destination/Credentials/Hop/Route/Dialer/NetworkDialer/BoxStream均已实现，返回protocolneutral TokioAsyncRead+AsyncWrite流；只trusted backend配置，不开放desktop/CLI/frontend endpoint。host/port安全验证、IPv6authority括号、ASCII/IDNAupstream要求、8hop深度/8KiBheader、≤30s总deadline，无每byte重置deadline/自动直连fallback。已锁base64.22/zeroize/Tokio边，无newthirdparty版本。
- HTTPBasic显式凭据/ownedscratch零化，HTTP全2xx成功/407auth/其它refused，status与field/control/obsfold严格检查。SOCKS noauth或RFC1929只offer对应method，server改noauth拒且不发credential；IPv4/IPv6/domainCONNECT与boundreply精确消费。header逐byte有界读不overreadapplication首bytes，SOCKS响应相同，UTF8解码不接触tunnelpayload。
- 实际probe-route 5场景：Direct、HTTP→SOCKS、SOCKS→HTTP各256KiB arbitrarybinary echo及8byte binaryprefix逐byte一致，writer半关闭后仍读完整剩余bytes并收到EOF，全部listeners/relay tasks已join。另错误HTTP认证时inner/targetaccept0；错误SOCKS认证时outer已forward1但targetaccept0。报告强制各mode/顺序/bytecounts/prefix/halfclose/acceptforward数值，不接受旗标替代。
- 12librarytests覆盖CRLF配置/secret/depth、同packetheader+binary/IPv4IPv6domainreplyprefix、真实TCP超时/取消未来drop后remoteEOF、DNSname原样透传代理、协议badstatus/header/2xx/authdowngrade/rep/reserved/emptydomain/truncatedEOF。2新增xtaskmetadata回归缺payload/双顺序复用/认证后冒出targetaccept/缺EOF与cleanup明确拒。真实probe+workspacecheck/fulltests/sharedprobe通过（93term/45app/33IPC/12route/10xtask/210TS），库及probeMCP无error诊断；CI注册probe-route跨平台运行但本机只有Linux证据。
- relay任务deadline须借用JoinHandle直到完成，不take后timeout丢后台任务；错误时EndpointDrop abort，成功明确awaitJoin。credential/error不写report；allfeatureaudit仍6license/2maintenance失败，sources/bans通过，无豁免。
- 未完成：真实企业代理/NTLM/HTTPSproxyTLS、SSH与该Dialer同条链协议复用（后续验证）、ProxyCommand/SSHJump hop、route pool/配置secretref/UI/productionCapability入口、三平台及网络慢消费性能。fixtureRFB样式prefix仅任意字节，不声称VNC/RDP互通或S18全通过，SSH共享连接HOL门禁仍在。

## S18同通用代理字节流接真实pinned SSH协议

- probe-ssh-jump复用owned HTTP→SOCKS双认证fixture（两个独立listen/relay），调用pulse-route NetworkDialer返回同publicBoxStream，再直接交russh connect_stream完成协议。proxy不知道SSH，不另造SSH特有代理实现或frontend入口；finaltarget仍新建隔离SSH服务，不访问外部网络/用户凭据。
- 第一次通过两个proxy对SSH目标提交错误expectedhostkey，Pin回调实际拒绝，connection失败且两relay正常join；target连接drained，不能因代理认证成功就信任SSH。第二次正确key与password后session传512任意binary逐byteexact，remoteEOF+Close都收到、clientdisconnect、两proxy/listener/tasks join和SSH连接drained成功。server应用bytes原4449追加512，共4961，原UTF8/GBK/actor/两个SSHhop拒绝矩阵继续通过。
- 1新增routed report negative regression拒缺keymismatch/错误hop或bytecounts/flags-only；33SSHfeaturetests/strictclippy以及真实组合jump通过。已有CIjump现在强制routedSsh，不增加新crate/lock/audit/CSP修改。
- 限制：已说明S18协议-neutral复用在受控SSH上成立，但不是企业HTTP/SOCKS服务器矩阵、SSHJumpHop池/ProxyCommand、proxy链端到端ACK背压或productionroute配置；原共享SSH连接HOL门禁保持。下一步将原有4MiB慢ACK样例显式经此代理链验证，不将512bytes正常echo当压力验收。

## S18真实HTTP→SOCKS→SSH→GBK actor的4MiB背压与三种视图释放

- 新mise/xtask/CI probe-route-ssh-pressure，保留原direct probe-ssh-pressure独立报告。复用同NetworkDialer BoxStream与HTTP/SOCKSowned双认证，每条ACK/detach/timeout压力SSH连接都经过该proxychain，而对照独立control连接仍直连。proxy不理解SSH/VT/GBK也不拥有无限缓存，Tokio copy_bidirectional有界buffer及TCP/SSHwindow保证流控，不改变锁定russh。
- actualwindow-aware远端producer固定4193280 sourcebytes（512×8190），归一化4892160 UTF8，trueactor/registry/v2/credit8220。暂停ACK稳定时actor/reader/remoteProduced三个计数不动，本轮remoteProduced155610<TOTAL，独立连接control响应、同连接control阻塞。恢复ACK后全部raw+UTF8逐byteexact/最终backend画面匹配/SSHEOF+Close/v2EOFACK成功；撤视图或3sACKtimeout两条路径只退binding，会话存活/远端与sameconnectioncontrol恢复，最后完整producer排空与actorEnded。每条disconnect后两proxy/listeners/tasks正常join。
- report强制请求proxyOrder同时匹配mainACK/detached/timedOut且proxyRelaysJoined=true，不能用direct成功报告替代某一proxyphase。1新mode错配回归、34SSHfeaturetests/strictclippy、原direct三路径回归/workspacecheck/tsc/Biome通过，MCP pressure模块无error诊断；新route三路径actualprobe通过。无依赖/lock/CSP/audit变动。
- 仍限制：proxy链加入未改善russh同连接HOL，不能准许生产SSH共享池/SFTP/转发复用；受控回环不能当企业代理/网络设备/三平台性能。用显式routeJSON区分多bufferTCP链也不代表复杂真实网络永不积压；没有生产SSH adapter/ProxyCommand/HTTPS TLS/二进制Zmodem预分类。

## S11可信活窗口之间的会话ownership事务（尚无迁移DOM验收）

- 新SessionRegistry::transfer(source,target,id)可信backendAPI，无frontend/CLI命令，不序列化WindowToken。source须authorized+open、target须open、session Running；samewindow是no-op，capture在途try-lock Busy，不等待/不改healthybinding。迁移仅换Entry owner并撤原binding，保留actor/sessionId/native工作，不发送input/ACK/resize/close。目标重新capture/attach才有新generation。
- compare/marker/repair/capture gate的Arc及历史时间沿迁移共享，不重置限频。capture准入及提交时除owner外还检验Entry Arc身份，A→B→A不会使旧Entry的迟到请求复活；跨迁移捕获已持gate则迁移Busy，旧monitor只按generation清理。原owner已native准入的write/resize不能回滚，接入adapter需先retire本地input/ACK/UI，再明确调用事务与准确重绑。
- 4新增actor-registry回归：有输出时迁移/原window随后close仍继续解析/目标newgeneration bootstrap+输入；旧bindinghandle迟到input不写driver，旧ACK/detach/foreign/closedtarget拒；sameowner/Busy无副作用；ABA/rate锁保留；EOF后拒迁移且原owner不变。最初3项及完整workspacecheck/fulltests（48app/93term/33IPC/12route/210TS）/真实PTY2MiB/v1五headless/v2两GBKheadless回归通过；第4项新增后targeted tests通过。生产Mutex不跨await，测试guard改词法scope通过strictclippy；MCP registry无error诊断。
- 真实native v2WebView、typedbindings drift、nativeclippy-Dwarnings与39nativeunit回归最终通过，但这些是既有窗口兼容验证，不是新跨窗迁移DOM证据。后续还需固定真实跨窗口Channel/DOM迁移、原视图retire/目标容器接收/尺寸协调/用户拖动确认和关闭后orphan授权恢复；目前不能标S11完整通过。不改变audit/第三方deps或恢复serializer门禁。

## S11单个真实PTY子进程跨可信owner迁移（非GUI窗口）

- probe-local-session新增transferred证据/schema5：只调用一次spawn_local启动既有portable interactive fixed Rust helper。源owner capture真实READY屏幕（19bytes），transfer到target并close源owner仍Running；新generation恢复与backend inspect逐cell完全一致，旧completion观察完仍check新binding成功。原ownerinput/ACK拒，新ownerboundinput中文在同helper实际read_line校验，最终PORTABLE_OK中/exit7，rawData连续19→63/EOF帧ACK/native read63及final state/digest/screen一致，所有native workers/directchildjoin。
- 2报告negative回归拒missing/false receipts、twochildspawn、无generation递增/错字节/native退出码/非整数；报告验证在落盘前强制执行。fullcheck/真实PTYprobe与4core迁移tests通过。不是runtime WebView窗口，更不证明跨窗口DOM拖动；窗口owner仅trusted registry token，未声称不存在的公共childPID查询API或process-tree验收。默认任务/Unix CI已有probe-local-session即覆盖新增fixed分支；不新建生产迁移RPC/用户shell。

## S11实际Tauri peer窗口→main窗口的PTY归属与Destroyed隔离

- 扩展原固定peer验证，不新增frontendtransfer命令，不信任JSowner。main与peer均实际WebviewWindowBuilder/runtime install mint token，foreigninput/ACK/repair拒后，将peer既有native SessionId从peer转main；source及target使用请求的v1/v2有序恢复，Rust TerminalProbe逐cell/cursor screen与backend inspect一致后Commit ACK，旧completion不撤销新binding。每次run只transfer既有peer child，不另spawn替代child。
- source.close后等runtime Destroyed revoke caller token AND get_webview_window消失（不能用因transfer已被拒的source state假称销毁）。main新owner的state仍Running；boundinput hidden产生真实child HIDDEN_SEED，无view时解析推进；target再次capture全屏精确，然后显式close/nativejoin/reap。主样例RawChannel/DOM不接peer数据，generation全registry单调跨两run。typed SessionEvidence/TS/总报告s.validate/xtask强制peerTransferChecked；native fixture false拒，xtask新负向拒两证据之一缺失/false/truthy值，旧报告不得冒充此scope。
- 实测Linux v2及默认v1各两headless+两DOM run（每个run都有实际peer迁移分支），两最终报告transfer receipts都true；v2 final gen10/30（新增peer capture造成分配递增，不是跨版本合流）。typed bindings漂移/严格nativeclippy/39nativeunit/fullcheck/fulltests（49app/93term/33IPC/12route/210TS）通过；MCP新probe无error诊断。newxtask报告negative在其后targeted通过。无deps/lock/CSP/audit changes。
- 首次v2运行未生成报告，进程已退出/no lingering children，但旧runner未保存native exitstatus/阶段，根因未知；不是已修复。新增bounded固定阶段diagnostic与runner错误的exitstatus context，无凭据/输出内容，保持原期限不延长；随后v2/v1复验通过但不宣称消除偶发故障。
- 仍未完成目标WebView RawChannel与DOM消费peer恢复、生产多窗口拖动/输入退役/尺寸协调、WebView reload及关闭后orphan授权恢复，S11整体不通过。此处backend消费wire验证只是runtime authorization/lifecycle加真实PTY延续，不能将它当crosswindow视觉/IME验收。

## S11迁移末次capture实际目标WebView Channel与DOM回调

- peer源码窗口的首轮capture仍由Rust消费，但source Destroyed后含HIDDEN_SEED的末次capture改由独立Channel<ArrayBuffer>送目标main WebView。主session/channel未Begin时临时创建独立xterm surface，用SharedUnicode17/query suppression/UTF8 stream与现有v1/v2Consumer；主通道和迁移通道各自queue/ACK/nonce/gen，禁止frame混流。目标DOM每次write真实callback后settle检查实际绘制rows再ACK，不把headless视为DOM。ready回执后计算visible digest，backend exactsameboundary compare Equal才消费一次确认票据；清理只自己的root，不误删随后主surface。
- 新typed probe_transfer_ack/confirm开发命令不收sessionId/owner/目的windowlabel，由current主run的runtime caller+nonce定位固定已迁移peer；permit最多一份。old/foreign/confirmed请求拒绝，guarddrop仅清理自己的generation且不关闭actor，新guard不被迟到olddrop清掉。确认还须backend已送Commit并收到该seq的ACK准入标记；这仍是consumer提交ACK而不是用户看见画面的通用安全证明。driver保持原protocol/ACKtimeout监督，不改credits。明确只接受预期Detached收尾，旧source撤销允许Detached/Invalidated(Replaced)竞态，AckTimeout/Protocol/Failed不能冒充成功迁移。
- mandatory peerDisplayChecked 与 peerTransferChecked分别记录真实targetdisplay/runtime归属。TSfinish必须一次readyconfirm且无query泄漏，reject live/repair/end/mixedcodec/额外Begin/Commit或cancel/confirm失败。native报告false拒，xtask两份报告都须严格bool true，null/string/数字皆拒。实际endpoint负向：wrongnonce transfer ACK、wronggen确认、将peer ACK送主session命令、重复确认均被拒而currentpeer仍正确完成。
- strictCommit门收紧之前的最终v1/v2 actual报告均completed=true、两headless+两DOM、两组transfer/display收据true；全workspacecheck/test（49app/93term/33IPC/12route/13xtask/215TS）/typedbindings/40native/strictclippy/基础probe通过。MCP新native模块无error诊断。新5项TS回归使用actual headless xterm中文屏幕及可控CommitACKpromise，确认不会先于callback ACK，取消/确认失败不当成功；1nativegate回归测试foreign/old/consumed及oldguard不伤replacement。
- 最新strictCommitseq/ACK前置与具体Detach收据收紧后的v2→v1真实复验最终通过（两版各两headless+两DOM的runtime/display收据均true），native40/typedbindings/strictclippy通过；gate回归还直接验证无Commit、无ACK、foreign、old以及consumed确认拒绝。原首次无report的未知风险仍保留，不声称修复。
- 本轮mise run audit仍明确FAILED（6license拒、paste RUSTSEC-2024-0436/proc-macro-error RUSTSEC-2024-0370两项unmaintained；bans/sources过），无新dependency/lock/allowlist/CSP变化，不得打包。后续生产多窗口drag/目标实际输入与尺寸协调、source端DOM退役、WebView reload/关闭后orphan权限恢复、跨平台/IME/用户按钮仍待验证；S11整体未通过。

## S11刷新前可信registry批量view退役（真实reload仍待接）

- 新SessionRegistry::retire_window_views(owner)只允许live window token，短metadata锁原子撤该owner全部bindings；返回实际retired view数（无binding也更换Entry身份）。actor/sessionId/windowowner保持，不sendinput/resize/close，不持锁await；该window关闭后Forbidden，其他window view不受影响。Entry::with_owner共享clone逻辑用于transfer及retire，保留capturegate/marker/repair/comparison时间，不因refresh重置额度。
- 批量retire不try-lock等旧capture完成，而是换Entry Arc身份使已准入oldcapture在提交处被current_entry复核拒绝；sharedcapture gate仍占住额度，所以new capture可明确Busy直到旧工作结束。这是旧Entry身份与绑定准入基础，不是JS navigation认证：runtime adapter还须先更换/废弃旧page nonce/epoch，防尚未准入的old请求在retire之后新开capture。已native准入的输入不能回滚；不添加frontend批量撤销RPC或隐式retry。
- 3新增regression：同owner两个liveview批量撤销/0重复count、旧input/ACK拒，foreign binding保持；无view仍解析output，newgen准确bootstrap，oldcompletion不影响new；持有旧capturegate时retire立刻成功/oldEntry身份复核拒/newcaptureBusy，共享marker/repair/comparison锁与时间保持；closedtoken不能退foreign。fullcheck/fulltest52app/93term/33IPC/12route/13xtask/215TS、真实PTY迁移/2MiBbackpressure与两GBK headless恢复通过，strictclippy/MCPregistry无error。
- 尚未接on_page_load Start/真实window.reload/旧IPC epoch，不能视为S11刷新验收。releaseaudit仍原6license+2unmaintained失败，未改依赖/lock/gate；下一步有限runtime reload样例应把native task与page命令future分离，以免页面消失误关PTY，并等待新page准确Begin/ACK。

## S11真实fixed peer WebView reload与同native actor延续（不是reload页面DOM恢复）

- 既有peer WebviewWindowBuilder的on_page_load记录最多两次Started→Finished，watch只存一份bounded Pages metadata；finished-before-start、重复Started/Finished或第三次导航永久invalid，不能靠重复回调制造fresh page。初次foreign-control脚本结束后，Rust准确capture peer会话，retire_window_views要求撤1个view、oldACK/input拒，然后调用实际WebviewWindow::reload（Tauri2.12.1 public SDK）。5s固定截止等待真实第二次页面加载完成。
- fresh page仍同runtime window token/registered native window，backendSessionState和同SessionHandle仍Running且完整等于刷新前；Rust v1/v2 recapture每cell/cursor与backend inspect exact，newgen>oldgen，oldreceipt必须Detached、晚完成不清newbinding，freshview显式detach只回Detached。随后同既有peer child继续跨窗transfer、sourceDestroyed和main独立Channel实际DOM恢复，child由main native command supervisor持有，peer pagehide不会drop native task。
- 注意这个peer JS一直只执行对main会话的foreign拒绝探针，从未获自己peer sessionId/input/ACK/attach权限；oldpage无capability可复用。这是runtime reload+actor/旧binding失效证据，不是已授权production页面的nonce/epoch隔离，也不是新peer页面的DOM/Channel渲染。本页recapture仍Rust验证器消费，之前已验的目标main DOM证据只能算后续迁移，不可冒充refresh DOM。
- 新mandatory peerReloadChecked加入typed SessionEvidence、TS与xtask两报告收据检查；native false拒、xtask缺/false/truthy值拒。2新增PageTracker负向tests，native42/strictclippy/bindingsdrift/tsc/Biome通过。实测v2和默认v1各两headless+两DOM run（各4真实reload），最终reports completed/peerReloadChecked及后续display receipts true；fullcheck/fulltests52app/93term/33IPC/12route/13xtask/215TS通过，MCP新probe无error诊断。
- 之前目标迁移和Entry retirement也补做explicit encoding-probe实际GBK Window兼容：两GBK DOM run/双坐标/reverse input/新runtime-display receipts均过、40nativefeature/strictclippy/绑定漂移过，普通desktop仍不启用GBK，未新增deps/lock/CSP/audit豁免。当前新增reload的GBK/外部WM兼容尚未重跑。下一步需新页面权限mint/revoke与其真实renderer消费，不开放用户shell/任意导航；S11总体仍未完整验收。首次未知无report风险不宣称修复，releaseaudit仍blocked。

## S11固定peer每页nonce与精确runtime窗口身份

- PageTracker保留最多2次导航和一份watch投影，nonce由backend uuid mint；刷新retire前显式revoke，Started同样清current、保留previous仅给negative probe，Finished才生成新值。注入只在可信builder Finished、local origin和runtime身份检查之后用当前document eval，UUID为规范ASCII；不提供前端nonce getter RPC，不记录nonce明文。当前PageLoaded但未auth/旧nonce未拒都不能标第二页ready。
- probe_session_peer现在需要run nonce+page nonce：Active只能由trustedbuilder一次绑定精确peer WindowToken，重复同token允许refresh，新/foreign/main token或wrongrun都在confirm/取oneshot前拒绝，不能将同label重建窗口当原peer。页面确认single-use。新document真实IPC脚本先尝试previousnonce并要求拒绝，再currentnonce确认；watch还强制previous拒绝标记+当前auth2，不能只看Finished2。旧nonce不复活、不消费newpage许可。
- 2新增nonce单测覆盖explicitrevoke/no-greeting-getter、duplicate/previous/unknown、已加载未认证；1新增实际actor+Active测试覆盖未绑定/foreign/main/newtoken/wrongrun不会耗valid nonce或oneshot，正确peer随后成功，bind不可覆盖。native45/strictclippy/typeddrift/tsc/Biome与真实v1/v2各两headless+两DOM（各4reload）通过；fullcheck/fulltests52app/93term/33IPC/12route/13xtask/215TS通过。
- 精确owner变更后的MCP window_sessions诊断超时，不算通过；此前将长shell与MCP放Promise.all导致fabric取消两条shell（status killed/无有效完成），未据此标通过。确认取消后顺序复验任务完整通过，source/controller注册和types以实际编译/tests为准，不安装语言服务器。首次无report未知风险依旧未声称修复。
- 本scope只授予fixed peer的只读foreign-control拒绝探针，不授予peer自身input/ACK/attach：真实刷新后的画面仍Rust validator，不是授权新页面自己的DOM恢复。下一步把同页面nonce及owner门用于fixed peer只读Channel，native task继续应用所有，旧页面回调不得关PTY。不改变deps/lock/CSP/audit/release门禁，不标S11完成。

## S11刷新新页面自己的只读DOM恢复（固定样例）

- 固定peer窗口改为实际visible 600×400，新doc通过已验的UUIDnonce与精确runtimepeerToken确认身份后，lazy初始化只读480×208 host；rootReact独立，probehost放body且finally恢复root.hidden/移除host与pagehide listener。同原child RESIZE_SEED中文/emoji非空画面，从backend固定peer_id（不接前端session选择）经独立v1/v2Raw Channel输送。普通桌面和生产shell权限未开放。
- probe_peer_view/ack/confirm/done四个typed端点均要求localcaller+run nonce+当前page2nonce+精确peer owner+backendsession owner，no input/resize/close。peer semaphore拒重复捕获（不替换当前view），render共用已验的ordered事务/3sACK/一帧/8212或8220credit、实际DOM write/settle后ACK；只有同CommitSeqACK与sameboundary摘要Equal才触发native单次确认。await后再次检查nonce，detach收据严格Detached，不把timeout当完成。原main迁移端点授权逻辑独立，sharehelper仅处理已授权的owner/id/gen。
- positive render完成只markCaptured，不提前markRendered：新页还必须完成首次ACK前错误nonce/错generation拒、confirm前错误nonce拒以及positive后重复confirm拒，view.finish和清理流程均无失败才调用probe_peer_done。nonce锁内captured/rendered验证避免revoke竞态；Done single-use，Root5sreload wait要求auth2+rendered。新增mandatory peerReloadDisplayChecked贯穿nativeDTO/TS检查/xtask报告严格bool拒绝，既有peerDisplayChecked仍只标后续目标mainDOM，不能混用。
- 首次新增sourceDOM→targetDOM紧邻触发Registry.compare_visible 500ms/session Busy：分类确认session.transfer_compare_busy，而不是改限频/重置owner额度。仅固定probe在source成功后按既有quota sleep500ms，再进行下一次目标compare；原3sACK/5sreload/40schilddeadline不变，不做隐式retry。这再次证明transfer保存comparisonquota。native错误日志只记录数字退出码/cancelled/resize，不记录终端、凭据或nonce。
- 最新v2与默认v1各两headless+两DOM run（每codec4个刷新sourceDOM及4个后续targetDOM）完整报告，实际oldnonce拒和newdocDone完成。首次默认v1末轮native_exit_failed未保留分类，补无敏感退出收据诊断后复验通过；尚未复现/定位，不声称已修复这项偶发风险。46native/strictclippy/typeddrift/7报告负向与fullcheck/fulltests52app/93term/33IPC/12route/13xtask/219TS、baseprobe通过，MCP新peer模块无error。
- 4新增TS是wrapper生命周期mock测试，不算绘制验收；覆盖scope参数/duplicate、pagehide取消在途native和latecompletion、rendererfailure、混版/7s截止清理且不能调用Done。真实绘制证据单独来自WebKitGTK实际probe。first page仍无自己的DOMChannel，live旧页面订阅跨导航、生产任意导航/IME/拖动/多平台、GBK及外部WM最新组合尚待验证；S11总体不标完成，deps/lock/CSP/audit/release门禁不变。

## S11刷新DOM链路的GBK/外部WM兼容与在途许可撤销

- 显式encoding-probe构建发现Active新增peer字段未初始化（默认desktop编译不覆盖此cfg分支），不能视为GBK新组合通过。改为Active::new统一无peer default：owner/nonce/id/visibility与peerNone/peerOwnerNone/peerIdNone/newPageTracker一致；普通固定run显式增加自己的tx/pages，encoding样例不继承peer页面权限。此前已通过的旧encoding版本不当作当前源码证据。
- 修正后实际probe-encoding-window完整GBK两DOM、双坐标与UTF8固定peer新docreadonly恢复通过；probe-window-external真实WM-only最小化/恢复与两headless+两DOM完整reports通过，freshsourceDOM/targetDOM均独立收据。feature测试最初46项，随后最终源码新增2项回归并补default/encoding-probe各48项tests、严格clippy、bindingsdrift/fullcheck通过，MCP peer模块无error。普通desktop仍未开放GBK入口。
- 将Active::peer_view同nonce/window/id门复用于四个readonly端点，compare异步返回后仍调用owned重新授权。2新增真实actor metadata回归：foreign/main/wrongrun/oldnonce拒，page2 admission后导航Started使pendingPageTrackerclone不能markCaptured/Done，registry退役后actor仍Running、正常close/join；no-peer默认run绝无page capability。这是受控授权yield回归，不冒充GUI中用户重复刷新操作。
- CI现有LinuxGBK实际probe后补显式encoding-probe单元测试与strictclippy，防止cfg分支今后只编译默认desktop而漏检查。真实GBK设备、firstpage liveChannel跨导航、生产ViewHost权限、连续任意导航与跨平台仍待验收；最新defaultcore219TS已过，无deps/lock/audit门禁变化。先前偶发native非零退出/无report继续记录风险，未新增豁免或扩大截止。

## S11共享恢复sender的无副作用准入

- 源freshpeer和目标main复用的sendlane现在以application单额度semaphore在attach/capture前try-acquire，metadata slot也在capture前检查；Busy返回不发送frame、不替换当前actorlease、不消费新generation。permit贯穿capture/CommitACK/摘要确认及detach完成，失败按RAII退本次view；capture成功即创建Guard，避免previousgeneration不合法等早期return遗留已创建view。不会close actor/session。
- 2新增native回归各覆盖v1/v2：occupy lane后render Busy且zero frames、oldbinding保持、释放后下一次准确attach generation仅+1；badprevious失败detach自己的lease、permit恢复1、actorRunning/metadataNone、nextgeneration2能正常attach/close/join。default和encoding-probe各50项native/strictclippy通过，实际v2两headless+两DOM sourcefreshDOM/targetDOM及7合成transport测量完整，typeddrift/check通过、MCPsharedhelper无error。前一轮v1/GBK/外部WM实际证据保持，当前新增sender顺序尚未重新跑这三种实际模式。
- 发送准入是元数据保护，不是复用SSH控制通道/提高native窗口吞吐或放宽恢复精度；所有deadline/credit/500ms比较限频与releaseaudit保持。下一步Started可信callback需自动retire已绑定peer窗口的view，以覆盖不经root显式retire的实际reload；生产任意导航仍未开放。

## S11实际导航Started驱动退旧binding（非调用方提前撤销）

- fixed builder每次PageLoadStarted先PageTracker.record撤当前nonce，再按后端当前run nonce与windows索引中的精确peerToken调用retire_window_views。首次Started未授权peer仅允许退0；第二次必须在Started2/Finished1阶段退1个旧lease，缺/重复/0或2count/错误/非Started回执标invalid，不允许仅加载新页/新页DOM成功冒充自动退役。mandatory peerNavigationRetired贯穿nativeDTO/TS/xtask严格bool检查。
- reload验证不再在root显式revoke或retire：capture的旧lease在调用window.reload前仍checkBinding成功，真实Started驱动撤nonce及view，Root等待新页认证+sourceDOM+Done+automaticretired，旧ACK/input拒/完成Detached与新capture精度单独核对，actor和窗口owner始终保留。用于撤销的runtime callback不要求新URL仍local（导航到远端也应撤旧许可），但授予新nonce/权限仍只允许可信本地Finished；no frontend revoke/RPC label。
- 2新增native回归：真实actorlease下oldrun、同label更换token不能退原binding；正确peer退1且重复退0、actorRunning/closejoin正常；PageTracker actualStarted阶段/count/失败严格负向。default/encoding-probe各52native、strictclippy/typeddrift/fullcheck/fulltests52app/93term/33IPC/12route/13xtask/219TS通过。actualv1/v2各两headless+两DOM run（各4真实Started自动退役及freshsourceDOM）完整JSON独立retired/display receipts true，原七组合成测量仍完整。
- 旧页自己的renderer先前仍未授权（它只有foreign-control拒绝probe），旧activelease来自Rust验证器；证明runtime Started对真actor订阅退役，不等于旧页正在绘制liveData时的完整恢复。下一步先补firstpage本身readonlyDOM许可，再真实验证其旧nonce对ownreadonly端点被拒。当前不处理生产任意导航/连续三次reload、跨平台或用户IME，既有偶发退出风险保留，GBK/外部WM最新Started版本尚未实测复跑；release门禁不改。

## S11首次页面曾获自己只读许可后的真实刷新撤权

- firstpage已用当前pageNonce+精确peer token完成自己的只读Raw Channel→DOM→ACK→摘要→Done，mandatory peerInitialDisplayChecked覆盖首次实际绘制。nativeSupervisor在peer注册和greeting后才主动eval首次renderer，不依赖首次Finished与peer_id注册的竞态。当前doc只允许单次view，无session选择、输入、resize/close权。
- readonly_ready仅认证完成的page1或page2，Started清currentcaptured/rendered（不复用旧DOM收据）但保留initialDisplay历史证据；freshdoc依然要求旧nonce拒/新nonce认证、真实新capture/新Done。旧页面确实曾通过ownreadonly gate；freshwrapper除unknownnonce/错generation/duplicate之外，明确用previousPageNonce调用自己ACK/confirm端点并要求拒绝。Root500ms节奏分别保留first→fresh与fresh→target共享比较额度，不增加deadline。
- 新native回归firstpage真实准入与currentcapture/render收据清零、oldnonce拒/新doc须重新capture后Done；新TSwrapper回归previousnonce确实调用ownACK/confirm负向，mock仅测试调用约束。nativeDTO/TS/xtask独立initialDisplay bool收据拒缺/false/truthy，不能把freshDisplay或targetDisplay当首次画面。
- latestv1/v2各两headless+两DOM run（三阶段各4次firstDOM/freshDOM/targetDOM）完整，actualStarted retired及initial/fresh receipts true；default/encoding-probe各53native、strictclippy/typeddrift/fullcheck/fulltests52app/93term/33IPC/12route/13xtask/220TS通过，MCP reload模块无error。
- firstpage是只读snapshot，完成后该Channel/surface被释放；reload瞬间Open lease仍由Rust validator持有，并非旧DOMliveData正在持续接收。下一步需要将firstpage订阅保留至navigation，让RuntimeStarted撤销真实renderer lease；不能提前宣称liveData跨reload完整验收。最新GBK/外部WM此firstpage扩展未重跑，生产/IME/跨平台和偶发非零退出风险仍保持，deps/CSP/audit不变。

## S11保留readonly renderer的前端续流与就绪握手（native live relay待接）

- TransferViewProbe增加显式validation-only live配置，bootstrap ACK/摘要后保留surface、Unicode provider、查询屏蔽与UTF8Stream，允许有界Data及纯Marker；snapshot默认仍拒Commit后的任意帧。Marker sink只等DOM绘制，不送VT、不finish pendingUTF8或flush同步更新；同seq ACK完成之后才调用带revision/source边界的marker摘要确认。取消/确认失败丢弃本代，不重用surface或清理新代。
- 6新增v1/v2实际headless回归：半emoji跨Data/Marker保持（含已写空格），暂停MarkerACK时不能确认，续字节完成🌍，DSR查询不会产生重复onData响应，lateData/marker确认失败不能被当成功；默认snapshot续流继续拒绝。6项是headless数据面证据，不是native live订阅跨reload证明，尚未启用第一页面live模式。
- 默认snapshot真实回归首次报peer_initial_display_deadline，未通过。实现审查确认Finished/greeting可能早于React effect安装__PULSE_PEER_VIEW__；新增单document resolver握手：Finished script先等已有hook或一个resolver，installPeerBootstrap先发布hook、清resolver再唤醒，无轮询、重试、nonce getter或deadline扩大。3单测直接读取/执行actualRust静态脚本，覆盖提前/迟到装hook、跨doc隔离和重复安装拒绝。复验过并不证明那次timeout的全部根因已定位。
- latestdefault v1/v2snapshot实际first/fresh/targetDOM及53native、fullcheck/typeddrift/tsc/Biome与229TS通过，CI原有actualprobe含新握手。前次firstpage构建GBK和外部WM实际组合也已过完整独立initial/fresh/retired报告；本次新的前端live/握手后这两模式未复验。无deps/lock/CSP/audit改变。
- 下一步把显式live接到native application-owned订阅监督：bootstrap确认、Data/Marker确认、Started撤销与oldviewjoin须分开；不能在窗口卸载时取消PTY。snapshot或headless通过不等于oldDOM仍在消费输出时的真实刷新验收。S11整体/生产/跨平台仍未完成，既有偶发退出风险保持。

## S11实际旧DOM持续订阅→Data/Marker确认→Started退役→新页恢复

- firstdoc用render_live保留自己的真实lease与DOM，不再以bootstrap结束后创建的Rust验证器lease代替旧renderer。Root仍持有同一个nativePTY task/actor；peer页面命令只持有view。bootstrap实际DOM行投影/write/fence完成后ACK/摘要Equal，peer confirm标captured、ready调用single-use Done，frontend不等native订阅结束才显示，而是保留surface/查询屏蔽/UTF8Stream直到pagehide或native完成。
- Root向本renderer generation发固定hidden输入，同一child输出HIDDEN_SEED中文/emoji。native受一帧/8212或8220credit转发Data，只有metadata本seqACK游标达到backend parsed边界才请求pure Marker；Marker必须已有Data本seqACK、自己的seqACK、sameboundary摘要Equal与本页nonce/owner，独立peerLiveDisplayChecked不可用initial/fresh/targetDisplay冒充。snapshot默认仍拒continuation。每次Data/Marker ACK前还检查实际DOM所有行text与buffer projection，不仅依赖模型摘要/RAF机会。
- metadata锁贯穿同步ACK准入与ACK状态发布：credit释放可能在其他runtime线程唤醒nextFrame，避免Marker误见lastDataAck=false。live Data credit允许后续ACK，不授予重复bootstrap/未ACK Marker确认；Marker单次sender独立重新arm。依旧保留原500ms/session比较quota，first→live、live→fresh、fresh→target仅样例显式按quota节奏安排。
- nonce锁内每页一次claim（两doc最多两在途请求）限制等待数量，Started重置当前claim/capture/render而保留旧实测收据。native live监督等actualStarted、registry退役后严格Detached receipt，先释放Guard/sender/peer permit再mark_navigation_joined；freshcapture有500ms固定join上限，await后重验nonce，不能抢旧lane或复活旧请求。Rootreload之前仍check旧renderer gen绑定Open；newdoc以自己的Raw Channel重捕获后继续可信跨窗迁移、输入与有序EOF。oldnonce/gen仍拒，view消失不close actor。
- 新3native回归覆盖claim per-epoch/重复与oldnonce、join真正完成前阻塞、live DataACK不等于Marker确认权限；新3TSwrapper回归native订阅未结束时已ready/Marker、pagehide只清oldsurface与latecompletion、native提前结束或后续失败不能伪成功。此前首次actualrun四次旧live/刷新/迁移已走通但main terminal.render_timeout，整组未通过；保留1sdeadline，只补安全frames/visible/connected分类，之后v2和默认v1完整复验通过，未声称根因已定位。
- 最新actualv1/v2各两headless+两DOM run（每codec4次oldDOM Data/Marker/Started/fresh/target），mandatory live/retired收据true、seven运输测量完整。default/encoding-probe各56native/strictclippy、fullcheck/fulltests52app/93term/33IPC/12route/13xtask/232TS、baseprobe/typeddrift通过，MCPshared native sender无error。随后actualGBK与外部WM此live版本均完整通过，独立live/retired/fresh收据true、typeddrift保持；next验证未ACK导航场景。
- 这份controlled sample在Data全部ACK且Marker完成后发起reload，不证明未ACK洪流/持续网络输出期间的bootstrap摘要及重绑策略，live relay只支持Data和single Marker，不把缺少Checkpoint/End支持计完整。firstdoc/currentnonce只读权限仍非production ViewHost，生产/跨平台/IME与S11整体未完成；所有deps/lock/CSP/1srender/原helper deadline/audit门禁不改。

## S11有限未ACK Data期间的真实导航与恢复

- 旧页已确认一次live Marker后，只允许当前run/page nonce及live generation设置ACK hold；真实hold_ready端点先检查runtime owner、固定session和binding。第二条固定hidden输入产生Data，本seq/end连续100ms保持未ACK才记peerPendingAckRetired。PeerAckHold在pagehide仅reject，绝不靠假ACK释放credit；旧DOM的预期取消不误伤新页，setter清理按函数身份保护新代。
- actualStarted撤旧lease，native严格Detached并释放sender/peer额度后join，新页不能抢占。后端无视图继续解析第二条有限输出；两条HIDDEN_SEED可见且SessionState连续20ms稳定后才允许freshcapture（1500ms固定上限）。必须比较排空后的expected而非误假定pending前状态不变。该排空门是有限样例监督，不解决连续网络输出时bootstrap/latest摘要策略，也不算洪流压力验收。
- 新3TS回归：hold只拒绝退出、不返回credit，新旧代独立，真实wrapper挂起ACK及错误nonce拒/oldcancel清理。原snapshot参数fixture改在native调用进行期间测试，禁止清理后再发ACK。新native回归验证live→hold→pending→Started→oldjoin→refresh许可顺序；DTO、生成types、TS与xtask严格bool收据均强制。
- 后续移窗样例原contains已有HIDDEN_SEED会提前判新输入完成；改为seed count增加且parsedBytes推进，不放宽成功条件。新增静态relay失败分类及old supervisor/driver receipt/permit/join阶段诊断，不记录nonce、终端或内部错误字符串。
- 多次整组失败（旧join、main relay/可见性查询）保持风险记录。期间工具ENOSPC：数据盘31GiB空闲，但共享/tmp 16GiB tmpfs已满，主要为其他项目；未删除其他项目文件，仅cargo clean核实的PulseTerm临时upstream target缓存约319MiB，保留src/tests/manifests/licenses与测量报告。后续使用0700项目target/probe-tmp.*运行时TMPDIR避共享压力，Sandbox/CSP不改。不能由空间恢复与复验成功推断所有先前失败根因已修复。
- 最新Openbox实际最小化整组及默认v1/v2整组各完成2headless+2DOM、pending/live/retired独立收据true，oldrelay→Detached→释放→join阶段齐全。default/encoding-probe各57native、strictclippy/fullcheck/fulltests52app/93term/33IPC/12route/13xtask/235TS/typeddrift通过，MCP reload模块无error。随后pending版本GBK与外部WM实际兼容均完整通过，独立pending/live/retired收据true，绑定漂移保持；所有原1srender/query、500msoldjoin、helper/ACK期限及deps/lock/audit门禁不变，S11整体仍未验收。

## S11捕获摘要与最新状态校验分离（受控输出推进）

- 最小双codec回归证明：Begin/Commit未ACK时actor从a推进为ab，captured VT仍精确还原a；原compare_visible必须Stale，而不是图像损坏。按offset1续流b后才匹配latest。不能靠冻结无限网络流或放宽latest检查解决bootstrap确认。
- SessionBootstrap新增digest，由session_stream::prepare在同一actor turn计算；AttachmentHandle仅保存boundary/digest和失效status的只读sidecar，不进v1/v2帧、JSON或source/display信用。SessionRegistry::compare_bootstrap在当前owner/gen/status下确认captured projection，格式与共享500ms/session额度保持（foreign/错gen/坏格式不耗额度）。Equal不代表queued Data已绘制，不清任何risk；latest compare/repair保持原语义。4新增app oracle回归覆盖授权/格式/限频共享/篡改/撤销、capture后已出现非零LINES/REP风险保持、换绑/移窗缓存不可跨代复活，以及恢复加载期间连续输出有界。
- 桌面固定端点仅在本Commit seq ACK与can_confirm通过后选择captured比较；Marker选择latest比较。新增双codec native分派回归证明capture后actor已推进时Commit可Equal，同旧边界Marker必拒。native bootstrap总3s内再等本页Done/initial_display，才转发queued Data：避免confirm RPC已在后端唤醒sender，而前端仍comitting时收到Data。新native屏障回归capture本身不能放行、Done后放行、导航旧nonce不能复活。
- 实测把原第一条hidden输出移到已捕获但Begin尚未发送时，等source parsed cursor确实超过captured边界后才发送Begin；不增加种子数量。firstdoc实际DOM还原旧图像并确认cached digest，本页Done后消费queued Data，随后Marker校验最新状态；原latest-only bootstrap实现会拒。新增peerBootstrapAdvanceChecked贯穿nativeDTO/generated TS/report严格bool检查，不用initial/live标志冒充。
- v2与默认v1/显式GBK/外部WM四种实际组合均完成2headless+2DOM，独立advanced/pending收据true，查询屏蔽、旧nonce/ACK、未ACK退役、fresh/targetDOM、EOF及运输测量保持。59native两feature/strictclippy、54app及原binary headless/GBK恢复、fullcheck/tests93term/33IPC/12route/13xtask/235TS/typeddrift已过；随后仅新增风险保持测试，最终app57与check/API隔离补验。MCP comparison/stream模块无error诊断。
- 新双codec慢bootstrap压力回归：真实有界Channel producer发送64×4096字节（共256KiB，中文/emoji）。Begin未ACK时parsed暂停且少于总量、producer未完成；在backend已推进时captured确认仍Equal。恢复ACK后每Data offset连续、256KiB逐字节一致，最终模型digest与actor相同。不算GUI/SSH洪流性能验收。
- 显式GBK headless探针追加缓存oracle：源计数/UTF8计数不同且半字符待解码时exactcaptured可Equal；source相同而UTF8游标错误必须Stale，共享500ms比较额度保持。bootstrapOracleChecked在TS报告消费者要求严格true，两份真实actor→v2→headless恢复仍通过。普通pulse-app与desktop未添加GBK生产feature或新依赖。
- 真实SSH压力追加“bootstrap未ACK且远端已经生产”阶段：先启动受SSH发送窗口约束的4,193,280字节GBK producer与读桥，不先完成bootstrap；700ms后采样再等待300ms，actor/source、bridge read和remote produced三计数必须完全不动且未达总量。此时cache Empty捕获确认仍Equal，随后bootstrap ACK及原Data暂停/恢复、逐字节UTF8、EOF、detach/timeout清理继续执行。
- 直连与HTTP CONNECT→SOCKS5→SSH实际报告均完成：bootstrap paused actor 57,330、bridge 106,478、remote 155,610字节，最终source4,193,280、credit≤8220；独立bootstrapWhileRemotePausedChecked与3个bounded counter为mandatory，缺值/false/truthy/0/总量负向拒绝。38项ssh-probe单测/strictclippy、两实际pressure probe、fullcheck/fullcore235TS通过。一次错误test filter得到0tests未计通过，已完整38项补验。
- 该网络结果仍是隔离回环/测试CA凭据/固定producer，不是任意网络设备或GUI持续洪流；同connection控制通道HOL仍必现，独立连接响应、各条route/relay/EOF/清理门保持。未添加依赖/allowlist豁免，未ACK导航检查点已独立签名提交 `e39c5fa`，捕获摘要/网络验证保留独立提交边界，未擅自无签名提交。
- 当前only fixed UTF8 peer开始前有限一条输出推进，不是连续洪流/跨平台生产验收；oracle为FNV可见投影而非安全认证或history/hidden/fullstate校验。上游5行getter、所有wire版本/信用/超时/CSP/依赖与发布审计门禁均未改。未ACK导航检查点已签名提交 `e39c5fa`；本节新代码/记录与它分开提交，签名要求不绕过。

## S11刷新后的只读页面持续输出与迁移交接

- fixed peer第二个document不再只用snapshot render；同readonly nonce/owner/fixed session权限下render_live保留renderer、UTF8流和查询屏蔽，等待本页Commit绘制/ACK/捕获摘要与single-use Done后才转发Data。前端keepLive同时支持初始/刷新页，刷新页不能调用仅用于initial场景的ACK hold。
- 固定Root在refresh后输入额外hidden，必须parsedBytes推进、seed count增加且本seq Data ACK达到当前边界，才请求pure Marker。fresh自己的Marker ACK/latest摘要才设置refreshed_live_display，旧initial live_display不能替代；随后Root显式detach，native确认同binding Detached、释放shared sender及peer额度后才mark_refreshed_joined，新capture/移窗必须等待此收据。
- 初始document仍由真实Started驱动退役；fresh不能因为started已为2误用旧navigation事件而立即退出。live监督器进入时固定document role，fresh在wire关闭时重新检验当前nonce，再要求真实Detached receipt；会话一直Running。等待Done/3s bootstrap、5s live、500ms freshjoin以及1s渲染/Marker/数据检查截止不变。
- peerReloadLiveChecked为独立mandatory bool贯穿RustDTO/generated TS/总报告/xtask两runtime检查，missing/false/truthy拒。新的native负向门证明fresh必须自己captured/Done/Marker才join，oldnonce与重复Marker/join拒；3项TS回归包含fresh保留DOM、旧nonce ACK拒绝/initial hold阻断、pagehide与迟到失败隔离、Marker失败不能冒充native退休成功。
- v2/default v1/显式GBK/外部WM真实组合均完成两headless+两DOM，fresh/pending/advanced独立收据true；60native两feature/strictlint、57app、238TS、fullcheck/fulltests/drift已通过，MCP reload模块无error。依然固定有限输出，并非任意第三次导航、洪流GUI、跨平台或生产ViewHost验收。
- 后续准入审查发现page claim早于peer/shared lane额度，Busy请求可能消耗恢复机会。已调整为等待oldjoin/refresh许可后，先授权nonce、取得两个额度，再消费single-use claim；lane贯穿捕获/发送/退役，不再次竞争。native回归覆盖initial与fresh坏nonce、peerbusy、sharedbusy后合法claim仍成功且permit全部释放，已获claim仍不允许重复。最新61encoding native/strictlint/check已过，最新双版本实际endpoint组合已通过两headless/两DOM与61default native/strictlint/drift；没有以此前GUI通过替代最新源码验收。
- 最新移除了fresh捕获前的“第二条输出排空且20ms状态稳定”等待：只等old relay Detached/join与页面授权，记录source下界不要求latest停住；fresh也在已捕获但Begin未发送时启动固定hidden输出，必须parsed游标推进后才标refreshed_bootstrap_advanced。fresh Marker成功还要求该独立内部收据，不能用initial bootstrap_advanced替代；nonce/claim/重复推进拒绝已纳入native回归。新页旧图像可见投影摘要确认后queued Data继续，再额外输入后以latest Marker确认并detach/join。
- 最新无稳态源码v2/default v1/显式GBK/外部WM四种实际组合均完成两headless+两DOM和fresh/pending独立收据；61native两feature、strictclippy、57app、238TS、fullcheck/fulltests/drift通过。仍固定有限输出与最多两页，不能称为无间断网络/GUI洪流或任意重复刷新验收；1srender/Marker、3sbootstrap/ACK、5slive及500msjoin截止未增加。

- 未ACK导航检查点已签名提交 `e39c5fa`；后续已验收变更保留独立签名提交边界。没有新增shell入口/输入权限、wire opcode、依赖或审计豁免。

## LOCAL-01/02 Unix自动Shell/Profile与启动字段基础

- 新公开`pulse-pty::profiles::{ShellKind,ShellCandidate,AutoProfile,AutoProfileCatalog,discover_unix_shells}`。Unix读取 supplied PATH/SHELL快照与常见目录的文件元数据，不执行候选或读取用户Shell配置。preferred已识别absolute shell优先，PATH顺序每kind选择第一份；目录/无执行位/相对及空PATH跳过，最多64组件/64KiB PATH/4KiB程序/5kind。执行位只是候选检查，不等于有效用户access/ACL/noexec判定或程序认证。
- 自动身份按kind稳定，refresh换路径保留hidden；缺失shell保持profile但unavailable，不准构造launch；remove记录deleted且重装、PATH变动及tombstone恢复后不自动重生。reconcile先验预算/duplicate，再原子改变metadata。此为in-memory backend模型，不是持久数据库/完整用户Profile/默认选择或窗口RPC。`sh`调用名保留，不能canonicalize到bash/dash改变Shell语义。
- `PtyCommand`新增pure validate，所有spawn在native分配前检查NUL、空env键/含equals的键、空cwd，最大128argv/128override/128KiB显式字段（含终止符）。保留空argv及最后env覆盖，Debug全部程序/参数/cwd/环境键值脱敏。继承环境、实际目录存在及OS argv上限仍由native层处理，底层native错误仍须adapter脱敏。
- 新9项profile回归覆盖private0700元数据fixture没有executed副作用、PATH/preferred优先、目录/执行位/relative过滤、symlink调用名、NUL/预算与隐藏/删除/恢复/atomic规则；4项command回归覆盖分配前拒绝、边界一字节、空参数/duplicate环境与无敏感Debug。pulse-pty共16项单测及严格clippy通过；xtask report新增missing/false/truthy/预算/exit/platform/schema负向门。
- 新`mise run probe-profiles`已注册xtask及Unix CI，不把登记当跨平台证据。实际Linux由/bin:/usr/bin元数据发现sh、以固定`exec "$@"`转发字面argv到自有Rust有限helper，核对TTY/cwd/env/中文emoji输出、exit7/EOF/reader join/direct-child reap，以及真实snapshot下hidden与deleted/tombstone恢复。候选发现本身不执行`--version`或其它命令，报告不含本机路径/环境值/输入内容。输出预算4096、helper3秒自退、worker12秒监督；固定脚本不接受任意CLI参数或远端输入，也不声称一般Shell树清理。
- 首次12profile/native单测与实际probe后严格lint发现let-chain/from_ref/print-newline问题，均修正且没有豁免；最新16PTYtests/strictlint已过，完整check/test/真实ProfileProbe/UnixPTY/portablePTY/actor/probe组合已通过（16PTY、57app、93term、33IPC、14xtask与238TS）；实际profiles.json所有独立收据true、shellCount=2、exit7/output23bytes。无manifest/lock/第三方依赖改变，audit仍原6拒license及paste/proc-macro-error两维护公告失败，bans/sources通过，发布禁令不变。MCP profiles模块Exact无error；首次引用缺path的Unknown未算通过，补path后Exact引用。
- LOCAL-01/02未全验收：Windows PowerShell/cmd/Git Bash/每发行版WSL、用户自定义命令参数/外观/默认profile、存储/UI、macOS登录及Windows注册表环境刷新均未实现或实测。锁定portable-pty只在默认program路径内部设置login argv0，显式profile未提供独立argv0覆盖；不能用`-bash`当executable或默认builder冒充自定义登录支持，本轮未patch依赖。

## LOCAL-01 WSL UTF-16LE列表纯解析基础

- 新公开`pulse-pty::wsl::{decode_wsl_list,WslDistribution,WslListError}`，接受可选LE BOM、LF/CRLF及空分隔行，不提供UTF8猜测回退或损坏surrogate替代。输出64KiB、最多64发行版、每名256 UTF16 units；坏BOM/奇数字节/无配对surrogate、NUL/control/仅whitespace及exactduplicate拒整份输入，错误不带stdout/name。名字保留case、空格、Unicode与出现顺序；不伪造registry GUID或作删除稳定键。
- `command`只将`--distribution`和完整名字作为两份OsString参数，不拼接shellsource、执行程序或加入其它选项，复用通用PtyCommand预算；Debug脱敏名字。option-like名字的纯argv回归并不证明wsl.exe的实际参数解析行为，这一点仍需Windows实测。
- 6项新回归覆盖BOM/Unicode/各发行版、损坏编码、全snapshot拒绝、字面选项/标点与Debug、边界及2000个seeded随机UTF16 snapshot。22项PTY、57app、238TS与完整check/test、LinuxProfileProbe、严格clippy通过；MCP模块Exact无error，未新增依赖/锁/审计豁免。没有运行Windows/WSL，CI只是原有三平台unit登记；native discovery必须先验证owned wsl process的成功退出、stdout预算与时限，纯解析不能认证数据来源。

## LOCAL-02 本地启动环境defaults与CLI路由元数据隔离

- 新公开`pulse-pty::launch::{prepare_local_command,LocalSessionEnvironment,PaneEnvironment,LocalLaunchError}`纯准备层；字面args/cwd保留，提供TERM/COLORTERM/TERM_PROGRAM/版本defaults，普通profile env及TERM/COLORTERM显式值优先。runtime身份4键的所有ASCII case别名覆盖/删除拒；backend version/id/socket限制格式/长度/NUL，错误与Debug不暴露值。context不是authorization token，不创建CLI服务或会话窗口绑定。
- PtyCommand新增env_remove，在继承环境/defaultTERM之后、显式env之前应用；清旧PULSE_PANE_ID/PULSE_SOCKET，只有backend当前pair才重新设置，不设置空字符串冒充删除。显式env覆盖/删除键合计最多128项、encoded byte均计128KiB。原通用PtyCommand不强制runtime政策；只有调用prepare的local-profile adapter走此行为，不能把它当生产IPC已受保护。
- Pwsh按trusted ShellKind前置-NoLogo（已有leading大小写同flag不重复）并默认更新检查Off，不解析/拆分script参数，profile普通env仍可明确改更新检查。此为纯argv/env回归，不是pwsh/PSReadLine/Windowsnative验收，更未实现自定义login argv0。
- 7项新环境回归覆盖defaults/字面argv/cwd/profile覆盖、无CLI字段、backendpair与Debug脱敏、reservedcasealias阻断、坏context、pwsh与预算、继承删除键校验/先删后设；29PTY/57app/238TS与完整check/test、strictclippy/真实ProfileProbe和actor2MiB授权/恢复兼容通过，MCP launch模块Exact无error。
- ProfileProbe schema2要求launchCount2、terminalDefaults/staleCliCleared/backendCliPair等独立strictbool收据；父worker通过process.env注入固定旧pair，不修改进程global环境，两个真实sh→finite Rust child分别断言旧pair不存在或被backendpair替换、全部终端字段与literal env/cwd/argv一致，exit7/EOF/readjoin/directreap均通过。字段值不入报告。新schema负向测试首次仍用schema2作为错误值失败，已修为旧schema1与单launch明确拒绝，没有放宽校验。
- 前一版本profile/WSL已补actualv2两headless+两DOM与61native/bindings/strictclippy兼容，不能当本轮env_remove的新GUI实测。最新本轮native编译/61tests/strictclippy已通过；无新依赖/锁/底层补丁或audit豁免。runtime metadata继承政策尚未接生产Profile服务/GUI/持久存储，Windows/macOS与一般进程树仍未验。

## LOCAL-02 Unix公开登录builder可行性实测

- 锁定portable-pty0.9的public default builder通过自身env SHELL选择程序，Unix as_command设置`-basename` argv0。新`mise run probe-login`在0700的owned project目录hardlink自有xtask finite binary，显式SHELL指向该文件，真实slave执行后helper断言精确`-pulseterm-login-probe`且argc1/TTY，exit7/EOF/readerjoin/direct-child reap全过。helper3秒自退、worker12秒监督、输出≤4096；不读取用户或系统shell rc，也不把该实验当startupfile/macOS/交互Shell验收。
- public args默认builder会panic，新增expectedpanic回归验证确实不能附带profile参数。missing-SHELL仅调用公开get_shell，观察选出的程序不同于requested path，不执行回退shell；report必须missingProgramFallbackObserved=true和customArgumentsSupported=false，避免success代表完整login。两项报告/公开API负向测试拒missing/truthy/wrong平台/非零argc/假定支持customargs。
- 先前“没有独立argv0覆盖API”的限制仍适用于完整显式program+args，但不能据此否认公开builder的无参数SHELL选择可行。生产完整选项：最小Unix argv0设置API补丁（保留显式program原路径/fail-closed，独立参数）、已知Shell登录参数（改变$0语义，generic POSIX shell需要单独支持矩阵），或暂缓完整登录。后续用户已选择 ADR-030 登录参数方案，不引入补丁；此段保留公开 API 实验背景，不以无参数例外降低 LOCAL-02 范围。
- 新xtask→已锁定portable-pty内部依赖边，Cargo.lock只多一dependency字符串，没有新包/版本。任务/CLI帮助/Unix CI已注册，Linux实际login.json selectedProgram/loginArgv0/exit/EOF/join收据齐全；完整check/test29PTY/57app/238TS、严格clippy、原ProfileProbe均过。最新两项xtask回归全量16项补验；MCP新模块诊断请求60秒超时，未计通过；未安装或重启语言服务器。全featuresaudit仍原6license/两维护公告失败，无新豁免。
- 本地环境prepare提交后actualv2 WebView也已通过两headless/两DOM、fresh恢复/输入resize/旧ACK/EOF与bindings漂移兼容；它不是生产ProfileGUI验收，Windows/macOS未实测。

## LOCAL-02 显式登录参数（ADR-030）

- 用户明确选择登录参数方案，不 patch portable-pty。公开 `pulse-pty::launch::prepare_local_login_command` 只为可信 Unix Bash/Zsh/Fish 分类请求登录；Bash 前置 `--login`，Zsh/Fish 前置 `-l`。已有 leading `-l`/`--login` 不重复，后续参数可能是脚本数据，不扫描、改写或拆分。选定程序/cwd 保留，复用 defaults、CLI 身份防伪和最终参数/环境/字节预算。generic sh、pwsh、未知类型和非 Unix 自动登录请求稳定拒绝。没有使用默认 builder 或程序选择回退。
- 首次35项PTY回归通过，但真实参数登录探针失败；复现 `bash -l --noprofile --norc` 的长选项顺序错误，改用 `--login` 后真实探针通过，输出断言、时限和启动文件隔离均未放宽。用户显式提供的无效选项顺序不自动重排；用户选项还可能改变登录状态，纯构造不作任意 profile 登录成功保证。
- 新 `mise run probe-login-arguments` 及 Unix CI 独立登记，报告 `login-arguments.json` 不得以原 argv0 实验代替。在0700 owned目录，实际 Bash 两次原生PTY分别断言普通非登录/参数登录状态、TTY、script `$0` 和中文/emoji/标点字面参数、终端环境、exit7、输出EOF、reader join、直接子进程回收；missing 显式程序真实spawn失败，未执行其他 shell。Bash 使用 `--noprofile --norc`，清除 BASH_ENV/ENV/SHELLOPTS/BASHOPTS 继承键，不读取用户启动文件；4096输出预算、worker12秒监督。此证据不等于系统/用户登录文件正确加载。
- 六项新纯回归覆盖三种known shell构造、leading选项复用、未知/非Unix拒绝、预算与身份防伪、非UTF8参数保留、坏命令先拒；报告负向回归拒绝 truthy/缺原生收据/argv0方法替代/错误模式和计数。Linux真实 Bash 已过，Zsh/Fish仅有纯构造证据；macOS、Windows及生产Profile默认选择/持久存储/UI尚未验收。`$0` 不保证 `-` 前缀，与 Terminal.app/iTerm2 不声称完全等价。完整 check/test、35PTY/57app/238TS/17xtask、严格clippy及原ProfileProbe、公开argv0实验、2MiB actor授权恢复兼容通过；MCP launch模块 Exact 无error。依赖/锁文件/审计规则未变。

## LOCAL-02 Profile平台默认与显式模式

- 新公开 `LocalShellMode::{PlatformDefault,Plain,Login}` 和 `prepare_local_profile_command`，平台默认仅在macOS为可信Bash/Zsh/Fish请求参数登录，其余分类和平台保持普通启动，不猜未知命令的能力。显式Login复用unsupported拒绝；Plain不添加登录参数，也不剥离用户已有flags。三种模式共用runtime身份与最终预算，不变化选定程序、cwd或字面参数，不创建会话授权或默认profile持久化。
- 原真实Sh ProfileProbe已改用统一profile入口。参数登录探针schema2要求三个原生launch（Plain/PlatformDefault/Login）、platformDefaultChecked严格true、platformDefaultLogin与当前OS一致；Linux实际参数计数6/6/7，Bash的非登录/默认非登录/显式登录状态、literal argv/终端环境/EOF/回收通过。旧schema1/两launch、缺default收据和truthy值拒绝，仍保持12秒监督与4096输出预算。
- 首次新增default负向测试暴露其收据未强制校验，已补strictbool和缺失/truthy/旧版本拒绝，不放宽测试。新增三项纯模式回归及完整38PTY/57app/17xtask/238TS、check/test、strictclippy、实际ProfileProbe/三launch/2MiB actor兼容通过；MCP launch模块Exact无error。macOS默认只取得policy纯构造/CI注册证据，不声称本机跨平台实测；Zsh/Fish实际启动、系统/用户启动文件与生产Profile/UI/存储尚待完成。依赖与发布审计规则不变。

## LOCAL-02 参数登录Profile到真实Actor与注册层

- `mise run probe-local-profile-session`及Unix CI注册：可信Bash+显式Login，`--noprofile --norc`和清除BASH_ENV/ENV/SHELLOPTS/BASHOPTS；固定bootstrap先断言参数登录和终端defaults，再仅exec自有finite Rust helper，绝不插值用户程序/脚本，不读取用户启动文件，不新增生产IPC。literal helper argv/cwd/env/TTY沿用原portable helper强断言。
- 抽取原 `wire_streaming_command` 正向注册路径，旧probe仍用原direct helper。两条实际路径均验证有序capture/data/end、正确owner/generation的输入及ACK、单帧信用9000内、暂停ACK解析不推进、恢复后EOF在ACK后完成、原生exit0和join。追加Unix应用固定X burst精确2097152计数，缺失或重复一字节也失败；ConPTY可能经console重绘，Windows仅记录观测值不扩大未实测断言，Unix profile收据要求burstExactChecked严格true。
- Linux profile实际source解析2097221（含控制序列和TTY echo），credit peak8212/limit9000，固定burst2097152，Rust重放最终PORTABLE_STREAM_DONE。它不是目标WebView DOM、真实用户启动文件或持续生产shell验收；仅直接子进程不作进程树保证。
- 原Profile/旧actor兼容、新native 2MiB路径及完整check/test/probe/bindings-check通过；38PTY/57app/18xtask/238TS/61native、strictclippy和MCP新probe模块Exact无error。新报告负向测试拒绝truthy/缺native收据、错误renderer、超信用和burst差1字节。审计实际exit5：advisories与licenses仍FAILED，bans/sources通过，仍为RUSTSEC-2024-0436/0370；未添加豁免或修改依赖。

## LOCAL-03 固定参数登录Profile到真实WebView

- 新增显式 `login-probe` 构建特性、`mise run probe-login-window` 和 Linux CI 入口；普通desktop不启用，任务要求Unix且已知Bash。只接受当前exe与单一`--session-probe-child`字面参数，无cwd/env自定义；拒绝额外参数、任意程序及环境覆盖/删除。工厂使用统一profile准备接口，禁用启动文件并清除BASH_ENV/ENV/SHELLOPTS/BASHOPTS，不提供用户shell IPC。
- 固定bootstrap检查`shopt login_shell`与TERM_PROGRAM后才exec helper，父进程继承的测试标记被覆盖为0，bootstrap检查成功后写1；helper检查精确标记再输出。该标记是验证收据，不是授权凭据。现有nonce/window/generation与ACK授权保持不变；报告新增`profileLoginChecked`，native验证要求与实际特性一致，xtask要求headless/DOM两份收据均与请求模式相同。生成DTO保持Raw Channel<ArrayBuffer>与字符串大整数，二进制帧不变。
- 首次编译暴露原生适配使用了不存在的builder API，已按当前字段式PtyCommand/LocalSessionEnvironment修正；strictclippy拒绝测试unwrap后改用Result传播，未添加allow或放宽断言。三项固定工厂/精确收据回归、普通/登录报告错配、缺失/数字/字符串/真假混合否定通过，login特性64项native/strictclippy、绑定漂移与MCP新模块Exact无诊断通过。
- Linux真实login-window报告已完成两轮headless/两轮DOM、刷新页续流/未ACK退役、目标迁移、输入/resize/修复/有序EOF以及七组传输测量，实际退出码0；单测不替代该结果。完整check/test/probe、普通v1/v2真实窗口兼容与绑定漂移通过；38PTY/57app/19xtask/238TS、default/encoding各61native通过，三份实际报告的login字段严格true/false/false且版本2/2/1。最终check额外发现否定fixture的unwrap，已改为无unwrap的空对象fixture后全链通过，不放宽lint。Zsh/Fish实际窗口、用户/系统启动文件、macOS/ConPTY、生产Profile/UI/存储及进程树清理未验收。依赖补丁、审计及发布门禁未改。

## LOCAL-04 当前自动Profile目录到统一启动准备

- 新增公开`pulse-pty::profiles::{AutoProfileLaunch, ProfileLaunchError}`与`AutoProfileCatalog::prepare_launch`；options无程序或shell分类字段，只携带字面argv/cwd/env/remove/mode。目录当前记录给出程序与分类，再调用已有统一环境/模式/预算策略。隐藏可显式准备；删除/缺失/失效拒绝，不从PATH或默认程序回退。该API不授予启动权限，不执行候选文件，不持久化配置，也不消除metadata到spawn的竞态；环境与cwd可敏感，Debug只暴露数量/是否设置。
- 五项新增回归：字面值及覆盖顺序/继承地址删除；隐藏、失效、删除再刷新和缺失fail-closed；目录路径更新及Bash/Sh登录分类；reserved identity/空cwd/NUL/参数额度/无效context；Debug与错误隐私。首轮测试函数名遮蔽和DoubleEndedIterator lint已修正，未增加allow或放宽断言。
- 真实`probe-local-profile-session`已改由目录直接准备options，不再由adapter构造shell程序或传分类；2MiB暂停ACK背压、精确续流、注册层输入与有序EOF通过。完整check/test、43PTY/57app/19xtask/238TS，strictlint和profile/login-arguments真实探针通过，新模块MCP Exact无诊断。当前消费者仍是Rust，不用新目录probe替代先前独立login-window DOM验收；生产Profile/UI/持久化、跨平台启动文件和任意shell入口仍未开放。无依赖或审计策略改变。

## SHELL-03 Linux Bash 配置与提示钩子隔离验证

- 新增`mise run probe-shell-startup`/Linux CI注册及固定xtask脚本fixture，不注入普通Profile。两次真实交互Bash5通过当前目录统一准备，以临时0700 HOME、--noprofile和--rcfile加载自有用户配置后加载集成，源码路径走环境字面值，不拼接用户路径为shell代码。系统bashrc可能仍由平台Bash加载，不能据此声称系统配置隔离或真实用户配置兼容；非Linux命令明确拒绝，纯收据tests保持可编译。
- 用户配置提供字面中文env/PS1/PS0，以及标量和数组PROMPT_COMMAND；fixture在最前捕获退出码并返回原值，原hook仍执行；PS1围绕原prompt输出OSC133 A/B，PS0保留原precommand并输出C，下一prompt输出D与状态。wrapper重复source集成仍只有一个前钩子，用户配置只加载一次。实际输入false/true产生精确D;1/D;0，原hook看到同状态，最终交互标志、env、hook数量及native exit7均验证。输出≤16KiB、stage读3秒/退出1秒/worker25秒，EOF、reader join、直接child回收通过；日志/JSON不包含输出或env值。
- 首次编译不支持所用Result便利方法，已换为现有is_ok_and，不升级工具链。新增严格报告否定：缺失/数字/字符串/false、模式/平台/单次launch/错误exit，以及realUserConfigRead/loginShell/productionIntegration三项scope必须精确false。完整check/test/probe、43PTY/57app/20xtask/238TS和原2MiB目录登录Actor通过，strictlint/MCP Exact无诊断通过。固定script不是生产hook实现，Bash<5、login rc顺序/实际用户脚本/readonly hooks、多shell、actor/IPC Shell事件和DOM标记还未验收；任何远端OSC均不可信，不授予控制权限。依赖与发布门禁未改。

## SHELL-03 有界 OSC 133 后端提示（低层）

- 新增`pulse_term::shell_markers::{ShellMarker, ShellMarkerBatch, MAX_SHELL_MARKERS}`与`TerminalProbe::take_shell_markers`。已有ground scanner开放只读callback复用64KiB unfinished OSC，不增加std vte Parser的无界Vec，Alacritty仍解析原字节。修正scanner在OSC内保留DEL以匹配vte（此前粗边界扫描忽略DEL）；其他C0仍按既有语义处理。只识别exact133;A/B/C/D以及D;0..255，允许缺省未知状态；只认BEL/完整ESC-backslash，CAN/SUB、单ESC及其他结束形式保守拒绝提示。无命令文本、cwd、权限字段或内容执行。
- 队列最多128条，超额丢弃新提示并累积饱和u32 dropped，drain不丢跨片段ST候选。6项新增回归覆盖每个chunk切分/BEL/ST/可选状态、取消/其他OSC/畸形或secret字段、C0/DEL、64KiB超长与队列上限、屏幕/DSR回复/nonzero风险不变，以及对real vte Perform回调的随机组合差分。首次测试错误符号与collapsible-if lint均修正，未增加allow。提示仅是输入流side-channel，无行坐标/parsed边界/完整command log，不随snapshot恢复，不清除risk；同步刷新时可早于grid应用。actor/注册层/IPC/DOM消费还未接入，不将prompt当可信证明。
- `probe-shell-startup`升级schema2，actual每个read片段增量送入TerminalProbe，两次真实交互Bash得到A/B/C/D;1/D;0且dropped=0；EOF不伪造native exit7的D标记，exit7单独验收。backendHintsChecked必须exacttrue；原schema1/raw-only报告拒绝。
- 完整check/test、99term/43PTY/57app/20xtask/238TS、realBash/2MiB目录Profile、五份v1实际actor恢复和两份GBK v2恢复、source drift与真实v2窗口/漂移均通过。标准snapshot600随机流条件matrix311/311；token matrix仍10类初始差异/18个base案例、11可修与7显式不可表示，输出含非法序列的xterm诊断，不视为无条件全部匹配。额外`PULSE_RESTORE_CASES=30000 cargo test -p pulse-term --features validation --locked restore_roundtrip_and_continuation_property -- --nocapture`通过，延期/近似类别不放宽。新模块strictlint/MCP Exact无诊断通过；上游parser补丁、二进制协议、授权、依赖和audit豁免均未改。

## Shell提示的actor与注册层隔离验证

- 新增`SessionHandle::diagnostic_shell_markers`、`AttachmentHandle::take_shell_markers`与`SessionRegistry::take_shell_markers`，不改帧协议或生成命令。actor同turn验证lease/cancel/receiver，最多128枚举提示及显式drop数；reply关闭时恢复原顺序/drop数。采样不写VT、不ACK、不清非零REP/LINES风险；`sampled_at`不代表每个提示的输出/行坐标或DOM消费。
- 注册层校验当前活窗口owner/generation，单session独立500ms准入、250ms响应截止，quota跨换绑/移窗保留；错误owner/stale不扣当前quota，返回后重新授权。成功采样后被撤销的迟到批次允许丢失，非exactly-once日志；与恢复/比较/Marker额度、协议及会话生命期隔离。
- 6项actor回归涵盖跨ST、旧lease、取消/放弃、满输出队列、溢出、reply失败还原与非零风险保持；3项注册回归涵盖外窗/stale、换绑迟到、取消保留提示、迁移限频与detach。真实Bash固定bootstrap输出A/B/C后exec原helper，schema2报告强制3条；原direct helper必须0条，两条2MiB ACK背压/输入/EOF精确排空均通过。该固定样例不是生产脚本注入或命令日志。
- 完整check/test、105term/60app/43PTY/20xtask/238TS、默认61native及strictlint、真实v2 WebView与binding drift通过；MCP对注册新模块Exact无诊断。marker尚未经IPC/DOM消费，也不能推导执行权限/实际子进程退出码。依赖、源码补丁、audit allow/ignore不变。

## Shell提示的EOF监督收据

- 新增`SessionReceipt`与`LocalSessionReceipt`、两层`join_with_receipt`，保持旧`join`和`join_with_screen`签名/监督关闭语义。收据保留最终actor状态、可见投影和最多128剩余提示/drop数；原生收据仍独立持有真实exit/cancel/read/write/resize。live已drain提示不重复，EOF/显式关闭不会推导或伪造D；不完整ST标记不因EOF补全。
- 2项新增actor回归验证EOF剩余D、不完整C、live读取Closed、显式close剩余128/9drop与不造D；关闭测试保持driver活着直到join，避免关闭和真实EOF同时投递造成错误测试断言。真实Bash profile样例改为等待固定helper后发送刻意D7再保留实际helper退出码0：schema3强制首批3、终批1、提示状态7与原生0分开；普通直接helper始终0提示。两条原2MiB背压、精确字节、EOF ACK与native join通过，不扩大生产Shell入口。
- 完整check/test/probe、107term/60app/43PTY/20xtask/238TS、五份v1与两份GBK v2恢复、真实Bash低层、v2 DOM窗口/绑定漂移、默认61native/strictlint通过。MCP对应用监督模块Exact无诊断。二进制协议和依赖门禁不变，尚未提供持久命令日志、每提示坐标或IPC/DOM消费。

## 固定页面Shell提示的只读IPC

- 新增`probe_peer_shell_hints`与生成`ShellHintReceipt`：限定localcaller、当前run、精确peer WindowToken、当前页面nonce、当前binding generation；页面必须已完成捕获确认且尚未Done。重复/旧页面在registry采样前拒绝，返回后重新检查owner/generation/nonce并单次标记提示回执，不新增输入/resize/close或任意会话选择权限。
- DTO最多128枚举提示及drop数；revision/sourceBytes/utf8Bytes为保留64位精度的十进制字符串，sampled-at不代表逐提示坐标或已经绘制的边界。固定登录bootstrap输出A/B/C，首次页必须收到三提示；普通模式与刷新页必须空。page Done必须已有提示回执；`peerShellHintsChecked`取实际首次及刷新页状态并贯穿native验证、JS、xtask严格bool门，不以root单次完成flag代替。
- 2项native页面/DTO回归及4项TS负向回归覆盖跨导航撤销、重复、未确认页、无内容枚举、状态/坐标非法和模式错配；实际IPC另外检查旧nonce、错误generation与重复采样拒绝。二进制帧、ACK、风险清理、比较额度、原生退出码语义不变。
- 完整check/test通过：107term/60app/43PTY/20xtask/242TS；default与encoding-probe各63native、login-probe66native及strictlint/绑定漂移通过。五种真实Linux模式(login/defaultv1/UTF8v2/GBK/externalWM)各2headless+2DOM及7测量完成，实际JSON提示回执均为true，只有login模式的profileLoginChecked为true。MCP对新端点Exact无诊断。
- 外部mode首次窗口匹配不唯一并超时：当时未保存XID集合，具体根因未证实。控制器现收紧为自有子进程PID+固定main标题，跨两个操作周期保持同一XID；拒绝歧义/缺窗/外PID/替换，不选第一匹配，不放宽2s操作截止。新增`mise run probe-window-controller`的5项确定性隔离fixture及Linux CI，不能当真实WM验收。
- 后续复验出现既有helper exit2（源码明确40s watchdog），耗时原因尚未定位；补无内容peer完成/EOF阶段计时，保留40s截止和exit0断言。再隔离复验完整通过、进程清理正常，但不宣称该截止风险已修复。生产脚本注入、持续命令日志、命令坐标DOM渲染、真实用户配置及跨平台仍未完成；依赖审计门禁未变。

## 自动Profile的typed SQLite app_state

- 新建`pulse-store`，按既定rusqlite/rusqlite_migration方案固定0.40.2/2.6.0，native bundled/backup、关闭默认Wasm特性；worker独占Connection，16待处理请求try_send准入，不在UI/async执行SQLite。WAL/FK/FULL实际验证；当前读写均串行，读池、Vault、密钥和应用目录平台适配仍未实现。
- 非秘密`AutomaticShell`偏好仅Bash/Zsh/Fish/Sh/Pwsh的hidden/deleted；严格版本1、4096字节行预算、未知字段/身份、重复和冲突拒绝，读坏记录不回退默认，保存事务不覆盖坏行。SQL固定且parameterized，无任意SQL、路径、程序、命令、环境或凭据载荷接口。
- 原10项store测试覆盖真实文件重开、worker clone/drop/join、16额度/Closed、取消执行前不写、坏/过大行、未来schema/无版本既有表保持原数据、私有权限/symlink、迁移前SQLite Backup保存未checkpoint WAL、existing backup不覆盖、失败迁移rollback并保留原schema及backup。新增9项通过：同worker及独立worker原子编辑合并、CAS旧快照拒绝/新快照写入、显式恢复deleted、坏行/trigger失败保持原值、真实SQLite锁竞争不改行、idle/fullqueue cancellation/drop owner、保留client后关闭、已提交写入lost reply不回滚；合计19项。升级测试为内部版本1→2fixture，不等于生产已有schema2；当前production schema仍1。此前tempdir默认权限导致Location失败，已只修fixture到0700，不降低门禁。
- app增加2项固定枚举映射/目录恢复回归；PTY保存独立hidden身份集合，重启初次缺shell而后再发现仍hidden。`mise run probe-profile-store`新版schema2报告在5个独立子进程真实保存、并发2editor、旧快照Conflict和重开，5s每child截止、无重试，最终join关闭worker，确认3个hidden与1个deleted全部保留；新增2项严格PID/模式/worker join/字面true负向回归并纳入同一mise task/CI，旧两进程报告不能替代；metadata candidate不执行，不计作Shell或真实用户配置验收。task/usage/跨平台CI注册，跨平台CI尚无执行证据。
- 初始检查点最终check/test/probe、实际Unix发现与PTY启动、2MiB登录Profile→actor ACK/EOF、bindings-check、63native tests/strictlint及实际v2两headless/twoDOM/7测量通过：10store/62app/43PTY/107term/242TS。本轮19store/strictlint、真实5process及2strictreceipt回归、完整check/test/probe、2MiB登录Profile→actor、bindings-check、63native strictlint/tests及v2 twoheadless/twoDOM/7测量均通过；62app/43PTY/107term/242TS保持通过。MCP store诊断Exact无错误，引用查询出现stale未当精确影响结论；实际调用者经有界读取/全编译检查核验。tokio-util沿用已锁定0.7.19，只新增直接引用与rt feature，没有升级包。
- Unix后续补齐有效UID、精确模式、单链接及预存侧文件检查（见下）；仍不宣称完整目录链/ACL/TOCTOU安全；WindowsACL/备份恢复UI/持久化Profile权限IPC/读池性能/Vault仍待实施。新6个库未新增cargo-deny拒绝或公告，原license6/maintenance2仍失败，不加白名单/ignore，详见dependency-review。

## SQLite进程强制终止与WAL恢复

- `mise run probe-store-crash`新增2个父用例，每个只执行自有integration-test子进程，在私有临时库、固定typed偏好上操作。直接SQL仅存在测试fixture，不扩大store或生产IPC权限。ignored子用例是被父用例明确调用并终止的helper，不是跳过的产品验收；正常workspace test也会执行两个父用例。
- 固定phase+实际child PID的完整行收据，5s就绪截止、32行/每行1024字节读取预算；已COMMIT用例还要求live WAL文件>32字节。父进程实际kill/wait：Linux SIGKILL=9；Windows代码路径要求Child::kill的退出码1，尚无Windows/macOS实测证据。12s helper watchdog退出不能冒充强制终止，RAII在失败时也kill/wait并join读取线程。
- 未COMMIT时终止后，重开保留此前Bash hidden和Zsh deleted；已COMMIT且两条Connection仍存活时终止后，重开恢复WAL中的Fish hidden，保留完整logical状态。首次超时来自libtest的同行名称前缀，改helper独立换行分帧后两个精确握手通过，不放宽字符串匹配或截止。
- 2个父用例、19项store unit、完整check/test、strictlint和5process偏好回归通过，MCP测试文件诊断Exact无错误。上个检查点原生v2DOM/63native回归已通过，本次仅新增测试/任务/文档与Unix限定import，未重复未变的原生测量。
- 不模拟物理断电、损坏磁盘/SQLite、用户已有库或备份恢复UI。schema仍1、frame协议不变。tokio-util rt直接引用后重跑all-feature audit仍仅原6项license拒绝与2项维护公告，bans/sources通过；audit不是通过，未新增ignore或放宽许可。

## Unix私有存储身份与侧文件边界

- DB打开前校验叶级parent的有效UID与权限700；主DB及预存-wal/-shm/-journal要求权限600、同有效UID、regular且nlink1，拒绝hardlink/symlink/目录、宽权限及外来owner。Owner只经safe Rustix geteuid读取，无unsafe或权限提升。caller仍必须提供可信目录，不验证或锚定完整祖先链，metadata检查与SQLite open之间TOCTOU尚未消除；WindowsACL/所有权/硬链接保证仍未验收。
- 22项普通store unit通过，新增硬链接保留两名原数据、12种sidecar别名/类型/权限组合不修改主DB或目标、只读父目录500/000及文件400保持权限与内容，Root也不得绕过只读意图。fixture-only模式恢复用于非Root测试清理，production不chmod/chown。
- `mise run probe-store-owner`在本机现有有效UID0显式执行1项真实chown负向测试，只改变专用临时目录/DB/3种侧文件的owner，证明拒绝且无自动修复；没有提权或触及用户文件。此用例在普通CI ignored并明确要求额外授权环境，不作为Windows/macOS实测证据。Node入口拒绝非Linux/macOS、非数值UID0与0tests假成功，必须有精确testname、1passed/0failed/0ignored、正常exit0收据；3项纯gate/收据否定测试纳入普通CI，不需Root。
- 完整check/test/probe、22普通store+1专用Root、2真实进程终止、5独立进程并发/重开、2MiB PTY→actor、bindings-check、default63native strictlint/tests均通过；62app/43PTY/107term/242TS保持通过，MCP database诊断Exact无错误。Rustix沿用已锁定1.1.5，只增加target依赖与测试fs feature，无版本升级；audit仍原license6/maintenance2失败且无豁免。
- 既有数据不会为满足这些条件自动修复；存储平台目录适配器、完整所有权链/ACL/TOCTOU保证、生产Profile IPC与Vault仍未实现，schema仍1。

## 设置分层与内存Vault密码学基础

- SettingsResolver 8项回归通过：default→user→group→host逐字段优先级与来源、显式false/同默认值、不可信字体CSS语法/Unicode名字、未知/重复/未来schema、文档预算和错误不泄露输入。schema由实际Rust类型生成，任务与CI注册，缺失/字节漂移/范围篡改负向均拒绝；不读取或写入用户settings文件，不计热加载或设置UI。
- AEAD 8项回归通过，含公开draft-irtf-cfrg-xchacha附录A.1已知ciphertext/tag答案、Unicode/NUL/空值、独立随机nonce、错误key/id/kind、密文/nonce/tag篡改与截断、64KiB双边预算、幂等lock与zeroize早清。固定key/nonce仅用于公开向量测试；正常seal只用可失败OS CSPRNG。额外3个compile_fail证明密钥/明文不能Debug、明文不能Serialize，严格lint与MCP两模块Exact无诊断。
- 本轮store最小实测38常规unit/1额外Root用例ignored、2真实崩溃回归与3compile_fail通过；最终完整check/test、schema只读漂移、45项SSH feature、5独立进程、2MiB actor、bindings与default63native严格检查均通过，62app/43PTY/107term/242TS通过；实际v2二轮headless/DOM与七组测量报告通过。无实际钥匙串/密码读取、主密码、KEK包装、秘密落库、完整应用锁或前端凭据IPC；schema与binary帧格式不变。
- 最终原生日志在EOF阶段后出现XIO fatal IO诊断，随后验证器接受完整报告且任务退出0；输出未标注来源PID，未定位、未宣称已修复，保留风险。不把该诊断本身作为通过证据。
- 下载先出现Cargo/官方HTTPS超时，同一地址复验HTTP200后Cargo正常校验归档并成功构建；早期超时原因未定位，不以成功复验认定修复。全特性audit仍原6项许可证与2项维护公告失败，bans/sources通过，未添加ignore。

## 配置文本的精确标量编辑

- 纯`set_terminal_setting`只接受font_size/font_family/cursor_blink类型化值，未提供任意key/路径/原始TOML片段。先验证原文和新值，不重置坏记录，生成后验证整个模型与预期相等且仍≤64KiB；不产生其他默认项、不写文件。
- 8回归通过，覆盖完整原文注释/排序、inline/dotted键、Unicode/转义token、显式false、新值稀疏、非法值/未知/未来schema及输入/结果预算。首轮发现toml_edit整体串行化会统一重复dotted parent的空格，改用不可变Document的真实值span只替换目标token，保留原失败断言；dotted新增值追加根级赋值保留旧字节。
- 直接引用已锁定toml_edit 0.25.15（parse/display），未升级版本；本轮46项普通store、2实际崩溃、3compile_fail及完整check/test、CLI strictlint、五独立进程验证通过，62app/43PTY/107term/242TS保持，MCP编辑器Exact无诊断。无Native/帧协议变更，未重复上一检查点已过的真实WebView。删除覆盖、原子落盘、外部并发编辑冲突、notify/设置UI仍未验收。

## 固定成本的主密码KEK验证原语

- 3项回归通过：RFC9106 §5.3 Argon2id v1.3已知答案（32KiB/p4/secret/AAD仅用于规范测试）；实际固定64MiB/t3/p1路径中，同密码/salt可恢复固定32-byte密文，换密码或salt认证失败，随机salt独立；忙时不排队、不开始第二次分配，空/超4096byte输入在准入前拒绝。生产入口不接受任意成本/版本/secret/AAD参数。
- 显式try_reserve工作区，Zeroizing<Vec<Block>>及32-byte key包裹所有正常/错误离开路径；单进程Mutex try_lock最多1job。密码是借用输入，由调用者负责寿命和清理；CPU工作不可取消，尚无异步调度、授权epoch提交、取消/迟到结果、限速/错误次数应用策略。派生成功不是密码认证成功或Vault解锁。
- 4次固定64MiB debug派生合计约16.6s，仅测试观察，不是production或跨平台基准、生产参数批准。49项普通store/1额外Root用例ignored、2真实崩溃、3compile_fail及完整check/test/45SSH/CLI严格lint/五独立进程/default63native均通过，62app/43PTY/107term/242TS保持，MCP Exact无诊断。无Native/帧协议改变，未重复已过DOM。此次audit实际core_exit0/audit_exit5：原六项license和两項maintenance保持失败。
- 仍无KEK包装DEK、版本化持久信封、秘密落库、OS钥匙串、主密码UI或完整应用锁，不读取用户密码/凭据。

## 内存KEK包装DEK与认证恢复

- `ValidationWrappedDek`只含private加密metadata，无持久serde/文件读取/任意成本。新建测试Vault随机生成独立32-byte DEK，KEK以Vault UUID+固定wrapped_dek kind的AAD加密；恢复严格48byte密文和可信预期ID，认证后检查32byte明文并通过Zeroizing源导入opaque cipher，未返回raw DEK、未生成替代key。
- 2回归通过：原DEK lock后正确密码恢复同一已有credential，错密码/tag篡改/连metadata一起换ID均认证失败；错目标ID、截断、nil ID在昂贵派生之前拒绝，原加密blob不变。对公开输入零化责任仍属调用者，派生后的key、工作区和解密DEK均有零化所有权。
- 增加cfg(test)串行场景锁仅协调两个真实高成本密码学夹具，生产1job try_lock及Busy语义不变；完整check/test、CLI严格lint、五独立进程通过，51普通store/1Rootignored/2真实崩溃/3compile_fail，62app/43PTY/107term/242TS保持；MCP Exact无诊断。无新依赖/参数/Native/帧变化，不重复上一阶段已过SSH/DOM/审计。
- 仍无持久信封格式、真实凭据/钥匙串、异步epoch与迟到key发布、应用锁或UI；这不是成功解锁/用户授权，后续控制层只能在当前授权与epoch确认后启用key。

## 后台内存解锁epoch与两阶段key确认

- `VaultKeyLifecycle`仅模型化backend key可用性，不等同用户/窗口授权。`AuthenticatedDek`由成功的wrapped_dek认证构造并带可信预期VaultID，普通KEK/随机cipher不能当证明。RAII permit→单次worker→candidate receipt→显式confirm；owner是不可串行化Weak pointer+不回绕epoch，没有caller字符串generation。scope clone共享状态、pending handle不保活owner。
- 初稿将候选key放在结果对象里，review发现lock只能撤权而不能同步清掉外部候选；已改为scope持有candidate key、结果是无key回执。lock/close/permitDrop/receiptDrop同步drop对应key，已锁或失败结果不会替换新scope。confirm借用回执，foreign拒绝不消耗当前合法确认机会。Mutex poison和epoch耗尽清key并fail closed。
- 9生命周期回归通过：候选未确认不可用、单worker/单pending、旧worker和已有candidate经过lock拒绝、取消后重绑、外来scope/同ID重建、owner销毁/错误VaultID、真实线程延后结果、重复confirm、耗尽时清active key、poison及lock/receiptDrop即时清candidate。真实64MiB KDF/tag认证成功结果接入当前permit确认并读取同一旧credential，未改断言或生产KDF Busy。
- 5 compile_fail通过（新增证明不能Debug、permit不能Clone）；全check/test/CLI严格lint/五进程/default63native通过，60普通store/1Rootignored/2崩溃回归，62app/43PTY/107term/242TS保持。MCP新模块诊断Exact无错误；改返回类型的引用工具带stale警告未算精确通过，实际workspace编译确认无漏改调用。无依赖版本/成本/帧变更，不重复已过DOM/SSH/审计；audit仍保持之前实际exit5。
- 尚无spawn_blocking/有界入队、CPU取消配额与result投递、窗口/页面授权整合、生产应用锁或idle锁、持久信封/钥匙串/用户凭据。控制器只能清自身持有的key，不能保证清除工作线程尚未投递的临时结果、早先合法transport副本、swap/core dump；后续监督层仍需处理它们。

## 有界后台主密码任务与取消交接

- `vault_unlock::begin_validation_unlock`只接受后端Rust scope、拥有密码和固定验证加密元数据。新PasswordInput 1..4096bytes、Zeroizing且禁止Debug/Clone/serde；借用副本仍归原调用者。KDF私有AtomicBool RAII令牌在CPU排队前取得，裸KDF与后台任务共用单quota，不能直接构造或复制令牌；没有新依赖/成本参数、前端命令或数据库schema。
- 一个spawn_blocking闭包持有密码与CPU令牌直至实际退出。未启动即接收方关闭/epoch失效跳过计算；运行中取消或ready未来timeout只撤销scope，CPU额度不提前释放。消费ready等待投递和join再给出无key回执，仍需当前permit确认；外来确认借用回执不消耗机会。显式cancel返回只可join、不能abort/清quota的观察收据。panic/认证失败/丢失或迟到结果不会自动重试、换DEK或启用key。
- 10项回归通过：joined仍Pending、foreign确认拒绝后合法确认；running取消/timeout仍Busy直到真实Drop；queued取消跳过compute；lock后的迟到线程不触碰新epoch；已投递未观察receiptDrop清key；worker panic释放额度且scope Locked；无Runtime/坏输入/错误ID不取得许可；Busy不消费任何scope的机会。额度实际退出采用cfg(test) oneshot见证，不轮询或凭sleep推断。真实64MiB KDF在阻塞线程完成、tag认证后当前epoch确认读取同一旧credential，错误密码拒绝。
- store严格lint及MCP Exact无诊断；完整check/test/schema漂移/五进程/2MiB会话/绑定漂移/默认原生严格lint与63tests通过。70普通store/1Root-only ignored/2真实进程崩溃/6 compile_fail，62app/43PTY/107term/242TS保持。改动不影响协议、Native工厂或算法/包features，未重复已过DOM/SSH/原有audit exit5。
- 仍是验证profile和内存scope，不是生产解锁/窗口授权/idle应用锁、系统钥匙串或持久信封；不能撤回已发给授权transport的secret副本、保证swap/core dump擦除或中断运行中的Argon2。生产参数及Win/mac解锁性能、窗口权限与应用退出时scope关闭须单独接入验收。

## 稀疏覆盖删除与下层来源恢复

- `settings_remove::remove_terminal_setting`只允许TerminalSettingKey的3已知显示scalar；输入完整SettingsPatch校验先于编辑，缺项byte-identical。从不可变toml_edit key/value span定位，普通/点键删目标赋值，内联表另删一个必要delimiter；不调用mutable formatter。delimiter scanner只处理parser界定的whitespace/comment/comma区间，不重写TOML语法或用户字符串，comments中的逗号不参与选择。返回前再次校验并比较预期稀疏模型。
- 9回归通过：普通/quoted-escaped/dotted assignment、inline first/middle/last/only与trailingcomma、comments中的逗号、multiline转义Unicode值、CRLF/无末换行、缺项幂等/空表保留、坏/重复/未来/超预算拒绝。真实resolver证明host删除只退回group，再删group退回user、再删user退回compiled defaults；显式false和其他字段来源不变。
- 864次生成布局oracle通过（6排列×3布局×转义键/CRLF/尾换行/trailingcomma），每次还验证第二次删除byte-identical。期望删除区间在生成时记录，不由被测parser倒推；评论故意含同名赋值和逗号，避免全局替换假通过。strict all-target lint、MCP Exact无诊断；初始符号工具stale未计精确通过。
- 完整check/test/schema漂移/probe/五进程通过，79普通store/1Root-only ignored/2真实崩溃/6 compile_fail，62app/43PTY/107term/242TS保持。无依赖/schema/前端/Native/帧变化，未重复上一阶段已过63native、2MiB、DOM或audit；发布仍由原有license/advisory阻断。此处无用户文件读写、原子保存或watcher，不能计完整VAULT-07。

## 私有设置文件冲突检查与原子安装

- `settings_file::SettingsFile`只处理可信绝对私有目录里的固定settings.toml；snapshot保留原文、文件/目录身份，未实现Debug/serde（comments可能不适合日志）。同步操作须离开GUI线程。pinned dir fd、openat no-follow/nonblock/CLOEXEC、700目录/600有效UID单链接regular文件，readonly intent也拒绝且不修权限；不建立Windows ACL的假替代。
- 跨进程每操作独立open-description try-lock，比较完整snapshot后才允许typed set/remove。temp随机exclusive同目录、write+fsync，临近安装再次比较及检查lock/dir/temp身份；首次NOReplace、更新rename+directory fsync。rename之后失败返回Uncertain，禁止自动回放；坏/未来/重复/超64KiB文档、符号/硬链接、外来目录snapshot、宽/只读权限都不会重置目标。只清本次且身份仍匹配的temp，进程kill的orphan/无关文件不回收。
- 首轮并行出现意外Busy；阶段诊断证明是锁类别而非NOReplace覆盖。只关fd会让fork/dup共享描述符继续持有flock，新增显式unlock守卫及clone-fd确定性回归；保留Conflict与超时断言后，含独立child/强杀的12例suite连续12轮通过。原“换inode”夹具改为真正同内容rename，不依赖原地同文重写时戳必变。
- 两独立进程先持同一snapshot，第一提交、第二明确Conflict；精确阶段/PID收据后SIGKILL：temp fsync前后但rename之前仍是完整旧目标，rename之后可重开完整新目标，运行时锁Busy而进程死后释放。仅证明进程故障，不等于电源/文件系统故障。post-install故障注入明确Uncertain，原文或已安装新文的断言都保持。
- `probe-settings-file`和CI注册严格12例receipt（3真实进程边界、9文件/锁回归），3脚本否定覆盖不支持平台/0tests/缺例/不完整summary/signal/timeout；mac候选实现与CI未运行不能称通过，Windows/其他平台拒绝。fullcheck/test/probe/schema、SQLite crash/五进程、native strictlint/63tests通过，91普通store/2ignored（Root及fixed-child）、242TS；MCP Exact无诊断。Rustix既有锁定版本仅将fs feature用于production backend，无版本/帧/UI变化，原有audit门禁未豁免。
- 仍不保证非合作同UID编辑器在最后compare之后的原子CAS、全部祖先链/TOCTOU或Windows ACL；尚无生产配置目录/UI授权/自动监视。计划notify8.2.0元数据许可为CC0-1.0，当前许可策略未允许，问卷未返回授权；未采用依赖或修改deny规则。

## 最后有效设置缓存与合并失效信号

- `settings_reload::SettingsReloadCache`拿走已准入SettingsFile作为固定source；只有同步`&mut self`单读取者，无GUI调用或后台调度。bootstrap为空user layer而非伪造成功file snapshot；dirty bool将1000次失效合并为一次读取，消费后无隐式polling/timeout重试。坏/未来/权限/Busy错误只返回静态分类，原文件、last-good overlay、保存snapshot与revision保持；后续可信事件或manual refresh需要显式invalidate。
- 验证后比较稀疏patch而非formatter输出。comment-only/同值但新inode更新成功source基线、不递增model revision或包含用户comment的事件；改变只返回revision与3个changed-field bool。显式false是user来源变化，即使等于编译default仍Applied；文件移除变成空overlay，真实Resolver回到下层来源。closed不再读/发布；u64耗尽保留旧状态并关闭，不允许回绕复活。
- 8真实私有文件回归通过：bad initial不重置/不生成snapshot、last-good坏文拒绝后新signal恢复、1000失效合并与bad source不隐式重读、comments保留到实际typed save、删除来源恢复与显式false、Busy保持且不自动retry、关闭拒绝、revisionmax保持。MCP Exact无诊断，strict all-target lint、fullcheck/test/schema/绑定漂移、strict12设置文件/五独立进程probe通过：99普通store/2ignored/6compile_fail，62app/43PTY/107term/242TS。无包/特性/帧/前端/原生入口变化，未重复上一阶段已过63native或既有audit；现有release gate不变。
- 此模块不是notify服务、异步读取额度、文件事件generation协议或settings.changed IPC；单读取者只保证同步调用不重入，不声称读取期间的后台新事件已自动采样。当前未选择用户生产目录，也没有修改用户设置。notify CC0许可问卷超时未获授权，未添加依赖或改deny规则，自动文件监视仍待决定。

## 组/主机稀疏覆盖与真实跨进程解析

- `settings_scope`/`settings_database`/`settings_store`已接同一bounded StateStore worker，app_state schema1直接复用，固定kind/UUID/v1 keys。8192byte有界原始读取，strict private doc校验kind/id/schema/revision与稀疏模型；无命令/凭据/任意SQL或原文保存。immutable snapshots不可序列化、改revision或转scope，新增3个compile_fail实际通过。
- IMMEDIATE typed edit/CAS保持另一字段与作用域；revision禁止ABA/回绕，最后override删除保留tombstone、missing/no-op不生成记录。nil/invalid/malformed/duplicate/future/misbound/oversized数据拒绝，坏记录不会被edit修成empty。DEFERRED成对读取实际验证另一Connection在group读取后提交host更新，不可混用旧group与新host。用户文件不参与SQLite事务，也未授予host存在性/成员关系/窗口权限。
- 11存储回归、3严格receipt否定通过；`probe-settings-scope`/usage/CI注册，Linux五真实独立进程验证两编辑者并发、同UUID namespace隔离、fixed private settings.toml→last-good cache→typed DB pair的逐字段来源、ABA拒绝/row保持及worker joined。模式/PID须distinct、布尔不可truthy/缺失、版本不可string或旧值；每child5秒截止未延长。只操作temp fixture，不读取用户home，macCI待实测，Windows该组合probe明确unsupported而非zero-tests成功。
- MCP Exact无诊断，fullcheck/test/schema/probe、5进程scope与Profile、12文件case/实际kill、2MiB session、绑定漂移、native strictlint/63tests通过；110普通store/2ignored/9compile_fail，62app/43PTY/107term/242TS通过。没有包/特性/协议/前端改变，不重复DOM或既有审计；六项许可证/两维护公告release gate与未获授权CC0依赖保持原状。

## 后端配置上下文的候选与发布交接

- `pulse-app::settings_context`已导出owner/weak handle/owned capture/candidate/published snapshot；不接受原始JSON、路径或命令，不实现window/host权限、主机存在性或自动文件监视。target在state mutation前检查group/host namespace与nil，user sparse patch先完整校验。owner close/Drop同步阻止confirm，weak句柄与候选不使owner复活，共享Store/PTy仍存活。
- 每context只一个capture槽，进入Store前取得；select/invalidate不提前让新capture覆盖旧槽。old context epoch与独立capture token共同约束read前后/confirm/Drop，sameepoch新publication也令old snapshot非current，A→B→A仍stale。候选确认前不publish；错owner/old/closed/exhaustion不能启用，计数不回绕。未来drop会discard响应，不取消已开始的SQLite操作，仍有原Store全局16queue/单worker界限。No auto retry；same-target失败保留last-good但不能作为current重新应用，different-target清除旧host值。
- 14真实SQLite app生命周期回归、2compile_fail与MCP Exact/strictlint通过。五进程scope probe升级schema2，fixed reader从真实用户file cache→DB pair→Context进入old host候选，切换至group-only时Busy/old确认拒绝，随后Group20/source/revision3显式确认，close令snapshot非current而sharedDB host1仍可读。新增contextSwitchChecked/lateCandidateRejected/contextOwnerCloseChecked严格布尔收据与否定用例，不允许旧报告/missing/truthy替代；每child5秒未变，mode/PIDdistinct/workerjoined要求不变。
- Fullcheck/test/schema、45SSH特性、scope/Profile五进程、2MiB session、绑定漂移与native strictlint/63tests均通过；76app/2docfail、110store/2ignored/9docfail、43PTY/107term/242TS。manifest/lock仅增加已锁tempfile/uuid dev引用，无包版本/许可/productionfeature/帧/前端变化，未重跑旧DOM/audit，不放宽六license/twoadvisory或CC0待决门禁。
- is_current只表示当前owner状态瞬时相符，不能代替external GUI apply的授权事务；DB变化需trusted explicit invalidate，未声称自动最新/跨file与DB一致。native WindowToken/page nonce/generation授权、自动事件调度和DOM应用留待后续接入，不计完整VAULT-07。

## 绑定实际注册视图的配置授权

- `registry_settings`导出opaque capture/candidate/snapshot与静态错误，Binding独占context owner，registry/window/session/generation逐次授权；无frontend target、明文密码、路径、命令、VT或ACK操作。configure/invalidate仅可信后端；context与Host存在性/权限仍需适配器先判断。
- 12个真实SQLite/actor-binding回归覆盖wrong owner/generation/invalid patch不清槽，switch、detach、rebind、navigation、transfer、destroy/recreate、last-good/current、同窗口跨session及跨registry伪造私有元数据。Binding消失同步drop owner，weak候选不能使视图或窗口复活，actor/store仍存活；registry mutex不跨await，短锁确认原子授权。strictlint与MCP Exact无诊断通过。
- 五进程probe升级schema3，强制registryWindowAuthorizationChecked/registryRefreshChecked/registryTransferChecked与原context收据，missing/truthy拒绝。Reader真实attach/授权、host→group切换Busy/迟到拒绝、移窗后source/group revision3、退休页捕获拒绝；无view时actor精确解析10bytes BACKGROUND再恢复binding，旧generation不能current，最终显式关闭并join actor与store。每child5秒及PID/mode要求不变。报告nativeWindowsCreated=false、renderer=rust-diagnostic，不计系统窗口/DOM验收。
- Fullcheck/test、schema/45SSH、scope及Profile五进程、2MiB PTY→actor、binding漂移、native strictlint/63tests、真实v2 WebView二headless/二DOM及七测量均通过；88app、110store/2ignored/9docfail、242TS。真实窗口仅兼容回归，未配置或调用新设置功能，不计设置GUI。无依赖、许可证、feature或frame变化；本轮不重复audit，原六license/twoadvisory及未批准CC0门禁保持。
- 已有page retirement/Destroyed回调会通过Binding销毁撤销context，但native设置端点/page nonce桥、主机实体权限、自动invalidate/notify、DOM应用和系统设置页尚未实现；current是瞬时判定，不是GUI原子应用或永久最新证明。

## 有界后台用户设置读取

- `settings_source`：先取得process唯一额度，再blocking准入/初读，initial坏文件不重置或fallback；一worker/dirty位、独立验证stamp与模型revision。只暴露parsed layer/静态状态，无文件原文/路径/新依赖或IPC。
- 12个真IO回归：坏initial、last-good/显式重试、1000信号合并、stale成功后latest坏文件仍保留已发布旧值、读前/后close、weak句柄不能复活、运行I/O额度保留、删除/false、耗尽、真实blocking panic与worker错误监督。Drop不abort/不隐藏Failed，实际exit才释放资源；strict store/CLI lint与MCP Exact无诊断通过。
- `probe-settings-source`/CI强制12个具名收据及总数；3个Node gate拒unsupported/零/缺项/duplicate/旧cache替代/signal/nonzero。Linux实测，mac仅CI注册，Windows不能0tests冒充通过。
- scope探针schema4：五进程reader从后台源捕获user patch、走SQLite pair及registry/context，backgroundSourceChecked/sourceTaskJoined必需；关闭源后句柄拒绝但共享DB可读，每child5秒/PID要求不变，仍Rust diagnostic/nativeWindowsCreated=false，未读真实HOME。
- fullcheck/test、Source/scope/Profile/file/2MiB、schema/binding漂移、native strictlint/63tests通过：122普通store/2ignored/9docfail、88app/2docfail、242TS。无包/许可/feature/frame/GUI变化，未重复已过WebView或既有audit失败；六license/twoadvisory及CC0待授权不变。用户验证版本与registry确认尚未合成事务，不计watcher/GUI或跨file-DB原子性。

## 用户源与注册视图的联合确认

- `with_current`在当前source身份/验证stamp/revision/完整patch匹配的短锁内执行同步回调；禁止I/O、重入、await或延后工作，固定source→registry→context顺序。3个真文件guard回归验证锁实际持有、失败回调不运行、同内容重验证/foreign/closed不复活。
- `confirm_view_settings_from_source`先检查窗口/session/generation，在source锁内比较候选原稀疏user层并重授权确认；6个file+SQLite+actor回归覆盖host覆盖遮蔽错误user也拒绝、同semantic revision新验证仍拒旧候选/回执、坏文件last-good不可current、权限优先、page retirement、同revision/content的foreign source。SourceBoundSettingsSnapshot Rust-only，不能充作主机权限或DOM应用许可。
- scope五进程报告升级schema5，jointSourceConfirmed/oldUserVerificationRejected强制布尔收据。Reader实际源重读同文件，semantic revision不变但旧候选与联合回执拒绝，新回执完成原transfer/retire/rebind与10bytes后台输出路径；source/store/actor均监督退出，5秒child/PID要求不变。报告仍nativeWindowsCreated=false、rust-diagnostic，不计真实窗口设置验收。
- 全check/test、12source/12file、scope/Profile五进程、2MiB、schema/binding漂移、native strictlint/63tests通过：125普通store/2ignored、94app、242TS；MCP Exact无诊断。无包/许可/帧/GUI变化，本轮不重复已过真实WebView及原有audit失败；六license/twoadvisory和CC0待授权保持。

## 特性隔离的真实窗口配置回执

- `settings-probe`显式特性隔离固定配置工厂，默认desktop不启用；`mise run probe-settings-window`和Linux CI已注册，xtask严格拒绝普通、缺失、混合设置模式回执。新增typed `nativeSettingsChecked`，不是GUI配置或输入/ACK授权。
- Linux真实窗口四轮（两headless、两DOM）：初peer页以实际WindowToken/page nonce及绑定generation确认用户18<组20<主机22；实际Started事件撤销旧候选/回执，新页重新确认组20。旧页nonce不能访问新页，source仍存活。
- 实际peer销毁并完成原移窗流程后，目标主窗口真实Channel绘制/ACK结束、Guard尚未detach时重新确认用户18，旧窗口不能确认迟到候选。不是另建Rust诊断绑定冒充目标页绘制。随后固定私有文件经已验证SettingsFile CAS/font19修改与fsync，显式trusted invalidate后源线程读取revision2；旧候选精准Source(Stale)拒绝，最新用户19可确认。源/store监督退出后actor仍Running，后续输入/恢复/EOF及七测量保持通过。
- `settings-window.json`初session/DOM的nativeSettingsChecked均true；普通channel与channel-v2报告均false，每种七测量通过。完整check/test、源12cases/五进程、schema/bindings漂移通过：125普通store/2ignored、94app、242TS、63native；双特性strictlint通过，Astrolabe编辑后Exact无诊断（调用/引用索引stale结果未计准确通过）。新增源变化阶段另四轮通过，没有重跑未变化的默认v1/v2/core。
- 原有180秒窗口外层、40秒helper与恢复/ACK限制未放宽；源变化Ready等待沿1秒预算。仅固定后端回执，没有GUI字体应用、设置页RPC、真实notify监听或跨平台验收；CC0仍待授权，六许可证/两维护公告门禁不变。GPG签名超时的原联合确认检查点仍暂存，本特性工作树另保留，未无签名提交。

## 原子只读配置捕获及 DOM 帧停滞诊断

- 新增 `read_current_view_settings_from_source`：源验证与视图授权在同一短锁内完成，无 I/O/await；返回不可改写的捕获模型，不授予输入/ACK/GUI应用权限。3项回归覆盖源重验证、同generation上下文真变动/无操作保持、外来窗口先于源关闭状态拒绝。全核心97app/125普通store/242TS、源12cases/五进程和绑定漂移通过，feature63native及strictlint通过；Rust编辑后Astrolabe Exact无诊断。
- 首次组合在第三轮主DOM遇到 `terminal.render_timeout.frames=0.visible=true.connected=true`，整组未通过；所有配置阶段完成不能替代整体验收。增加仅包含时长/焦点/绘制与围栏数/销毁布尔的诊断，不读取终端文本、路径或配置值。1000ms墙钟和两次真实rAF机会保持，无timer fallback/强制focus/自动重试；新增2项反例验证延迟timer与有焦点的一帧都不能通过。全部244TS、tsc/Biome通过；TS Astrolabe诊断等待publish超时为Unknown，未计通过、未安装组件。
- 仅复验失败的 `probe-settings-window` 四轮后通过：原子读取接入真实目标活绑定，Source1→2/font18→19后旧候选拒绝；报告schema4/completed=true/sessionRuns=2/domRuns=2、两模式nativeSettingsChecked=true、七测量通过。未重跑已过且未变化的默认v1/v2及核心。第一次零rAF超时原因仍未定位，此次成功不称修复；保留既有draw/visibility/query超时风险。
- 随后修正独立的帧截止漏洞：计时器调度落后时，rAF回调仍须按单调时钟检查 elapsed>=1000ms并拒绝；新增0/1已接受帧、先执行过期rAF而不dispatch timer的2项反例，10项围栏/全部246TS及tsc/Biome通过。这不解释或修复首次零rAF超时。严格截止下另四轮settings真实窗口通过，默认v1/v2也各2headless+2DOM+七测量通过（completed=true/nativeSettingsChecked=false），最新设置报告同字段为true；日志与JSON交叉核验。TS最终Astrolabe诊断仍Unknown，未计通过。
- 仍无设置RPC、GUI字体应用、真实notify监听、跨文件/数据库原子性或跨平台验收；CC0和发布门禁保持。原暂存联合确认检查点与后续工作树修改分开，未绕过GPG签名。

## 检查结果

- Rust：43项pulse-pty测试通过（profile目录、启动字段、WSL UTF16解析与本地环境）；107 项 pulse-term 单元/属性测试通过（新增2项bound resize ACK/no-op/取消/失败保持grid回归与1项subscribe commit gate回归）；pulse-ipc 33项通过（新增有序Repair、NoCredit稳定性和大整数边界）；pulse-app 97项测试通过（新增配置上下文与注册视图设置授权回归；新增2项存储偏好到目录恢复回归；含4项可信活窗口transfer与3項刷新前批量view退役/Entry身份/捕获限流保持回归）；额外178项原上游测试通过；原生适配器default/encoding-probe各63项、login-probe66项测试通过（fresh生命周期与双额度先准入再claim回归）（新增导航期间在途页面许可撤销与无peer默认scope隔离回归；保留只读page2/两阶段收据回归）；xtask默认20项负向测试通过（含真实PTY迁移2项报告、目标runtime/display收据和codec/外部WM/GBK模式错配）；本轮未修改parser补丁或上游源码。
- TS：246 项测试通过（新增2项帧截止安全诊断及2项单调时钟截止反例；1000ms/两动画机会要求不变）（新增4项只读新页wrapper nonce参数、pagehide/迟到任务、渲染失败、混版/截止清理回归）（新增5项目标迁移capture的真实headless中文渲染/ACK确认顺序/取消/混版/确认失败回归；原park/PaneVisibility回归保留）；tsc、Biome、Vite 构建通过；负向类型用例确认 ID、camelCase 配置和二进制 Channel 参数的错误会被 tsc 拒绝。
- Rustfmt、clippy `-D warnings`、初始依赖分层检查通过。
- 原来的默认特性 cargo-deny 通过不足以覆盖原生依赖。现已改为全特性审计：来源与 bans 通过，许可证及维护公告失败；没有放宽白名单或添加公告 ignore，详见 [待审查依赖门禁](./dependency-review.md)。
- pnpm audit 无已知漏洞；初始化时发现 Vitest 4.1.0 的公告，已升级到 4.1.11。
- Astrolabe 已用于骨架、Rust 符号精确引用、TS 文件依赖范围及 Rust 诊断。TS 语言服务器诊断超时，未取得安装授权，不能视为通过；TS 的类型证据来自 tsc。

## 终端配色转换基础（Windows Terminal / Alacritty / Ghostty；纯后端）

- `pulse-store::theme` 提供有界 `import_windows_terminal`、`import_alacritty`、`import_ghostty`、`parse_terminal_fragment` 和终端片段编码。只处理显式传入文本，不选择文件／路径、UI appearance 或 Radix 色系，也不改变终端恢复协议。方案名仅作为显示文本，不能用于路径／CSS／HTML。
- 9 项定向回归通过：ANSI16 顺序与 cursor 映射、64 组 RGB 编解码、可选颜色不填默认、中文／引号名称往返、非法名称和颜色、重复／缺失／未知键拒绝、64 KiB 边界、终端片段不误收完整 UI 文件。WCAG 黑白为 21:1、同色为 1:1；`#777777` 对白色约 4.478 并告警，`#767676` 大于 4.5 无告警；低对比度不改原色。
- 新增 Alacritty 8 项、Ghostty 8 项定向回归，三格式共 25 项：必需 ANSI16 完整性、明确色槽映射、opaque HEX 各格式拼写、可选 cursor/selection、TOML 往返、重复／未知／不支持字段拒绝、独立名称与文本预算、动态色拒绝和共享 WCAG 告警均通过。名称先检查字节预算再扫描 trim／控制字符，不因外部巨长名称做无界扫描或复制。
- `mise run probe-theme` 已直接通过同一合成方案在三种格式的精确等价、终端 TOML→重新解析及错误无原文泄露验证；已注册 CLI usage、mise 和 CI。Rust 严格 lint 与 Astrolabe 编辑后 Exact 诊断通过。
- 仅在 `pulse-store` 启用已缓存 `toml` 的 `display` feature，未新增包／升级版本。最终完整核心检查通过：150 项存储（另 2 项手动夹具 ignored，不计通过）、97 项应用、246 项 TS；此前同依赖组合的 63 项原生测试已通过，新增纯解析不改变原生接口，固定配色探针、五进程设置链与绑定漂移检查通过；审计退出 5，仍为原有六项许可证和两项维护公告失败，未新增豁免。现有 GPG 暂存检查点仍独立保留，未无签名提交。

- 完整 Alacritty 检查首次为 141/142：`current_callback_holds_metadata_lock_and_releases_it_after_return` 在回调返回后瞬时 `try_lock` 与后台监督线程竞争，不能据此判断回调锁泄漏。生产代码未改；回调内断言强化为精确 `WouldBlock`，紧接第二次真实授权回调证明前一个 guard 已释放，worker 实际退出后再要求瞬时锁可取得。三项 guard 连续 24 轮及后续完整 142／150 项存储套件通过；Astrolabe 夹具与两新格式的编辑后诊断为 Exact。原失败证据保留于 `target/probes/theme-alacritty-core-final.log`，修正后的验收见 `theme-alacritty-final.log` 和 `theme-three-format-final.log`，未跳过测试、增加生产重试或放宽时间约束。

## VS Code／iTerm 配色与统一导入入口（纯后端）

- 在前三格式上新增 VS Code 10 项、iTerm XML 9 项、统一入口 5 项回归，共 49 项主题测试。VS Code 同时提取完整 ANSI16 与六个固定语义 UI 变量，组合片段不能被终端-only parser 接受；精确名称策略、512 颜色／32 JSON 深度、重复和 Unicode 转义重复键、缺失字段、alpha／JSONC／include 拒绝与忽略项计数通过。token grammar 不应用，不加载 schema 地址。
- iTerm 子集只接受明确 sRGB 的完整 ANSI16，浮点到 RGB8 为最近整数映射而非浮点无损恢复；alpha 只接受缺省或 1。XML 预检限制 4 层／2048 事件／单 root，拒绝未定义实体、外部／内部自定义 DTD、非 UTF-8 声明、binary plist、额外色槽／色域、深层／data 标签和尾随文本。标准 Apple DOCTYPE 可识别但无网络／文件访问。预检针对锁定 plist reader 会忽略未知实体的行为，不能直接把其成功当精确输入解释。
- `import_theme` 必须显式选格式，Windows Terminal／VS Code 禁止名称覆盖，其他格式必须独立名称；无内容／扩展名嗅探或默认回退。私有结果保留终端、可选固定 UI 覆盖、未映射色数量及量化说明。固定 `mise run probe-theme`／现有 CI 覆盖五格式逐一导入与合成配色一致性、组合片段保留、TOML 往返和 WCAG 告警；不下载第三方主题，不读写用户文件。
- 最终 `check/test/probe` 与定向探针、五进程设置、schema 和 binding drift 通过：174 项存储（2 项手动夹具 ignored，不计通过）、97 项应用、246 项 TS，默认 desktop／settings-probe 原生各 63 项及严格 lint 通过。三个新增模块 Astrolabe 编辑后诊断 Exact；引用／依赖查询含 stale 时不计精确通过。日志 `target/probes/theme-five-format-final.log` 与独立审计记录 `theme-five-format-audit.log`；audit exit 5 仍六项许可证及两项维护公告，sources/bans 通过，无新增豁免。
- GPG 原有 14 文件暂存边界保持，后续改动仅工作区，未绕过签名。完整主题基底／appearance、落盘、目录监听、GUI 配色／光暗切换、跨平台主题文件操作、base16／base24 YAML 均未计完成。

## 完整主题数据组装／解析（无文件 IO／GUI）

- `ThemeDocument::from_import` 连接已验证导入、显式 light/dark 与 Radix 基底，VS Code 的固定 UI 覆盖完整保留。`parse_theme_document` 严格解析文档既定 scalar appearance、ui neutral/accent/overrides 与完整 terminal；拒绝未知／重复／缺失字段、auto appearance、任意 CSS 变量、字体／间距覆盖和透明／表达式颜色。所有显示名称仍为文本，不作路径。
- 六种中性色、31 个已知 Radix 色系／opaque RGB8 强调色选择只定义数据，不生成 CSS 色阶。UI 内部 wire 转换后验证 28 个语义 token，公开 UiThemeBase 无 Deserialize 旁路；公开 ThemeDocument 的字段私有，仅校验 constructor/parser 可构造。5 项新文档回归和原有 49 项主题回归通过；固定探针逐一五格式×两种 appearance 共10完整文档往返，组合覆盖与独立终端投影保持，完整文档不能被片段 parser 接受。
- 最终核心 check/test、固定主题探针、五进程、schema 和绑定漂移通过：179 项存储（2 ignored 不计通过）、97 项应用、246 项 TS；未新增依赖／生产原生入口，沿前一阶段两种 feature 各63原生测试与 audit exit5 的有效边界，不重复过关项。Astrolabe 编辑后诊断 Exact；日志 `target/probes/theme-document-final.log`。文件落盘、目录监听、生成 Radix CSS／种子色阶和GUI切换未计完成；原暂存14文件仍保持独立，GPG未绕过。

## base16/base24 有界导入与显式 ANSI 投影

- 从 tinted-theming 官方 `styling.md`／`styling/base24.md`／`builder.md` 核对映射：base16 当前规范亮色重复 normal RGB，base24 使用 base12–17；亮黑／白分别 base03／base07 与 base02／base06。支持 common palette 与受限 legacy 字段布局，系统由调用者显式选，原始 16／24 色、名称与元数据保留，不执行模板或访问外部资源。
- YAML 事件入口限制 64KiB／192 事件／2 层 mapping，只接受无 tag／anchor／alias 的 scalar 元数据与颜色；重复／混合／额外槽、多文档、序列与深层容器拒绝。10 项新回归和前 54 项共64主题回归通过，另256组生成色样核验映射与源色保留。普通文档／片段序列化精准拒绝隐式丢源色；只有显式投影 API 可输出当前 ANSI16 模型，原始额外槽不在投影 TOML 内。
- `probe-theme`／既有 CI 现覆盖七格式、14 个完整 light/dark 文档往返；五格式合成配色仍精确等价，Base16/24 单独校验官方映射、原始色保留、投影说明和默认拒绝。完整check/test：189存储（2 ignored不计通过）、109终端、97应用、246TS；设置五进程、schema／binding drift、两种原生feature各63及严格lint通过。Astrolabe新模块编辑后诊断Exact，stale引用不算精确证据。
- `target/probes/theme-seven-and-spacer-final.log` 最后真实v2阶段遇到500ms native visibility query截止，组合退出1，不能记完整组成功。单独已有Openboxwrapper＋新Xvfb的默认v2复验完成2headless／2DOM／7组测量，日志 `spacer-native-wm-compat.log`；未改截止、operation模式或fail-closed判据，原超时未定位。独立audit `theme-seven-format-audit.log` exit5仍六项许可与两项维护公告，bans/sources通过，未加豁免。

## spacer 组合字符的固定恢复反例

- 完整检查缩减出 `中 é / CR / DCH1 / REP3` 的宽字spacer组合符号丢失。合成head上的marks会随DCH丢弃；当前重放在删head之后恢复首列marks，其他有两列scratch空间的位置经临时spacer将零宽符号附到目标，再ECH清scratch，避免普通覆写的clear_wide清除左格marks。未改Alacritty源码／私有状态。
- 10位置×5种REP／ICH／查询／alt／DCH后续逐格指纹、模式、光标和回复一致，原exact属性断言不变。末两列当前局部算法尚不能精确重建，新增明确nonexact说明；不是证明VT永远不可表示，更不是全S2通过。当前完整源码／Rust单元与Astrolabe编辑后Exact诊断、严格lint通过。
- 30,000次Rust属性恢复通过，`probe-snapshot` 新增首列／中间固定案例与末列非精确案例；600随机案例仍独立报告approx与原始解析分歧，条件M1|M2为311/311，risk矩阵18/372差异均标记（11精确repair、7明确不支持／延期）。首列spacer组合案例xterm原始解析不同，不能把Rust恢复等价当双方解析一致；原有近似／defer／parser gate保留。
- 上述GPG原14文件暂存边界仍保持，后续改动未混入索引，未绕过签名。该阶段尚未验收主题文件保存；后续 Linux 后端持久化见下节，目录监听、Radix CSS／RGB seed色阶、GUI配色和真实跨平台主题操作仍未计完成。

## UUID 主题文件与共享私有文件边界

- `theme_file::ThemeFile` 同步 Rust-only API，仅调用者显式提供可信私有目录和非 nil 后端 UUID；文件固定为 `<uuid>.theme.toml`，显示名从不作为路径，快照绑定目录设备/inode、slot、原文件元数据及字节内容。空快照亦不能跨主题／跨目录使用；ID／快照不构成窗口权限。
- 从 SettingsFile 提取封闭 `private_file` IO，共用原 no-follow／有效 UID／权限／单硬链接检查、显式解锁、固定锁、随机临时文件、两次完整 CAS、原子 rename 和目录 fsync；Settings 保留原 `.settings.lock`，主题使用 `.themes.lock`。不存在目录不自动创建，旧目录／文件不修权；主题读取的坏文档、空文件、非法 UTF-8 和超额分别拒绝，不以默认色回退。保存完整规范文档，不承诺保留注释。
- 6 类真实主题文件回归、Snapshot 无 Debug 的编译拒绝、严格 lint 与 Astrolabe 两新模块 Exact 诊断通过；精确引用查询 Unknown/stale 不计通过。原设置 12 项严格准入和真实进程强杀探针继续通过。
- `probe-theme-file` 已在 mise／固定 CLI／Linux CI 注册，四个不同 PID：提交者和旧捕获者并存，另一子进程在 Loaded 后未获 GO 被杀退并回收，原文件仍完整；提交者成功后旧捕获者准确 Conflict，新读进程重开相同完整文档。回执限 1024 字节／5 秒并校验 schema2／PID／phase／强制 background_io_checked／枚举、未知／重复字段及非零退出；所有子进程有界退役，非 libtest 伪子进程。两项负回执测试通过。
- 同步保存阶段完整check/test通过195存储、109终端、97应用、246TS、36CLI、63原生及设置／源／分层／Profile旧新探针。背景IO阶段最终复验通过203存储（2 ignored不计）、10文档测试、36CLI、63原生、全项目严格check、binding漂移及四进程／源／分层探针；终端／应用／TS代码未改，前述结果保留。当前 Linux 夹具不是断电、macOS 运行时、Windows ACL、完整目录链／TOCTOU 或 GUI 应用验收。非合作同 UID 编辑者仍可能在最终比较后竞争；rename 后确认失败返回 Uncertain，必须重读而非自动重放。尚无目录发现、删除管理、设置／主题写入 IPC、文件监听或 UI 应用。
- `theme_io` 已继续接后台调度：非Clone owner绑定已打开可信文件、cloneable weak handle；单进程全局单job额度在enqueue前取得、实际blocking闭包退出才释放，没有无界队列。runtime不可用／Busy不入队；queued cancel／owner Drop跳过IO，active cancel／超时不释放额度、丢回执不重试。ready等待实际join并检查owner仍开；返回快照不是窗口授权或持续新鲜性。文件打开仍要求可信后端自行调度，不在GUI线程同步打开。
- 8项真实IO与确定性屏障回归：读／保存／重开及精准Conflict、无runtime／失效weak、active取消与跨owner Busy、queued skip、ready超时保持额度、owner销毁、已commit后close拒绝回执且文件持久存在、panic精确Worker及额度释放。四独立CLI进程schema2现在实际通过此API，旧同步回执不能替代。Astrolabe新module／双进程夹具最终编辑后诊断Exact，名字匹配调用图stale且同名误报未计精确。
- 背景IO完整套件初次遇旧设置双进程Disconnected，定位夹具两个非阻塞load并发争锁；旧日志没阶段信息，不能完整追认原始child退出原因。现以first Loaded再spawn second Loaded排序，两进程仍同时活并持同一提交前快照，精准commit／Conflict原断言保持，不加重试／截止。24轮＋原12设置探针通过；补只含固定phase／PID／退出状态的失败诊断，生产文件锁与CAS逻辑未改。最终整个203存储与消费者复验通过。

## 主题选源与显式发布边界

- `pulse-app::theme_context`已接有界后台主题读取：同实际 IO owner 是 no-op，不同 owner 即使同 UUID 也撤销旧 epoch；A→B→A不会复活。候选需显式 confirm，外来 context、closed／耗尽、旧 epoch 与旧 publication token拒绝；不同选择清 last-good，同源坏读取保留明确 stale 的旧文档，缺失不补默认。这层本身不授权窗口；注册视图授权见下节。GUI CSS／xterm 配色、文件监听或持续文件新鲜性未计完成。
- 13 项真实文件／确定性屏障回归通过：模型读完不自动发布、foreign同源、sameUUID新 owner、ABA、无操作更新、坏文件 last-good／缺失、queued cancel与timeout占位、source close、context Drop共享 source存活、全局 Busy不消费第二槽/token、runtime缺失与计数溢出。两个 compile-fail证明 owner不可Clone与 published epoch不可篡改。
- 实际 blocking 闭包保留 opaque lifetime guard；运行时关闭的排队任务确实取消时观察最终 guard Drop再释放槽，不与 active工作混淆。另有第9项真实 IO屏障验证已进入操作后的 runtime关闭：原监督future退出，额度／guard仍保持到实际blocking退出。原8项取消／commit／panic回归均保持。该证据不同于断电／文件系统故障或GUI应用。
- `probe-theme-file`四独立 PID 回执升为 schema3，mandatory context_checked：每个子进程实际读候选、invalidate后精准Stale拒绝、重新捕获并确认；保存／Conflict后重新确认已提交主题，独立reader关闭context不关闭共享source。旧schema2仅IO回执不得替代；1024byte与5s准入、未知／重复／缺字段拒绝、退出和join门均保持。固定四进程探针、严格lint与Astrolabe最终context／IO诊断Exact通过。最终完整验收通过：204存储（两项ignored不计验收）、110应用、109终端、246 TS、36 CLI，两个原生feature各63；四主题PID schema3、原12设置文件／12后台源／5进程scope、typed bindings漂移及严格check全部通过。未重跑未变的真实窗口或已知失败audit，既有许可证／签名门禁保持。

## 注册视图主题授权

- Binding唯一构造挂独占ThemeContextOwner；其自动Drop统一覆盖replace、retire、transfer、active窗口close、completion与shutdown。WindowToken/session/generation授权先于Unconfigured/Closed source检查，registry pointer及opaque candidate来源另校验，其他view不能消费本view槽。sameSource无操作、sameUUID不同owner撤销；current捕获在registry/context短metadata事务中，无锁跨await。ready的失败结果也在返回前重授权，旧入口拒绝后监督实际退出，不遗留未join读取。
- 9项真实Rust actor／私有文件回归通过：授权排序、确认前无current、完整文档/当前捕获及trusted invalidation、换generation迟到read/snapshot拒绝、导航旧候选拒绝与新view未配置、移窗+源窗口销毁、active绑定直接窗口close保持actor及共享IO、sameUUID重配与noop、跨session pending槽保持、强制相同标签仍拒绝foreign registry。13context测试组共享测试IO锁，防测试并发触发单额度竞争；业务I/O准入未放宽。这些registry tokens不是实际Tauri窗口／DOM主题验收。
- 最终完整注册层消费者通过：119应用、36 CLI，两个原生feature各63、五进程settings scope、四主题schema3与typed bindings漂移及严格check。先前204存储／109终端／246 TS源码未变，已有完整证据不重复；新module与最后fixture Astrolabe诊断Exact，名字匹配调用图stale／同名误报只作辅证。原14文件签名暂存边界保持，发布审计和真实WebView旧未定位超时风险未放宽。GUI主题应用／真实窗口主题载荷、主题端点／用户目录发现与自动监听仍待后续接入。

### 隔离的原生窗口主题回执（Linux组合验收通过）

- `window_theme_probe.rs`仅随settings-probe启用，初页私有UUID11文档A→实际导航新页UUID12文档B→移窗并销毁源窗口后目标A；目标检查位于实际Channel绘制ACK之后、view guard撤销之前。完整模型读取/确认/current投影一致，原A/旧B快照不能在回到A时复活，旧页待确认候选必须精准Stale/Forbidden。
- 目标阶段通过有界ThemeIo执行临时夹具CAS，可信invalidate使旧候选精准Context(Stale)并清current，再确认新文档；关闭两个共享source使current不可用、原PTY仍Running。不扫描用户主题目录、不写用户文件、不加载模板/资源、不应用CSS或xterm配色。
- 64项feature原生测试、默认与feature strictlint、双字段从完整baseline构造的missing/truthy/mixed反例、生成绑定/TScheck及Astrolabe新工厂Exact诊断通过。新增nativeThemeChecked在两个页面报告强制对照feature mode，不允许旧报告缺字段或普通模式false替代主题真回执；同一mise/CI任务继续执行，没有重复入口。
- 最终实际四轮主题窗口以及默认v1/v2均通过：每报告completed=true、2 headless+2 DOM、7测量组；settings-window两个页面nativeThemeChecked=true，默认两个模式的两个页面均false，4轮initial/reload/target阶段齐备。整体check、绑定漂移、36 CLI与63默认原生通过；64 feature原生此前已过。原14文件签名暂存边界与原生timeout/依赖发布门禁不变。这里只验收实际窗口授权后端主题回执，未向页面交付绘制数据，GUI应用、生产选源与跨平台行为另行验证。

### 有界主题绘制投影（完整核心验收通过）

- `ThemeDocument::paint_data`生成immutable、Serialize-only的ThemePaintData，private字段只允许从validated完整文档构造。4096字节compact JSON包含固定u8 RGB三元组、ANSI16、4个可选cursor/selection色、28个固定UiColorSlot，以及appearance/neutral/已验证accent选择或seed；不包含name、文件路径、源配置、任意CSS变量名或view权限。缺失颜色不填默认，既有terminal 4.5:1告警原样携带，不自动改色、不假称已解析Radix或检查UI对比度。
- 7项回归覆盖完整通道/可选缺失、全部28slot与限额、标签隔离、显式appearance/seed、未知CSS fail-closed、未舍入对比度阈值；compile-fail明确拒绝反序列化。七格式×两appearance的14完整主题往返绘制投影一致，直接probe与strict store/CLI lint、Astrolabe编辑后Exact诊断通过。原生工厂与前端实现未变，不重复已通过的12实际窗口轮次。
- 最终完整核心通过：211存储（2 manual ignored不计验收）、119应用、109终端、246 TS、64 feature原生，以及四主题/五scope独立进程、绑定漂移和strictcheck。typed主题端点、active-view/CSP绘制guard、Radix CSS解析及实际GUI配色应用尚未接入。数据相同不能复活旧context或窗口授权。

### 前端主题v1解码与真实跨语言数据探针（通过）

- 独立投影`version=1`不改变terminal stream v1/v2；`paint-data.ts`只接compact canonical JSON，按UTF8限制4096字节，精确字段/16 ANSI/4 optional/u8 RGB/31 family或canonical seed/28 unique semantic slot检查；未知或重复字段、escaped key、非canonical numeric/whitespace、prototype slot拒绝。canonical重编码放在有界结构验证后，避免遍历恶意深树。所有输出deep freeze；RGB到hex边界再次验证，不执行主题输入、不创建DOM/CSS、无nonce或view应用授权。
- 12个frontend回归覆盖上述拒绝、字段presence、冻结、未填optional默认和静态不含输入的错误；全部258 TS、typecheck/Biome通过。7 Rust投影（v1 header）、serialize-only compile-fail与strict CLI通过；先前211存储/119应用/109终端/64原生完整消费者已过，其未受影响部分不重复。
- `probe-theme-paint`在mise与CI注册，构建现有features的xtask后执行已有tsx；validation-only `probe-theme-paint-json`输出14个固定合成数据，普通probe不打印机器载荷。实际Rust→TypeScript消费14 complete documents、16 ANSI、两appearance、4 UI override项，通过10秒子进程/64KiB stdout边界；不把两语言各自手写fixture当互通证据，不写用户文件。
- Astrolabe最终Rust投影Exact无诊断；TS诊断Unknown/未发布不得计通过，AST符号stale亦不作精确引用证据。只读核验get_languages为Ready但选中现有vtsls，同时现有mise typescript-language-server 6.0.1与workspace tsserver均存在；不是确认缺少安装，未安装或重启共享服务。TS功能/类型检查依据实际tsc、258 tests与跨语言探针，MCP TS诊断链风险保留。
- 仍未完成：实际typed页面交付、active-view/epoch/CSP应用guard、Radix自定义seed解析、UI对比度、CSS/xterm应用与backend-owned terminal默认颜色查询同步。数据相同绝不授予输入或复活旧epoch；原14文件GPG暂存与发布审计门不变。

### 视图-owned UI主题覆盖与nonce资源（受控真实WebView通过）

- `ViewThemePaintGuard`直接复用ordered Begin的`ViewScope`，不发明backend generation；本地epoch以身份撤销A→B→A，最多一个reader，abort/close保留占位至reader promise实际退出。reader仍须来自后端window/page/generation/source授权，不把本地guard当window权限。坏数据保留明确stale的last-good，prepare不产生可见效果；tentative surface在apply前登记，source/view检查覆盖prepare/apply/旧资源cleanup重入，活跃/已释放surface不重用。16项回归含late成功/失败、关闭/换绑、prepare/apply/cleanup重入、资源故障全清理且关闭view不关闭session。
- `UiThemeOverrideTarget`只输出28个固定语义CSS变量的opaque RGB8 hex，不执行任意CSS、HTML、URL或主题脚本；借用既有`nonceDocument`逐页nonce，缺失/未替换拒绝。不写style属性；CSS-only selector身份不是binding generation，每目标独占、兄弟不串色，最多old+tentative两sheet。apply检查sheet和computed custom-property readback，关闭撤销selector并尝试全部sheet清理；10项DOM ownership/nonce/cascade stub回归明确不算浏览器CSP通过。未解析完整Radix，未改变xterm theme/options或backend-owned default-color replies。
- fixed `ui-theme-overlay`挂到每actual Begin的真实DOM scope，验证全部28变量、A→B→A晚到/Busy占位、坏输入last-good、兄弟隔离、替换仅一sheet、关闭清理与原值恢复，并比较xterm options.theme完全未变。native `DomEvidence.uiOverridesChecked`为强制bool；Rust验证false拒绝，CLI逐一拒绝missing/null/false/string/number/旧证据，生成绑定同步，default63 native与strictlint通过。最终新增fault分支26定向测试通过；最终全部284 TS、37 CLI、14实际Rust JSON互通、完整check及bindings漂移通过，记录`target/probes/view-theme-guard-final.log`；未改动的211存储/119应用/109终端不重复。mechanical detector现有binary无下载运行，结果空列表。
- real v2首次incomplete `session.native_query_failed`（2 session/0 DOM）在overlay之前触发既有500ms visibility query deadline，未计CSP通过。保留`view-theme-guard-native.log`；独立Openbox v2/v1以及settings4轮复验均completed=true、2 session+2 DOM、7 measurements、UI bool=true，默认nativeTheme=false/settings=true。不得称原query超时已修复；1秒frame/500msquery/40秒helper截止不变。真实CSP证据仅指固定受控UI覆盖和computed变量，不是像素/性能/手动/跨平台或production完整主题。
- Astrolabe native flag最终Exact无诊断；新frontend guard诊断Unknown/未发布不计通过，AST stale引用同样排除；未安装/重启已存在的服务。本阶段当时未接实际backend投影；后续固定peer页面typed交付见下节。完整Radix/custom seed/UI对比度、terminal默认色与动态OSC状态同步仍待完成。原14文件GPG暂存边界与已有发布门保持不变。

### 固定peer页面实际主题投影交付（受控真实WebView通过）

- `window_theme_delivery`三typed命令已注册并实际生成绑定。可信settings工厂在已绘制页面安装一个challenge，前端不可选择路径/theme UUID；exact WindowToken/run nonce/current page nonce/actual generation在短同步锁内校验，未captured/rendered、foreign main/registry、无challenge、错generation/ticket/阶段均拒绝。read交付≤4096bytes canonical独立v1 paint JSON，无theme name/path/完整导入；仅UI overrides应用，ANSI/terminal默认色不变。
- read→confirm→nonce CSS computed readback→drawn为单次序列，confirm/drawn各自核验RegisteredThemeSnapshot source当前性。source A→B→A（即使同文件和相同generation）、重新publish、导航不能复活旧receipt；supervisor取消/timeout按ticket身份RAII清单slot，旧guard不清新challenge。真实actor/文件测试直接abort监督future并join见证slot和oneshot关闭，session仍Running。前端3新回归验证顺序、foreign run/page、错generation/坏ticket、confirm中pagehide与旧hook清理不删除replacement；mock不算CSS/CSP证据。
- 新测试与旧fixed_sources共享生产global theme IO quota=1，并行首次造成旧fixture Busy；已查明是两个独立fixture同时申请全局quota，而不是认定OS锁或丢数据。仅两个测试的I/O片段用test-only TokioMutex协作，保持生产quota和parallel suite，未sleep/retry/扩大限额。65 settings-native、63 default-native、37 CLI、287 TS、严格check/clippy、bindings漂移与14实际Rust JSON互通通过；日志`peer-theme-delivery-unit.log`、`peer-theme-delivery-faults.log`、`peer-theme-delivery-front-faults.log`、`peer-theme-delivery-final.log`。原未改动211store/119app/109term不重复。
- real `probe-settings-window`四轮共8次`session.native_theme.peer_paint_checked`：固定私有文件的`--bg-base`从#112233到刷新页#445566，actual Begin/ViewScope→typed IPC→nonce sheet→computed readback→独立drawn回执完成。`settings-window.json`、默认`channel.json`/`channel-v2.json`均completed=true、2 session+2 DOM、7 measurements；`nativePeerThemePaintChecked`在session和domSession内settings=true/default=false，强制CLI逐项拒绝missing/truthy/mixed/普通证据。原deadline未放宽；日志`peer-theme-delivery-native.log`。新Rust模块与最终测试Astrolabe Exact无诊断；TS既有Unknown诊断链不计通过。
- 不把瞬时确认视作跨进程原子事务：confirm后源再变化可能需要撤销通知/再取，drawn拒绝不等于从未出现过旧颜色。没有生产source listener/完整GUI热加载、目标main实际主题绘制、完整Radix/custom seed/UI对比度或terminal palette/OSC默认查询同步。既有visibility_query_deadline风险、TS MCP Unknown、14文件GPG暂存与发布审计门不变。

### 固定移窗main主题投影（真实组合验收待回执）

- `window_theme_target`复用唯一challenge槽，5个typed命令仅提供factory mode、ready、read、confirm、drawn；无主题路径/UUID选择或写入能力。注册本地main才可查mode；default=false。目标必须同WindowToken/run/current transferred session/generation，commit ACK和真实summary确认后还须前端显式ready；ready等待并入原3秒捕获总截止。snapshot-only retained renderer仍拒绝连续数据和第二事务，非live才可取得target权限，源peer和target的challenge不可互换。
- trusted factory生成claim，targetread/confirm/drawn复验当前RegisteredThemeSnapshot和主document epoch。main的trusted Started回调清ready/challenge并递增epoch，饱和MAX永久拒读；不关闭window/actor。真实文件+actor+迁移测试覆盖source窗口销毁、lost draw/ready、未就绪、未ACK/live、foreign/window/run/gen/claim、阶段/重复、ABA、导航和MAX。epoch反例先在MAX-1且同actual-ready授权成功，排除not_ready掩盖。
- factory对headless/DOM主会话均创建独立真实、现有static `terminal-host`尺寸的目标容器，不复用主ViewSlot；仅首次数据通道仍为headless。nonce CSS对28变量computed读回并恢复原值，释放renderer/styles、移除临时host后才drawn。source关闭与主PTY接续仍由原fixture独立核验。不是完整Radix/UI视觉主题、像素/CSP性能或跨平台验收，xterm配色及backend OSC默认查询未改；confirm仍只是瞬时检查，源通知/生产自动热加载未完成。
- 修复过程失败实证保留：原headless没有DOM却等待ready，触发`session.transfer_view_deadline`（`target-theme-ready-headless-failure.json`）；临时host位于报表下方被既有viewport geometry拒绝（`target-theme-geometry-failure.json`）；改为可见后4次target绘制通过，但最后主helper40秒code2使整组失败（`target-theme-helper-deadline-failure.json`、`target-theme-native-visible.log`）。后续改为所有factory目标独立host且绘后即时移除。未延长1秒frame/target、500msquery、3秒capture、40秒helper或180秒driver；不得据定向成功声称原helper/visibility超时均已修复。
- 66 settings-native、63 default-native、37 CLI、296 TS及check/clippy/bindings漂移已通过；新增释放故障反例与297 TS验收结果另记`target-theme-release-final.log`。`target-theme-native-isolated.log`已终态：默认v1/v2 completed=true、2 session+2 DOM/7 measurements且target=false；settings在第3次target绘制后`input.transport`失败，不计组合通过。新增固定enum输入诊断只记录phase/kind/generation/bound/running，不输出文本/nonce/path/动态Display；67 settings-native及严格lint通过，`target-theme-native-input-trace.log`再次见主helper40秒code2，故意wrong-generation的stale负向记录不能认定输入根因。曾实验`Utf8Stream.feedFrame`保留4096字节增量解码、64KiB硬上限及跨帧/EOF状态，仅将同一8KiB帧合并为一次xterm write回调；1-frame/8KiB+header credit、全量bulk、原2-rAF ACK fence、截止均未减弱。3项额外回归含真实headless字符/width/颜色/历史/光标与旧写法一致，全部300 TS通过（`target-theme-frame-write.log`）。实际`target-theme-frame-native.log`三模式均主helper40秒code2，调度优化假说未通过，已完整撤回帧写改动和其3项实验测试。原4096解码/写回调及原8KiB预算恢复，297 TS、67 settings-native与严格检查通过（`target-theme-progress-trace-final.log`），不是原超时已修的结论。仅增有限合法ACK每64帧与固定go/resume的匿名phase/时间/parsed计数；`target-theme-stage-native.log`终态通过：4 target/8 peer绘制，completed=true、2 session+2 DOM/7 measurements且headless/DOM target=true；DOM helper36.119/36.559秒，8KiB上限而实际数据帧多为4KiB，每64ACK计数观察到bulk/paint链占较大时间，40秒总期限余量小，原失败原因仍不声称全修。恢复路径后`target-theme-restored-compat.log`默认v1/v2亦完整2+2/7通过、target=false，最新三个JSON都completed=true且UI=true。两个最终Rust模块Astrolabe Exact无诊断，TS publishDiagnostics超时Unknown不计通过，未安装任何服务。14文件旧GPG暂存边界与发布审计门保持不变。

### 主题源撤销观察与实际目标视图清理（只读受控验证）

- `ThemeIoClosure`、`ThemeRevocation` 为每 owner／publication 的单向、最新状态布尔通知，不积累事件或回绕序号。source close、context select／invalidate／close、替换 publication、计数耗尽均唤醒；noop 选择不撤销。观察者不保活 owner／registry／view，也不占用或释放文件 I/O 配额。`observe_view_theme_revocation` 在同注册层短锁内先校验 exact WindowToken/session/generation/registry/当前 snapshot，事件不是主题读取或重绘权限。
- 10 项新增真实文件/actor回归覆盖 ABA、1000 次失效合并、迟到观察、source close、弱 owner drop、替换发布、取消观察、poison close、两条 counter 耗尽、foreign／stale 拒绝、窗口导航／关闭和旧 supervisor 回收；11 IO、19 context、11 registry-theme 定向检查及严格 lint 通过。完整 `theme-revocation-core.log`：213 存储（另2显式平台测试未运行）、127 应用、109 终端、297 TS，以及四独立进程文件 receipt、check／bindings 漂移通过；不把零测试或 ignored 计作通过。
- 前端新增明确的 `ViewThemePaintGuard.revoke`：authority 失效撤掉样式并中止 pending，但读槽待实际 promise 退出；已有 `invalidate` 保留 stale-last-good 像素的策略不改。metadata可保持 stale，不允许旧模型变 current；cleanup 异常只使原 ViewHost fail-closed，尝试所有资源并屏蔽私有错误，不能影响独立健康 view。5 项新增清理／重入／tentative apply／slot／异常反例通过。
- 目标 fixed factory 加一个只读 `probeTargetThemeWaitRevoked`：真实 CSS 绘制后预留单次观察，通知 native ready；后端显式 invalidate 已捕获源，观察完成后复验同 main WindowToken/run/generation/claim/document epoch，前端对实际 Begin 的 guard 调用 revoke 并检查全部28个变量恢复，最后提交 single-use cleanup receipt。challenge 退休的 oneshot 立即终止待观察命令；main 导航不需关闭 source／registry／PTY，不留下等待源关闭的悬挂请求。整个握手仍在原1秒总截止内，未改 query 500ms／helper40s／流量预算。
- `theme-revocation-final.log`：67 settings-native、305 TS、tsc/Biome/strict Rust/bindings-check 通过；`theme-revocation-default-consumer.log` 真正 desktop 配置64 native测试和strictlint通过，无feature运行的零测试不算消费者验收。native真实文件测试还验证 early／duplicate cleanup、重复等待、外国 owner／run／gen／claim、导航取消而 source 与 actor仍活；3 TS反例拒绝通知失败、pagehide迟到结果和错误 generation 清理。
- 实际 Openbox `theme-revocation-final-native.log` 完整四轮：4 target、8 peer，最新 settings JSON completed=true、2 session+2 DOM／7 measurements，两模式 nativeTargetThemePaintChecked=true、UI=true；target标记必须完成通知和原值恢复后才置 true。DOM主helper33.279／33.901秒，原40秒余量风险仍未解除。之前 default v1/v2 恢复路径完整回执不重复测量，新接口在普通工厂无 challenge 仍拒绝。
- 编辑后 core/native Rust Astrolabe Exact无诊断；TS publishDiagnostics Unknown不计通过。只验证显式源失效的受控只读通知，不是生产持续订阅、真实文件 watcher、GUI自动重取、完整 Radix、terminal ANSI/OSC查询同步或跨平台视觉验收。未增依赖／许可证／主题路径或write IPC；旧14文件GPG暂存边界（563／28）和既有发布审计门保持。

### 完整 stock Radix UI 色槽与显式对比度门（受控只读绘制）

- `ui/src/features/theme/radix-theme.ts` 复用已锁定 `@radix-ui/colors 3.0.0`，只导入 opaque sRGB light/dark 色阶，不加载 alpha／P3、不新增依赖或近似生成色阶。6 neutral ×31 accent ×2 appearance 的372组覆盖全部28个固定语义槽、canonical v1 JSON往返与原终端数据不变；RGB seed 返回 `theme.ui.custom_accent_unsupported`，不能悄悄映射近似 stock 色。
- 按既定级别生成 neutral1–8/11/12、accent3/4/9/10/11及固定 red/amber/green/blue3/9/11；accent-contrast 选择对9/10两态最低对比更高的黑或白。trusted行政窗口选项强制amber，忽略并计数全部6个导入accent覆盖，但不授予管理员或窗口权限；目前仅纯模型回归，不是实际提权窗口验收。
- 独立WCAG sRGB计算检查25个正文前景/背景组合，低于4.5:1输出有界告警，纯解析不会自动改色；显式 `contrast: "reject"` 在样式分配之前拒绝绘制。实际 TransferViewProbe 按需加载生成器，以此门交给既有 nonce/view guard。新反例验证低对比/unsupported seed不能创建新sheet，原画面仅可作为显式 stale-last-good 保留，authority撤销仍立即移除。白/黑21、相同色1及#777/#fff参考值通过。
- 原受控深色背景#445566/#778899不足4.5仍保留为拒绝反例，不放宽门；fixture的UI背景与终端ANSI显式分离，UI深色改为#243546/#223344，原终端ANSI仍是#112233/#445566/#778899。Radix原始light11在若干有色背景仅约3.95–4.41，不能笼统声称所有stock组合正文AA通过；受控light主题明确覆盖accent/warning/success/info前景为官方light12的#0d3c48/#4f3422/#193b2d/#113264。默认light低对比仍告警/拒绝，无silent repair。
- `probe-theme-paint` 已在现有mise/CI入口加强：14份真实Rust JSON→strict decoder→full Radix28，保留全部导入覆盖与终端ANSI16，覆盖两种appearance；这是数据互通，不当DOM验收。`radix-theme-light-interop.log`11模型、67 settings-native、strictlint/tsc/Biome及该互通通过；`radix-theme-final.log`全部318 TS、64 default-native、bindings漂移与strictlint通过。Rust fixture编辑后Astrolabe Exact无诊断，两个TS诊断Unknown不计通过，未安装语言服务。
- `radix-light-native.log`实际Openbox四轮完整：first peer light→refreshed peer dark→移窗target dark，8 peer/4 target绘制、28变量computed读回及source失效后原值恢复；最新JSON completed=true、2 session+2 DOM／7 measurements，headless/DOM target=true、UI=true。先前纯dark四轮亦通过并保存`radix-dark-only-settings.json`。原1秒主题握手/500ms query/40秒helper和ACK/流量预算均不变；最后两个DOM helper34.630/36.882秒，旧期限余量风险未消除。
- 实物bundle为`radix-theme-Bh3beob8.js`15782 bytes（gzip约6.24kB），实际compiler产物有唯一dynamic import且无P3表达式；早期自写bundle探针引号错误/假定chunk名错误已纠正，不算产品失败或旧失败通过。普通窗口不调用factory绘制路径，默认consumer关闭验证工厂。
- 这完成已知stock色阶的受控UI资源生成/绘制，不是全局应用外壳主题、自定义seed色阶、主题选择/编辑界面、生产连续订阅/外部文件监听或终端ANSI/OSC查询同步。未打开主题路径、写入、输入或提权IPC；原14文件GPG暂存边界（563/28）与发布审计门保持，未重复未变化的storage/core/known audit。

### S14 终端捕获调色板与受控实际浏览器恢复
- 后端 `SessionBootstrap` 在相同 actor 捕获内只读携带 19 个基础色；`QueryPalette` 由已验证主题文档显式构造，光标色缺失才继承文字色。Registry 按真实窗口、绑定 generation 和读取状态拒绝外来/stale 捕获。基础色在恢复字节前作为 xterm 构造选项，不混入远端 OSC 或修改 v1/v2 帧；OSC 104/110/111/112 回退基础色。
- 首次实际 Openbox 样例失败为 `theme.target.deadline`，仅当前 challenge 的匿名失败回执定位 `session.native_palette.failed stage=3`：原测试误以为 RIS 清除远端 OSC 颜色。后端已有定向查询测试明确 RIS 保留覆盖，而实际 xterm 也保留；曾在受控视图加 parser RIS 清色钩子且一次实际窗口通过，但该做法会使渲染色与后端权威查询分歧，已撤回，不算正确验收。旧反例与钩子通过日志保留于 `terminal-palette-stage-native.log`／`terminal-palette-ris-native.log`，已用修正后语义重新跑实际窗口，四次目标颜色阶段均完成，但主 helper 超时使整个窗口验收仍不完整。原 1s/500ms/40s 门未延长。
- 受控浏览器恢复语义现要求：恢复前 19 基础色→4 远端覆盖→显式 104/110/111/112 回退→再次覆盖→RIS **保留覆盖**→再次显式重置才回退基础色。`probe-theme-paint` 校验七来源 14 个真实 Rust 文档的 19 色跨 TS 严格解析、UI 28 色与 cursor fallback；撤钩后 357 TS 检查通过；修正语义的 `terminal-palette-aligned-native.log` 四次 `target_paint_checked` 均在原1秒色检查期限内取得（含浏览器四类OSC与RIS反例），但最终 JSON completed=false、sessionRuns=2、domRuns=1，不能把颜色子项成功或旧钩子通过当作完整窗口验收。新增 `pulse-term` 直接 OSC 4/10/11/12 查询测试覆盖主题基础色→远端覆盖→更新基础色→RIS保留覆盖→104/110/111/112回退新基础色，定向测试和严格 lint 通过。
- 完整 `mise run check` 通过；首轮 `mise run test` 在已有 SQLite `edit_tests::independent_workers_merge_and_stale_snapshot_is_rejected` 失败，212 通过、1 Database 失败（阶段未知）；加入阶段标记后再次完整运行通过（213 store、112 terminal、357 TS），仍将首次失败保留为间歇风险，未宣称已修复。独立只跑该例一次通过不消除原失败；`mise run probe`、14文档跨语言 probe、bindings-check 后续均通过。默认 v1/v2 实际窗口各在第三轮40秒 helper 到期报 `session.native_exit_failed`；修正语义后的 settings 样例在最后两轮亦于约40秒到期报相同结果，三模式完整验收均未通过。进度显示实际 actor 已处理约3.11MiB 且连续 ACK，尚不能证明特定瓶颈或把允许耗时放宽；已有渲染门为每次写完再等待两次动画机会，不能跳过绘制信用门凑通过。Rust Astrolabe修改文件诊断 Exact，精确引用索引 stale不作证明；TS诊断 Unknown、语言服务不可用，不算通过且未安装。旧暂存与发布许可门禁不变。

### S4 后端主机密钥拒绝策略与有界 known_hosts 文本解析
- 新增 `pulse-ssh` 后端 crate；纯 `verify_host_key` 以最终连接目标的规范化主机名/IP及非零端口比较完整公钥。应用信任记录优先；同目标已知但密钥或算法不同报 `ssh.hostkey_changed`，未知报 `ssh.hostkey_unknown`，任一必需来源读取失败或超过 128 条视图报 `ssh.hostkey_store_unavailable`；均不进入认证、不会隐式写库或首次信任。系统记录只读文本解析限 256KiB/单行8KiB，支持精确及逗号主机名、非22端口、OpenSSH `|1|` hashed host；匹配 `@revoked` 直接拒绝，通配/否定及不支持标记或畸形内容拒绝而非降级为未知。证书无CA策略仍拒绝。
- 七项 crate 单测含 `ssh-keygen -H` 独立生成的公开 hashed 向量（主机与非22端口）、不同算法/优先级/轮换/文件大小/畸形/撤销，以及临时 `.ssh` 目录的只读文件正反例；47项 xtask 特性测试和严格 clippy、Astrolabe 新 crate Exact 诊断通过。新增 Unix `load_system_known_hosts_directory` 只接受可信后端提供的绝对路径，要求同UID私有目录与单链接、非他人可写的普通文件，通过 NOFOLLOW/NONBLOCK fd 读最多 256KiB，缺文件为空来源而目录缺失/符号链接/FIFO/权限/过大/读取失败均报 StoreUnavailable；非Unix显式拒绝。完整 `mise run check/test/probe-ssh/bindings-check` 在新Unix文件层后通过（7 core、213 store、112 term、357 TS）。最新 `mise run probe-ssh` 在loopback russh handler前从隔离临时文件只读装入同一目标：错误文件pin在任何密码认证前拒绝、畸形文件不降级为空来源、正确文件pin可完成密码认证；新增必需报告位 `systemKnownHostsFileChecked=true`，原PTY/UTF8/EOF与系统OpenSSH client完整回执保持通过。报告验证不代表生产SSH已接入。
- 只读取隔离临时目录，未读取真实用户 `~/.ssh`；尚未对父目录整个链条或并发修改提供完整 TOCTOU 保护，应用 known_hosts 表、审批与变更更新、真实设备兼容或 GUI 授权未实现。锁定 russh 的上游 `check_known_hosts_path` 把打开失败当空记录且可在 debug 级打印原始行，不用作生产信任来源。新包复用已锁加密依赖；本次 `mise run audit` 实际仍报6项原有license及2项原有unmaintained，禁打包规则和GPG暂存边界不变。
### S4 可选应用信任快照的持久化和实际认证前优先级（非生产授权）
- `pulse-store/ssh-trust` 只在可选特性编译：共用既有私有 SQLite `app_state` 与单个有界 worker，版本化记录最多 128 个规范化主机/端口/有界公钥文本，单行最多 64KiB；结构畸形/未来版本/重复目标/超限直接拒绝。存储文本不代表有效 SSH 公钥：上层 `pulse-ssh/managed-store` 必须逐条解析完整快照，任一密钥无效则整来源拒绝，不能回退到系统文件；该适配器在 russh 连接前完成校验，绝不转换为首访空库。每次变更须以同一打开句柄的 opaque 收据及单调修订 CAS 提交；空库跨库/跨句柄不可复用，删除仍保留修订墓碑，重复旧收据或修订耗尽不能重写已提交信任；关闭且尚未执行的请求不会写入，已经执行的提交不能靠取消回滚，丢失回执时必须重读。可信 Rust `edit_ssh_trust` **不证明用户批准**；从未注册页面 IPC 或自动 TOFU。
- 最初实现被 `mise run check` 按架构约束拒绝（禁止 `pulse-store → pulse-ssh`），已移除该反向依赖：`pulse-store` 仅维护有界原始记录，SSH 可选上层适配器验证全体公钥；改动后 `mise run check` 及 9 项 SSH 可选测试、4 项存储定向测试、严格 clippy 和 Astrolabe Exact 通过，默认 desktop 与默认存储依赖树没有 `pulse-ssh`/russh。完整 `mise run check/test` 通过（默认存储213项、终端112项、TS 357项），可选存储全套217项通过、2项标记忽略；47项 SSH 探针特性测试通过。组合脚本的240秒截止落在已编译完成的 `bindings-check` 阶段，单独按原命令补跑绑定漂移检查通过，不把被超时的组合命令伪记成通过。最新 `mise run probe-ssh` 的固定临时库和临时系统文件双源实测：应用 pin 正确而系统 pin 错误时允许实际密码认证；应用 pin 错误而系统 pin 正确时在任何认证前拒绝；另一个**非目标**的形状合法但公钥损坏记录也使整源在网络前拒绝，不静默回退；旧收据不能复活原 pin；关闭并重开数据库后修订和变更仍在。新增必需报告位 `managedTrustStoreChecked=true`，原固定 SSH 流程仍完整。没有读取真实 `~/.ssh`、连接真实设备或授予写权限。
- 当前只是 `app_state` 中的可选验证记录，不是规划的正式 `known_hosts` 表/迁移、外部文件自动导入、生产 SSH 连接或用户审批界面；也未验证多进程断电一致性和完整父路径 TOCTOU。`mise run audit` 仍报原有 6 项 license、2 项 maintenance 拒绝，许可与 GPG 门禁保持不变。重构后 `mise run check/test`、可选存储全套与单独 `mise run bindings-check` 均已取得回执；审计失败和之前 SQLite 间歇错误仍是独立风险，不能因本次绿色而移除。

### M1 本地会话启动预准入（仅后端，不是生产 shell 页面）
- `SessionRegistry::begin_local_launch` 为当前可信 WindowToken 在生成子进程前预留最多 64 个会话/未完成启动名额；普通 `register` 不能绕过预留容量。`commit_local_launch(&mut ticket, actor)` 在同一短锁下确认同一 registry、窗口仍有效、未撤销的票据、非重复 actor、容量和下一 SessionId，再原子以已登记会话替换名额；失败保留票据，必须先关闭并 join 原生任务再丢弃，否则仍会过早释放名额。关闭窗口仅撤销该窗口的票据而不释放其在途容量，也不关闭已有会话；应用 shutdown 撤销票据及已登记会话。
- 6 项定向测试通过：64 个同时持有的预留、直接注册绕过、foreign registry、旧窗口、shutdown、重复 actor、编号耗尽、新窗口在旧票据撤销后仍不能越额。Unix 固定 `/bin/sh` 真实 PTY 测试在拒绝提交后显式关闭并 join 子进程，在成功提交后关闭窗口仍自然输出并 EOF，原生退出码为 0；严格 clippy、Astrolabe 对两处 Rust 文件的 Exact 诊断通过。随后 `mise run check/test/probe/bindings-check` 一次完整命令退出0：应用136项、默认存储213项（2项忽略）、TS 357项及受控 probe/绑定漂移检查通过。随后隔离 Openbox 的真实 v2 WebView 单次回归**失败**：前两轮 headless 完整，第三轮 native helper `session.native_exit_failed`，在约40.6秒以 code 2 退出，DOM 轮次0/预期2、完整报告未生成。保持原固定40秒期限，不重跑或把此结果算通过；历史同类超时风险仍在，不能仅凭该错误断定由票据修改造成。原有发布审计与间歇 SQLite 风险未解除。
- 新增仅可信 Rust 可调用的 `local_launch::start_local_registered`：后台监督者在 PTY setup/失败清理期间持有票据，失败直到 native/actor join 才释放；成功提交时票据原子转换为已登记会话。单次 ready/accept 握手处理“已提交但接收方随即消失”的竞态：未确认回执触发关闭、原生 join、会话 reap；成功确认后后台监督者仍持有 native join，返回真实 EOF/exit 回执。4 项定向测试通过：无 runtime 时不预留、关闭窗口后的正常 PTY 完整 EOF、已生成原生子进程但接收方丢失确认时回收、取消后等待后台工作退出再复用名额；严格 clippy 与 Astrolabe 对新模块及 registry 的 Exact 诊断通过。监督者版本的 `mise run check/test/probe/bindings-check` 已全部退出0：应用140项、存储213项、TS357项及受控 probe/绑定漂移检查通过。
- 仅可信 Rust 可调用的 `start_profile_local` 先从当前 `AutoProfileCatalog` 解析身份、程序和已分类 shell，再进入同一预留及回执监督路径；页面不提供可执行文件路径。真实固定 `/bin/bash` 的完整 PTY/EOF/退出码与缺失、暂不可用、删除后同名重新发现的拒绝无 fallback 一起通过（本模块总共5项定向测试），严格 clippy 与 Astrolabe 新旧相关源文件 Exact；Profile 入口后的 `mise run check/test/probe/bindings-check` 一次完整命令退出0：应用141项、存储213项（2忽略）、TS357项、受控 probe 和绑定漂移检查通过；这些检查不替代失败的真实 v2 窗口回执。
- 后台任务真正轮到执行且窗口已撤销时，`LocalLaunchTicket::require_current` 在原生 setup 前拒绝；current-thread 反例先证明同一固定不存在可执行文件在有效窗口会进入 native 错误分支，再证明窗口关闭后返回 registry Forbidden（即未进入该 native 分支）。本模块6项定向测试、严格 clippy 与 Astrolabe Exact 通过；新增检查后的 `mise run check/test/probe/bindings-check` 一次完整命令退出0：应用142项、存储213项（2忽略）、TS357项、受控探针与绑定检查通过。该短锁预检查不保证在其后到 spawn 之间无撤销，提交时依然必须二次验证并在失败时 join 后释放容量。
- `PageLaunchScope` 后端只读代际：native adapter 先验证 WindowToken、local-origin 与真实文档 nonce，才能创建该 Rust scope；换页或 `retire_window_views` 同一 registry 短锁下撤销旧代际在途票据，原生创建前及提交时再确认。旧 scope 析构不撤销新页，撤销不提前释放64名额，也不关闭已登记 actor。3项 page 测试（含真实 actor 生命周期）、7项 supervisor/PTY 定向测试、严格 lint 与 Astrolabe Exact 通过；最新 `mise run check/test/probe/bindings-check` 一次完整命令退出0：应用146项、存储213项（2忽略）、TS357项、受控探针与绑定漂移检查通过。`start_page_profile_local` 只在可信 Rust 内复用 Profile 目录和相同的 native join 路径；固定 `login-probe` 已在真实 Tauri 页中接通此入口，scope 本身并非页面身份凭证，仍未完成生产 shell 打开。
- 受控 `login-probe` peer admission 已调用真实 Tauri `caller`（本地来源 + WindowToken）与当前 `PageTracker` 文档 nonce 后，在 nonce 短锁内激活 registry 页代际；只读试预留立即丢弃；Started 导航先废止 nonce 再撤销 scope/票据。首次真实 login-WebView 原限额四轮完整通过（session/dom各2轮、7测量，四轮均有页 scope 绑定/撤销/重新绑定），但当时尚未从页面新建 PTY。随后固定 `login-probe` 将已确认 scope 交给 `start_page_profile_local`，后端只选择已发现的 Bash Profile 和固定短脚本，清除继承的 shell 初始化环境键，持 scope 锁仅到预留结束；页面不能提供路径/参数。native69项含真实 Bash PTY/EOF、旧页拒绝，default/login双mode严格 lint、Astrolabe 当前源 Exact 通过；新增 PTY 的真实 login-WebView 四轮回执已通过：每轮当前页与刷新页各完成一次受监督 Bash Profile 的 actor→native EOF（合计8次），每轮原页导航撤销一次（合计4次），固定页面scope绑定8次；`login-window.json` 中 session/dom各2轮、7项测量、两者 login 标志与 native 退出0。原40秒主 helper 均在期限内结束；未开放用户可选路径、命令或实际产品 TermView。随后 `mise run check/test/probe/bindings-check` 一次完整命令退出0：应用146项、存储213项（2忽略）、TS357项和基础探针/绑定检查均通过；旧40秒余量风险和独立审计阻塞未据此消除。此工厂不提供用户打开 shell 的生产页面命令。
- 票据、页面scope和监督者都不是页面/路径/可执行文件权限；生产 Tauri 页面选择与 Profile 身份联合授权、正式 TermView/Capability 以及 Windows/macOS 行为仍未接入。取消与回执丢失的担保限于活跃 Tokio runtime 内的直接子进程，不能宣称销毁 runtime 后依然监督阻塞系统调用或安全终止子进程树；不得据此开放任意 shell IPC。

- M1 本地 Profile 产品入口前置边界：可信 Rust `start_page_default_profile_local` 只接收 ShellKind 和后端组装的默认参数，复用代际票据与 PTY join；`start_page_stored_default_profile_local` 用 `StoredProfileLaunch` 显式归组后端发现候选、终端尺寸/历史与上下文，每次先从私有 SQLite worker 加载已校验偏好，重建目录，再由 `begin_page_local_launch` 复核当前代际；删除抑制、旧页面或存储关闭均不回退到默认 shell。2 项临时 0700 数据库→真实 PTY/EOF 定向测试及 1 项身份选择真实 PTY 测试通过；`mise run check/test/probe/bindings-check` 完整命令通过（应用149、存储213〔2忽略〕、TS357），严格 Rust lint 与修改文件 Astrolabe Exact 通过。`native_origin` 提取原有 Tauri 打包资源来源规则（测试非零，login 原生69项通过）；来源判断只是前提，不是页面授权。Astrolabe 引用/依赖索引 stale 不能作为通过；代码与编译/测试已交叉核对。默认 Tauri 命令仍只有受控探针，未登记产品本地会话命令/Capability/TermView，且本轮未重新取得真实窗口或三平台验收。SQLite 偏好读与后续原生创建不是跨进程原子事务，不承诺并发更改偏好瞬时撤销已启动/启动中的子进程；审计阻塞及原 40 秒窗口风险不变。

- M1 只读候选/产品文档前置边界：`local_launch::list_page_profile_offers` 每次从私有 SQLite 读取偏好并以可信发现候选重建目录，只返回 ShellKind 与隐藏状态（不返回路径、参数、环境），旧页/跨 registry 与关闭数据库均拒绝，读取不消耗启动票据；列表仅为快照，启动仍必须重新读取并获取当前票据。独立 `ProductDocuments` 不复用只允许两次刷新、绑定固定样例的验证 tracker：真实 WindowToken 在 registry 登记后仅 native Started→Finished（backend UUIDv4）→当前页 nonce 确认才生成 `PageLaunchScope`；导航先撤销旧 scope/在途票据/视图，不关闭 actor；异常重复事件、跨窗口、错误 nonce 确认和超额窗口 fail closed。4 项文档门定向测试含96次刷新、原 actor 存活与上限替换；严格 lint、Astrolabe 修改文件 Exact；其后 `mise run check/test/probe/bindings-check` 全部退出0（应用155项、存储213项〔2忽略〕、TS357项）。该模块**尚未连接实际 Tauri 生命周期、注入 nonce、产品 Capability/命令或 TermView**；UUIDv4 在 `ProductDocuments::finished` 内生成，不接受调用方传入；随机格式本身不证明当前 Tauri 页面来源，适配器仍须验证实际 WebView 并只向当前文档注入 nonce，不得新增 getter IPC。既有审计阻塞、40秒窗口风险及真实文件/跨平台验收未被此单元测试解除。

- M1 独立工作区预览：新增可选 `pulseterm-workspace` runner、独立 HTML/CSP 与 `workspace` Capability，命令表只有页面确认，没有验证应用命令或本地 shell/存储读写 IPC。后端 `ProductDocuments` 在 native Started 撤销上一代页面，在 Finished 同时检查事件 URL 与当前窗口 URL 是否为固定打包页面后注入 UUIDv4；命令再核对窗口标签、当前 URL 与一次性 nonce。原生适配器 2 项定向测试覆盖非本地来源、跨窗口、畸形/重复/旧 nonce、关闭后拒绝。Linux 隔离 Openbox 实测 `mise run probe-workspace-reload` 通过：同一 PID/XID 的 3 次真实 Started 与 3 次确认，页面按钮调用浏览器自身 reload，未新增测试后门；Vite 必须先于 Tauri 构建以嵌入新资源，此顺序已写入任务并接入 Linux CI。首次仅发 X11 Ctrl+R 的探针没有触发导航；后来检查截图发现原生二进制嵌入旧 HTML，修正构建顺序后任务通过。TS `pnpm check` 和 workspace 严格 Rust lint 通过；Astrolabe Rust Exact，TS 诊断 Unknown 不算通过。此轮仅接受页面生命周期与空预览，不包含 shell 启动、TermView 或跨平台。其后组合回归在新增第二二进制的默认选择处失败；manifest 加入 `default-run = "pulseterm-desktop"` 后 binding-check、workspace 全部66项原生测试与严格 lint 通过。后续实际私有数据库探针发现同目录的非标准配置文件名被锁定 Tauri codegen 忽略，独立配置/CSP 的旧描述不可作为已验收证据；修复及实证见下一条。审计及既有 40 秒窗口风险未解除。

- M1 工作区只读 Profile 列表：独立产品命令 `workspace_profiles` 仅接受页面 nonce，读取 OS app-data 私有 `state.db` 与后端 PATH/SHELL 发现结果，只返回5类固定 Shell 身份及隐藏状态；隐藏项不绘制，删除记录抑制再发现；SQLite/发现失败不回退默认 Shell。一次性 `confirm` 和后续 `current` 检查分开，未确认、旧 nonce、跨 owner、非本地来源及换页均拒绝；修复真实页面列表错误地二次消费确认的反例，重复确认仍拒绝。原生单飞 reader 的 supervisor 在 IPC waiter 丢弃后仍保留容量，阻塞 closure 独立持有票据直到退出；Destroyed/Exit 关闭 reader 并取消其 store，导航只撤销页面、不关闭共享库。Unix `state_directory::prepare` 逐组件拒绝符号链接、非目录、不可信 UID/可写祖先及非0700末级，不自动 chmod 现有路径；父级 `..` 在创建前拒绝、只读意图即使 root 也拒绝创建，根属主 sticky1777祖先单独允许。7项真实目录、4项 reader（真实偏好重读/删除、导航前置、失败关闭、取消保留容量）测试通过；不能据此声称完整 TOCTOU、ACL、掉电或 Windows ACL 验证。
  - 配置隔离实证：锁定 `tauri-codegen 2.7.1::get_config` 按传入路径的**父目录**读取标准 `tauri.conf.json`，同目录 `tauri.workspace.conf.json` 没有选择独立配置。已迁至 `apps/desktop/src-tauri/workspace/tauri.conf.json`，修正 frontendDist/icon 相对路径、显式只选 workspace Capability；启动先校验实际 Context 的 workspace identifier、空自动窗口、严格 CSP、无 devCsp/目录 override 与 bundle inactive，不接受验证应用或错误 TAURI_CONFIG merge。配置、当前页与 reader 共8项定向原生测试和严格 lint 通过，Rust Astrolabe Exact；引用索引 stale、TS Astrolabe Unknown 不算通过，未安装新语言服务器。
  - `mise run probe-workspace-reload` 经隔离 Xvfb/Openbox 与临时 HOME/XDG 执行，3次导航＋3次页面确认＋3次只读列表回执、同PID/title/XID以及**准确工作区 identifier 目录的 state.db 存在**全部通过。第二、第三轮实际截图显示 Bash/POSIX sh；第一轮截图仍是验证中，不能将这次探针说成3轮 DOM 绘制证据或 compositor 回执。首次新探针的双回执比较错误、随后二次确认拒绝及旧配置导致 database_missing 均保留并修复；原8秒页面/80秒总截止未延长。脚本隔离目录最终清理，未访问真实用户配置；新增图片与日志进入Linux CI归档，CI注册不是远端已执行证明。前端20项定向测试覆盖固定字段/数量、重复/隐藏、稀疏及继承字段拒绝、坏nonce、换页立即取消、迟到丢弃、5秒无重试截止与空库/错误区分。其后完整 `check/test/probe/bindings-check` 加 workspace 原生全套/strict lint 全部退出0：存储220项〔2忽略〕、终端112项、TS377项、workspace原生72项；既有审计6+2、GPG暂存边界、40秒终端风险均未解除。列表仍是瞬间快照，**没有生产 shell 启动、TermView 或路径/环境/存储写IPC**，Windows尚未开放系统库读取。
- M1 生产本地会话根所有权原语：`pulse_app::local_session_owner::LocalSessionOwner` 接受**已授权并登记**的会话，不自行授权／启动。根状态持有而非页面持有，导航/关窗仅撤页；自然 `finish` 保留最终 actor/native 回执并退登记，取消等待或 Drop 则关句柄、后台继续等待真正 native 完成，观察者不延长 owner 生命周期。成功 join 回执之前不调用 `reap_closed`；关闭通知通道、原生报错、未知完成均标为 Failed 并保留登记容量，不能把 actor EOF 或定时器当 native 已回收证明。运行时必须活到清理结束，尚不承诺 runtime teardown、后代进程树或 OS 卡死恢复。7项真实 Unix PTY 回归覆盖 exit7、页/窗口撤销存活、finish取消、清理 waiter 丢弃、延迟完成、丢回执、finish/cancel 两种 native-error；应用162项、workspace原生72项、严格lint、2MiB session probe及 diff 检查通过，三个 Rust 文件 Astrolabe Exact 无诊断。初次 fixture 错将会删除登记的 `close_session` 用于存在检查，已改用 test-only 非破坏计数；PTY hangup 可先于 native kill reap，故 close请求不等同于 native.cancelled，不改原生退出语义。仍是可信 Rust 原语，**没有产品页面启动命令或 TermView**，错误回执的容量只可在另有真实 join 证明或应用 shutdown 后处理；审计与暂存边界不变。
- M1 当前页面终端绑定原语：新增 `SessionRegistry::attach_page` / `attach_page_v2`，在捕获前、分配代际及捕获后安装绑定的同一元数据锁内核对 registry/window/page；绑定记录仅后端可见的页面序号。`check_page_binding`、`acknowledge_page`、`input_page_bound` 不允许当前页面借用 window-only 租约。Scope Drop/旋转撤销该页绑定并保持会话存活；旧输入即使已入 actor 请求队列，仍由 lease 取消检查拒绝，已入 driver 的输入不承诺撤回或安全重试。8项新增测试各覆盖v1/v2（旧/跨库/错窗不消费代际，捕获中刷新/关窗，旧监督者不删除新绑定，scope销毁，排队输入取消，新页UTF-8输入，legacy租约拒借，捕获future取消和迟到scope Drop），连同既有4项page-launch测试共12项通过；输入反例检查的是 ChannelTransport driver 队列，**不是新增真实OS写入或WebView验收**。严格lint和4个改动Rust文件Astrolabe Exact无诊断；引用索引仍标stale，调用路径按当前源码交叉核对。完整影响层已通过：应用170项、workspace72项、login69项，以及 check/probe/bindings-check，日志 `target/probes/page-bound-attachment-full.log`；现有window-only API兼容、v1/v2二进制格式与授权nonce不变。仍无生产页面启动命令、TermView交付或跨平台验收，审计与暂存边界不变。
- M1 根会话来源与集合后续：`RegisteredLocalSession` 的来源注册表只由提交监督者设定，身份/句柄/完成回执字段改为 crate-private、对外提供只读身份/句柄方法。`LocalSessionOwner::adopt` 直接从登记回执取得来源，不再允许另传注册表；双实例同号 SessionId 的真实 PTY 回归证实只能回收自己的来源。固定 login 页改用同源 owner 的 `finish`，不能在 native 结果错误前先回收登记。`LocalSessionRoot` 是可信 Rust 根集合（不是页面权限）：上限64个已持有/待消费EOF/关闭中/失败条目，页面导航不销毁它；取走 owner 后继续保留观察占位，只有同一 owner 的 Reaped 能释放。失败或失联 watch 保留容量，拒绝外来/已关闭/满集合的 owner 仍监督实际子进程关闭/join。8项 owner 与8项 root 定向回归、179项应用、workspace72项、login69项、check/probe/bindings-check、严格lint和新增/改动Rust文件Astrolabe Exact诊断通过；64槽满容量部分是合成 metadata/closed-watch 表，拒绝新增后的清理使用实际 PTY，不称为64进程性能/容量验收。固定 login 实际WebView四轮完整回执通过（`target/probes/root-owner-login-real.log` / `login-window.json`）：2个headless＋2个DOM轮次、8次页面子进程EOF、7个verified测量、两组nativeExit=0和profileLoginChecked=true。DOM两轮主helper约36.3/34.7秒结束，原40秒期限未改，既有边缘超时风险未称为修复。此为固定样例同源清理的WebView兼容，不是新产品TermView或应用退出排空验收。未重复不变的存储/TS或原审计失败集；生产页面启动、TermView、应用退出时运行时保活/排空仍待接入，签名暂存14文件边界与发布门禁不变。

- M1 新读存储偏好→根启动连续操作：`local_session_root::start_stored_page_local` 在新读 SQLite 前检查当前页面、根来源/关闭/容量，随后复用当前页面启动票据并无 await 地把登记交给同源 owner/root；预检查不是容量预留，迟到的满/关闭根仍按实际 native cleanup 保留底层容量，返回错误不表示 join 已完成。5项应用反例涵盖隐身 Profile 显式选择、页面退休后的真实 PTY EOF 消费、删除/失败存储不回退、异源/已关闭根优先拒绝、读取中换页/取消不登记或接管。原生 Profile reader 拆分 backend-only 准备，并绑定首次打开的目录，另一目录不能静默复用旧库；2项原生回归走实际临时私有目录/SQLite/发现值/固定无用户rc的PTY/root完成链，复用准备值后新增删除仍拒绝。184项应用、74项workspace原生、check/严格lint/probe/bindings-check通过，真实workspace3轮各有navigation_started/document_confirmed/profiles_listed（`target/probes/root-stored-profile-regression.log`、`workspace-reload-native.log`）。这是后端操作和现有列表兼容，不是页面启动/交互Shell/TermView或退出排空验收。本轮Astrolabe引用索引标过期、4文件诊断请求60秒超时，未计通过；不安装/重启LS。未重复不变的存储/TS及既有审计失败集，发布与14文件签名暂存边界不变。

- M1 注册原生任务排空基础：`native_lifecycle` 在实际 enqueue 前预留最多64项，未启动取消可退出；已开始任务仅同源 native join 成功可退出计数，丢监督、部分 setup 错误或失败回执保留 unknown/失败状态，不能以 actor Closed、Tokio监督 task正常返回或外层future取消当成功。`SessionRegistry::seal_local_workers` 拒新入队、观察已排队/运行工作；范围仅该注册层启动，**不覆盖 raw spawn_local、设置worker或操作系统进程树**。`LocalSessionRoot::shutdown` 先seal再关闭已有和借出finish的句柄，等待原生worker及同源Reaped，等待取消不abort工作，不持根锁跨await，错误/超时不能授权退出运行时。修复取消监督吞 native join 错误、未接收页回执阻塞关机、确认与seal同时就绪时误丢已经交给root的完成回执；确认优先保留交接，未接收启动由seal独立唤醒关闭。4项ledger、5项root排空、1项真实未接收PTY回收及1项模拟错误传播反例通过；包含真实PTY已持有/借出finish/等待取消及确定性queued-before-poll，故障metadata/失联模拟未冒称真实OS故障。194项应用、74项workspace、69项login/check/probe/binding通过，随后新增1项模拟反例及严格lint通过（`target/probes/local-native-drain-regression.log`）。六个改动文件Astrolabe诊断Exact无匹配；引用索引仍标过期未用其证明调用完整。实际固定login WebView四轮兼容通过（`target/probes/native-drain-login-real.log`/`login-window.json`）：2个headless＋2个DOM、8次页面子PTY EOF、7项verified，session/dom的nativeExit=0、profileLoginChecked=true；DOM主helper约33.4/33.3秒，原40秒期限未变，旧窄余量风险未宣称修复。之后仅把未接收分支的同源reap移到ledger成功清零前，相关1＋5反例/lint通过；此错误分支不是四轮既接收正常页的实测覆盖。此基础验收时尚未挂生产workspace根会话/ExitRequested及Profile物理排空；后续正常退出链见下一项，不称页面启动、TermView或跨平台验收；原审计及14文件暂存边界不变。

- M1 生产工作区正常退出挡板：`product_profiles_shutdown.rs` 将读信用绑到实际阻塞闭包及读监督退出；关闭Semaphore后以其真实permit计数和watch事件等排空，不用通知旧值认定空闲。共享退出监督任务持有唯一`StateStoreTask`并等待SQLite join；取消等待不abort工作，重复等待共享成功/失败，锁损坏或监督失联不能晋升成功。5项新增Profile反例含取消实际阻塞闭包、排队前关闭、共享真实库join、不再重开、关闭后缓存发布模拟及毒化所有权（测试自行清理），不把模拟故障称OS故障。`product_workspace_exit.rs` 的ExitRequested先veto并撤销页范围，独立root关闭后同时等原生和Profile物理退出，只有全部成功才重新请求Tauri退出；失败保持Runtime，重复退出只启动一个监督，未确认成功不得退出。新增2项纯gate反例和1项真实活PTY＋SQLite联合排空通过；82项workspace/check/binding/严格lint通过（`target/probes/workspace-exit-regression.log`）。Linux独立Openbox实际3轮刷新后由PID＋固定标题核对的同一XID接收Alt+F4（不是XDestroy/SIGTERM），8秒关闭观察界限内得到唯一有序`shutdown_started→native_drained/profiles_drained→shutdown_complete→exited`回执，保留原80秒全程限；实测工作区只有只读Profile，活PTY联合证据为后台测试，不冒称GUI活Shell退出。7个相关Rust文件Astrolabe诊断Exact；初始符号引用索引过期未用于证明，原14文件暂存及发布门禁不变。该链不覆盖强杀、Tauri restart、启动失败、进程树、跨平台ACL或生产TermView。

- M1 当前页面终端视图绑定基础：新增`terminal_view.rs`，v2 attach在同一actor边界捕获并订阅，页面范围`captured_query_palette_page`与`detach_page`在同一注册锁内核对当前页、实际来源和generation；此阶段transport/palette是非授权数据；后续投递层将整个capture封装为opaque，外部调用方不能替换binding或wire，见下段。Drop只撤销对应租约，不关闭根会话或新租约。6项合成Driver反例验证同页替换、换页旧ACK/input/palette拒绝、legacy v1/v2隔离、跨窗口/注册表、取消捕获及新基底palette；1项真实私有Profile DB→根PTY→v2绑定验证输入回显、主动detach后同一actor重绑、导航后新snapshot边界不退、旧输入未发送、Data→End有序与直接子进程退出7的同源回收。7项测试/严格lint和4个相关Rust文件Astrolabe诊断Exact通过。受影响check/202项应用/82项workspace/69项login/probe已通过；原组合命令240秒在最终绑定二进制启动后超时，没有完成回执且相关进程已不在，保留失败日志（`target/probes/terminal-view-binding-regression.log`）。不重跑已过检查，单独原`mise run bindings-check`在60秒观察限内正常退出0（`target/probes/terminal-view-bindings-final.log`），补齐最后门禁；未称原组合命令通过或绑定挂起原因已修复。测试中ACK只代表后端消费，不是WebView绘制/CSP/IME证据；此服务未挂生产IPC，未开放页面Shell启动。生产Surface将复用xterm public documentOverride/nonceDocument及base palette构造，不使用OSC伪装主题基底或放宽CSP。原暂存及发布门禁不变。
- M1 TermView前端Surface基础：`terminal-surface.ts` 按需加载xterm，使用其public documentOverride、nonceDocument和SharedUnicodeProvider；解析并在构造时应用显式19色基底，远端查询只由后端响应且OSC52抑制，不给远端HTML/URL/剪贴板接口。尺寸遵守core 1..512列/1..256行，Surface仅接受0..10000历史行；backend Begin的ViewScope拥有资源，晚到loader不创建新视图，打开失败回收parser和terminal，手动/代际销毁都结算解析等待，停用输入监听后尾回调无效。单飞write复制最多64KiB，回调只证明解析，不能释放绘制ACK。7项DOM stub生命周期反例、384项TS全套、类型/格式检查和Vite build通过；stub不是真实CSP/DOM绘制、系统IME或键盘验收。两份TS文件Astrolabe诊断Unknown（typescript-language-server不可用）未计为通过，也未擅自安装。Vite保留原有probe动态/静态混用和大chunk警告；Surface尚未挂工作区，未对未使用模块的build声称渲染验收或优化。产品HTML运行时nonce、Raw Channel→consumer→真实绘制ACK、页面Profile启动和输入IPC仍待接入；原14文件暂存及发布门禁保持。
- M1 前端v2消费与投递后续：`terminal-view-stream.ts` 接收Raw ArrayBuffer到唯一有界版本严格队列；只有解析完成、两次rAF及自有DOM行/矩形校验后才ACK，rAF不叫compositor完成。Commit先ACK，再等待捕获图像确认，暂停消费已排队Marker/Data，摘要不与latest freshness混用；3秒bootstrap总截止、1秒绘制截止保持，Ready后撤bootstrap timer，不因长运行自动驱逐。UTF8改由public Utf8Stream按序解码成字符串，incomplete bytes不留xterm私有decoder；CAN screen-only修复只丢decoder片段、不清history，真正End才finish。共享`visible-digest.ts`提取原探针同一投影；10项流反例、10项Surface、8项旧v2、4项真实headless投影共32项定向通过，headless/stub不冒称真实颜色/CSP/IME。401项TS、类型/格式、build/绑定及真实workspace三次刷新/WM有序退出通过（`target/probes/terminal-view-stream-regression.log`）。产品HTML已置受Tauri替换的nonce锚点，但新Surface仍未挂页面，既有空工作区验收不叫新终端绘制验收；4个TS诊断Unknown未计通过，没安装LS，旧build警告保留。
- M1 后端有界投递后续：`TerminalViewDelivery` 消费opaque capture，外部不可错配wire/binding；只读palette需当前源验证。单拥有relay每帧验证页范围、v2布局/generation，不新增队列、JSON化、坐标推断、send重试或自动ACK。Raw Channel send admission不是绘制回执；发送与导航的跨进程竞态仍需客户端丢弃退休scope，不能宣称发送与导航原子。取消未轮询/活relay、丢delivery但保留control、sender失败、预排队Begin页退休、旧Drop与替换、内部模拟错元数据及1帧无ACK超时的7反例通过，actor/root仍存活；加原7项绑定真实PTY共14项及strict lint通过，3相关Rust文件Astrolabe Exact。该relay尚未接Tauri commands，14项不叫真实Raw Channel验收。209项应用/82项workspace/69项login及check/probe/binding影响面通过（`target/probes/terminal-delivery-regression.log`）；该任务不包含新Raw Channel真实窗口验收，不能据此标生产TermView完成。真实nonce+Channel endpoint、captured/Marker确认、页面受限Profile启动与交互入口仍待接入；原14文件暂存及发布门禁保持。

- M1 独立工作区原生终端连接：`product_terminal.rs` 注册 prepare/connect/ack/confirm/detach 五个真实Tauri命令，只选择根持有且当前窗口授权的既存会话，不能启动Shell、传路径/参数/环境、写入用户输入或关闭根会话。prepare先以同capture生成19色JSON与UUIDv4一次性ticket，connect才发送v2 Raw Channel。连接3秒绝对截止同时核单调时钟与timeout_at，避免迟到timer放行；64个relay配额包括等待/退休，真实任务退出才释放，应用ExitGate还须等待relay排空。导航同步撤页面范围和bank记录，旧页/错ticket/重复connect/退租约后的ACK均拒绝，view drop不关闭actor。ACK返回是队列admission，不是协议成功或绘制认证；错序由attachment监督回收。confirm以后台capture完整state比精确十进制双坐标/尺寸及digest，`compare_bootstrap_page`在同metadata锁校验原页/视图后共享原500ms配额，不能将legacy绑定升级，也不清risk或称latest Equal。9项native Channel/DTO反例、1项两协议page compare反例与旧5项bootstrap通过；210项app、91项workspace、69项login、check/probe/strict lint/binding以及真实空workspace三次刷新和WM有序退出通过（`target/probes/workspace-terminal-native-regression.log`）。8个Rust诊断Exact；Astrolabe调用图仍stale，交叉核对源码，不计引用图通过。
- M1 前端原生连接适配：`workspace-terminal-connection.ts` prepare metadata严格限四字段，generation/session不能经过JS Number，palette符合既有bounded合同。只在校验过Raw Begin之后创建ViewScope，先Palette后connect，Commit绘制ACK后调用native capture确认；其他Marker/End观察只是未确认drawing data。取消/迟到metadata/connect或confirm失败/错代际首帧/host退休都丢scope并一次detach，不重试输入或关闭session；等不到Begin也由3秒单调截止终结ready。12项反例、完整413项TS、type/format/build通过（`target/probes/workspace-terminal-client-{tests,build}.log`）；两TS Astrolabe诊断Unknown未当通过，未经授权没安装LS，原大chunk警告未放宽。连接适配器尚未挂生产页面；空workspace与内存native Channel不是新Surface真实WebView/颜色/CSP/系统IME验收。受限Profile创建入口、生产TermView挂载、输入、Marker/latest确认仍待接，原14文件暂存边界及发布门禁保持。

- M1 受限本地只读工作区挂载：`product_session.rs` 注册当前nonce/WindowToken范围的 Profile open、根会话 list、显式 close。选择DTO仅kind并拒未知字段（程序/参数/env/cwd/mode），只接受五个discovery身份；`ProfileReader.start` 监督真实I/O信用、再次读SQLite当前偏好，经原页启动二次检查交给同源根集合。取消或丢IPC回执不重试进程；已采用会话可由新页list/rebind。关闭在registry锁内验证当前页、关闭actor但保留registration，之后Root owner join/reap才回收；已准入关闭不能由随后导航撤回。旧页/foreign close、fresh deletion与retired startup、literal kind/path/args DTO反例通过。独立workspace挂载lazy Surface、nonce动态style、共享Unicode/查询拦截/v2 credit和capture确认；固定80x24/10k history，`disableStdin=true`且未注册任何input命令。Profile点击启动、选择既存session恢复、标准CtrlR导航、明确关闭会话都已接入，失败不回退或自动重试。UI明示只读，不能称交互终端完成。417项TS、type/format/build、211项app/94项workspace/69项login、check/probe/binding及strict lint通过（`workspace-readonly-{ui-tests,core}.log`）；5个主要新改Rust诊断Exact，后续session日志更改当前诊断也Exact。3个TS诊断仍Unknown没安装LS；调用图stale/同名异语言误匹配不作权限依据。
- 实际Linux/Openbox产品页链路已验：固定private HOME/XDG/PATH中fixture sh，经真实Profile按钮键盘选择→PTY→Raw→DOM解析/1秒绘制门禁→ACK→backend capture确认，三次页面代际都取得匹配摘要，同一native PID存活且只open一次，窗口退出root/native/SQLite排空后PID不再存活（`workspace-terminal-window-selection.log`、`workspace-terminal-live-exit-pass.log`及round-1/3截图）。固定白字/红绿字与页面只读状态实见，CSP没加unsafe-inline；这不是OSC reset/query全矩阵或系统IME证据。额外实际Close Session点击成功：3capture后收session_closed、native PID被回收，再WM关闭仍严格ordered shutdown（`workspace-terminal-close-run.log`/`workspace-terminal-explicit-close-pass.log`）。空工作区CtrlR三次、不启动任何Profile也通过（`workspace-empty-reload-current.log`）；`mise run probe-workspace-terminal`注册两种有界mode，Linux CI纳入。两次早期native_pid_missing的原因是探针错以为PATH只会发现fixture；实际discovery还补system directories，首按钮Bash，故未计固定sh通过，生产发现规则没有改。之后按系统前序metadata走Tab，且最终断言profile_selected.sh并检查专属绝对PID回执才通过；不是HOME覆写缺陷。原80秒整体窗、8秒阶段、3秒连接、1秒绘制期限未放宽。只读挂载和固定fixture验收不代表User Shellrc兼容、连续网络/长运行、resize/隐藏策略、input/粘贴/system IME、跨平台、S2 parser全精确或M1 Alpha完成；此前40秒与visibility/rAF风险也没有宣称修复。原14文件暂存及发布审计门禁保持，未新增Tauri插件权限或依赖。

- M1 工作区输入接入（Linux受控键盘链与默认v1/v2兼容通过）：Attachment Driver仅在真实信用窗口验证包含Commit的累计ACK后发布可撤销bootstrap回执，driver退出/异常置false；ACK队列准入或Commit发出均不开放输入。页面范围wait在等待前后核原generation，v1/v2确定性反例验证queued ACK仍pending、非法ACK/换页/撤销无ready。新增当前nonce/票据enable_input＋input：必须已连接、准确capture确认和监督ACK证明，每次写复核lease；单并发/1KiB UTF8，取消IPC waiter后台工作仍持有Entry和信用至actor写准入操作退出（不是物理write ACK）。前端在Commit消费完成及native enable后开启stdin，现有InputQueue维持1秒不确定超时、总队列界限和不重发；退出/代际更换停旧onData，粘贴capture阻断；CtrlShiftR刷新避免吃Shell的CtrlR。固定read/printf fixture不会执行输入文本。421项TS、213项app/96项workspace/69项login、check/probe/binding与strict检查通过，6个Rust诊断Exact；随后first-error取消传播新增反例及26项定向通过，不能替代完整后续回归。
- 输入真实窗口反例保留：最初三页keyboard已到同一PTY并收到ready/capture，但round-3截图终端被清空、UI显示连接失败，故退出0撤回为未验收（`workspace-input-window-run.log`）。随后probe明确拒绝任何failure receipt；`workspace-input-window-diagnostic.log` keyboard_timeout/view.retired，并发现scope释放遮盖原始绘制错误；已用取消重入guard＋finally释放保留第一原因。`workspace-input-first-cause.log`仍failed_surface/unknown，白名单只收静态阶段码、绝不输出TTY/nonce/路径；随后静态异常码精确定位`attach2.queue_limit`（`workspace-input-exact-cause.log`）：前端直到ACK IPC回复才释放已消费wire占位，backend接受ACK后立刻投递替换帧造成合法8帧信用流的瞬时超载。原失败保留，不以第一次exit0回退验收；实际IME、多行粘贴、resize和跨平台仍未验收；原1秒绘制/1秒输入/3秒连接/8秒阶段/80秒整体与14文件暂存及发布门禁未放宽。

- ACK重入修复与Linux键盘验收：v1/v2 consumer在解析/绘制完成后、调用ACK之前丢弃active的wire引用并幂等释放本地计数；ACK等待和Commit captured确认仍可取消、阻止后续Data消费，不新增信用或第9帧队列。1帧回调即时enqueue replacement的两协议反例先红（queue_limit）后绿；旧active计数反例改为阻塞真实render而非消费完的ACK，保留超载拒绝；另验ACK在途cancel不双减、Paste事件阻断与监听清理。426项TS全套/type/format通过，213项app/96项workspace/69项login、check/probe/bindings和strict已过；最终96项native/lint/bindings再次通过。7个相关Rust文件诊断Exact（含后续report adapter），TS consumer/connection/stream诊断仍Unknown、未安装LS，调用图stale不能作权限依据。
- 实际产品窗口通过（`workspace-keyboard-fixed-window.log`/`workspace-keyboard-final-native.log`）：live窗口退出和explicit session close两mode各3次真X11 ASCII键盘→xterm onData→1秒有界队列→当前页/票据/bootstrap监督→同一PTY；每页固定line仅一次、capture/input-ready receipt匹配、无failure、samePID且open1，finally ordered native/profile/SQLite shutdown。空工作区3刷新也兼容。分mode原生回执/截图保存，避免空窗口覆盖。第三张live图可能仅显示回显前缀（Pipeline尚未画完），不误称完整屏；额外第4页准确captured恢复已验（`workspace-input-final-capture.log`），`workspace-terminal-close-final.png`实见全部3条keyboard＋echo和红绿文本，随后明确关闭回收、WM关闭。此为固定read/printf fixture实际ASCII键盘和恢复，不是User shellrc、系统IME/硬件keyboard layout、paste、自动resize、长期压力、跨平台或M1 Alpha全量验收。共享consumer改动的默认v1/v2真实WebView兼容通过（`queue-release-default-webview.log`）：两协议各2个headless＋2个DOM，7项verified/两组nativeExit=0，nativeSettingsChecked/nativeThemeChecked均false保持factory隔离；四轮DOM主helper约32.9/33.0/33.0/33.9秒，40秒期限未变，窄余量风险未称修复。默认app未继承product shell/input命令。既有40秒/visibility/rAF风险与原14文件stage和release门禁不变。

- M1 页面范围resize准备（此准备段后已挂载，见下一验收段）：`SessionRegistry::resize_page_bound`及`TerminalViewBinding::resize`在metadata锁内核原页/lease准入、释放锁等native ACK，提交后核同一个page与owner、不要求已失效旧lease仍有效。两组确定性v1/v2测试证明native ACK前size不变、ACK后新捕获尺寸正确/旧输入拒绝；导航发生在native准入后不能升级scope，也不伪称撤回已发生resize。实际private Profile→Root portable PTY已验证48×10 native确认→新gen恢复→继续输入→exit7同源reap。NativeBank监督resize与input共1并发，先阻旧输入；测试调用者Drop仍持Entry/信用直到nativeACK，非法size/重复请求拒绝，release实际后再drop全部credit guard。该适配器暂为cfg(test)（无dead_code豁免，无生产resize IPC），待视图退休/重建/旧结果隔离前不注册。
- resize字体测量准备：Surface使用公开`.xterm-screen`实际rect/cols/rows和host client bounds复用`measuredGrid`，不读xterm私有_core、不加FitAddon；hidden/zero/失效scope不发明2×2尺寸，proposal不是native确认。新测量反例与原FitScheduler共22定向、全427 TS/type/format通过；6个Rust诊断Exact，TS Unknown未装LS，Astrolabe resize符号未命中stale不作依据。FFF窄路径pattern多次泛化，已用当前源码与实际目录`fit-scheduler.ts`复核。原输入成功工作区布局/渲染保持未变，ResizeObserver/输入暂停/原生回执与重建串行化、实际窗口连续resize/DPI等未完成。组合`page-resize-preparation-core.log`仍后台验证215app/97workspace/check/probe/bindings/lint，该准备段组合215app/97workspace/check/probe/bindings/lint已全部通过；原14文件暂存与release gate不动。

- M1 生产工作区自动适配首次接入：`workspace_terminal_resize`只接原页nonce/connection与2..160×2..60数字网格，已注册独立workspace handler。NativeBank与input共1信用，原生已准入任务取消不能提前退配额；先阻旧输入，nativeACK后原page后检查，旧generation无权写，后端原生size在新Begin供显示，前端不optimistic调用terminal.resize。`WorkspaceFit`每Begin用公开screen/host+viewport scrollbar测量；ResizeObserver立即暂停input/consume但保native租约和DOM测量，50ms合最新size，成功才重捕获；6s不确定失败不自动retry，late/retired callback不能升级权限。回到原网格但队列已cancel时也重捕获，不复活旧队列；事务busy与fitBusy分离，首个Profile finally不解禁在途fit。Viewport布局给实际余高，滚动条不遮末列；hidden/zero不伪造尺寸。33相关/新5fit协调反例、全434 TS/type/format通过。
- 首次真实autoFit曾在native确认92×31后重连失败：确认使用跨绑定保留的500ms comparison额度，立即确认新capture撞限流。新增`compare_bootstrap_page_when_ready`持单比较准入guard，准确等上一确认的nextEligible，只尝试一次；不持metadata/terminal锁跨await，不改500ms规则/3sbootstrap，不按固定sleep试运气。睡醒原页+gen必须仍有效，退休不占新页额度；两个确定性反例证明原sync仍Busy、间隔≥500ms且旧候选不能发布；Native重复确认立即拒。该修改后实际初fit/四页capture/唯一三输入/显式close通过。
- 实际Openbox WM三种尺寸验证通过（`workspace-resize-matched.log`、`workspace-terminal-resize-pty-size.txt`、分mode截图/native.log）：960×640→840×560→1000×720，sh `stty size`为rows/cols 31/92→26/77→36/97，每步严格等于actor成功回执，三条唯一键盘输入、初spawn恰一次、同PID跨页仍活，新代际确认后才恢复输入；最后第四页捕获和pixel中三组完整回显/红绿颜色均正确，WMclose排空完整。217app/97workspace/69login与check/strictlint/probe/bindings已过；7个相关Rust诊断Exact，TS Unknown不算pass，未安装LS。mise `probe-workspace-terminal`加入第三resize mode，既有Linux CI入口已同步；最新closeguard构建的三个终端模式全部通过(`workspace-autoresize-final-registered.log`)；第四个empty模式因归档cp自身失败，页面3reload已过但当次整体任务不算通过。修正同路径archive判断后仅重验受影响empty模式，完整3reload+ordered shutdown通过(`workspace-autofit-empty-archive-fixed.log`)，不重复未变已过三个模式，不用单exit0替代回执。mise/既有LinuxCI的实际入口和Shell语法已核验。仅控制的Linux WM/布局适配，不是高负载连续拖拽、DPI跨屏、真实IME或Win/macOS验收，paste仍blocked，release/原14staged边界不变。

- M1 resize风暴／确认退休进展（最终actual重验pending）：`terminal-storm`两批各9次无sleep外部WM动作到固定目标已通过旧构建，samePID/3uniqueinput/kernel==actor/四页恢复/有序exit都通过，有限事件批不是连续拖拽或负载性能验收。审查发现退休的capture confirmation clockwait继续持comparison准入直到500ms：新增内部Attachment.cancel receiver，仅退休clockwait可立即结束并退该纯计时guard；不提前退实际native IO信用，不改500ms quota。确定性首次poll退休、原页不消费新页额度及另native取消实际ACK前仍持信用反例通过；4相关Rust Astrolabe Exact（符号索引stale不作依据）、217app/97workspace/69login/check/probe/bindings/full436TS通过。
- `terminal-refresh-resize`要求在PTY已ACK新尺寸、Commit确认尚未结束时按CtrlShiftR，exact两个旧确认retired回执后新页恢复同一PTY，而非把keypress当退休证据。首次实际运行在`capture_pending_missing`前出现`terminal.dom_geometry`：live draw先于ResizeObserver通知，窗口缩小时旧网格被几何gate拒绝。新增current-generation beforeDraw hook在prepare及2rAF后verify前主动测Fit并abort旧wire，不放宽DOMbounds/projection/2rAF/ACK；保scope/native lease到实际resize，再重新capture，hook退休必须解绑。37相关定向及新438TS/type/format已过；最后actual刷新＋风暴正运行（`workspace-geometry-refresh-storm.log`），尚未更新CI/宣称该新反例通过。先前4mode接受仍按先前记录，不用它代替此次新draw变化真实回执；原14staged和release gates不动。

- M1 在途capture退休定点验收：进一步失败表明`confirm_pending`只是进入函数，可能已完成；新增observed comparison hook只在真实nextEligible未来时报告`confirm_waiting`（仍单500ms quota，不是可确认/清risk许可）。前两页在input-ready后立即resize并按实际waiting回执reload，第三页才输入3unique行，最终第四页截图完整；不再宣称此模式每页输入，也不要求3行physical尺寸互不相同（最终页native97×36三次相同）。实际`workspace-reload-intent-native.log`取得exact2retired、一次spawn/samePID/新页恢复/最后三输入kernel==actor/完整DOM文本红绿像素/有序shutdown通过。还暴露WebKit beforeunload之前旧IPC失败可报错：主动按钮/CtrlShiftR先abort本地life→dispose→navigate，晚结果不记新页面失败；纯helper验证order/idempotent/cleanup不确定不navigate/不抢CtrlR或alt/meta，全441TS/type/format通过。4相关Rust诊断Exact，TS Unknown未装LS；最终217app/97workspace/69login/check/strictlint/probe/bindings再次通过(`workspace-resize-lifecycle-final-core.log`)。
- 真实scheduler-paced storm仍有启动阶段几何限制：首次注册组合中，独立xdotool进程跨50ms合并间隔，native已提交85×30但新surface还未Commit时容器又缩小，引发`terminal.dom_geometry`。已就绪的live draw主动hook不能代表未就绪bootstrap也已解决，此失败不掩盖、不放宽geometry/ACK/confirmation门禁。有限event-batch探针维持两批各9次同形窗口动作，改一个控制进程批量执行以精确验证合并；与慢速/调度间断连续拖拽是不同场景。新mise/Linux CI `probe-workspace-resize-lifecycle`入口已注册，但最新single-controller storm+在途刷新组合仍pending(`workspace-lifecycle-single-controller.log`)，不得当整体通过或CI通过；原registered多进程组合失败保留。新Surface bootstrap中尺寸再改变的安全过渡策略、跨屏DPI/systemIME/跨平台/释放门禁仍open，原14staged边界不变。

- 本段最终阻塞确认：single-controller batch仍失败(`workspace-lifecycle-single-controller.log`)；不是CLI进程开销修正成功，18动作未减少且仍保留manual probe。当前Surface.verifyDraw测的是`.xterm-screen`完整网格而非外容器，CSS host本来已overflow hidden，不能简单限定外盒伪装完整绘制。当in-flight bootstrap的原生85×30大于新的容器时，既有drawing gate拒绝；TerminalBank.resize又要求capture.confirmed。两门禁形成启动布局闭环。未缩字体、未crop冒充完整绘制、未删除resize confirmation检查。该问题须采用明确布局控制授权/过渡策略后再验收慢速或间断resize，真实storm未验收。新增mandatory CI step已撤回（基线原有probe-workspace-terminal CI仍保留），不以continue-on-error/skip将已知失败算pass；manual新mise入口继续用于保留反例。拟议（尚未实现／未批准）：将bounded layout-only尺寸控制与输入确认授权区分，前者仍须当前WindowToken＋page nonce＋后端绑定generation＋会话归属＋single-flight native实际IO信用，不能输入或清risk；输入继续完整validatedCommit+capture confirmation。涉及既有resize门禁调整，先取得设计决策，不以移除检查求pass。

- ADR-031 已由用户批准并完成本轮实现：布局 resize 不再等 capture.confirmed/Commit-ready，但仍要求原始真实 WindowToken/page nonce、后端票据/绑定代际/会话归属、已连接 Channel、2..160×2..60 和一代际单次原生调整。输入仍要求已确认捕获和监督器验证 Commit；Native 短准入锁串行 input credit/enable 与 resize_started，不跨 await。resize_started 永久挡迟到启用或待验证输入，已准入输入继续原生串行且不可回撤。取消调用者不退实际 I/O 信用。新增2原生反例覆盖未Commit布局成功、所有输入拒绝、旧页/旧代际拒绝、调用取消保持信用和原生ACK后才释放；已有输入/投递/退出回归保留。
- frontend新增 layout-only pause；公开 `TerminalLayoutChanged` 仅原生明确成功或从未派发的尺寸目标回转才能请求Root循环重捕获。原生失败/6s不确定不返回此结果、不重试；未派发沿原3s期限。50ms合并器与Observer由原代际持有，先停止消费和Data/Commit绘制ACK（已解析header/meta0/1不回滚），字体13/Unicodeprovider/CSP/DOM全网格bounds与text projection/2rAF预算均未改变。尚未成熟的ready回调在布局中不能迟到安装输入。两新前端反例和全443TS/type/format通过，14终端Native定向通过，2Native文件Astrolabe Exact；TS依然Unknown未擅自安装。
- 实际layout-only修复原反例通过：`workspace-layout-only-native.log`的有限18动作批＋两真实capture-wait退休回执完整通过；随后`workspace-original-paced-native.log`还复验原来两批各9个独立xdotool进程的调度间断动作，不是只通过快批。samePID/一次spawn/逐页unique键盘/3不同kernel尺寸==actor/最终第四页13字号红绿文本及三echo/有序关闭通过；在途刷新模式三输入仅最后一页，该范围保持明确。最终mise实际入口全部7模式再次通过(`layout-only-final-windows.log`)：storm、pending-refresh、paced-resize、normal terminal、explicit-close、discrete resize、empty reload；LinuxCI恢复注册3-mode lifecycle任务。原几何失败日志保留，不回写成旧构建passed。本轮完整217app/99workspace/69login/check/strictclippy/probe/bindings与443TS通过(`layout-only-final-core.log`)；架构授权文档和索引已同步。尚不能作为连续高负载拖拽性能/systemIME/crossDPI/macOS-Windows/完整M1或release通过，原14staged/8审计阻塞不变。

- 布局退出与EOF补充：当前源码+Astrolabe追踪确认TerminalBank投递排空不是物理PTY退出；实际退出还必须LocalSessionRoot监督器/native controller同源join，controller线程包含PTY resize及read/write线程回收。新增2确定性Native测试扣留TransportWrite::Resize ACK，在关闭bank/真实WindowToken退役/caller和shutdown waiter取消后仍持信用、会话Running，原生ACK后才退信用；失败ACK维持原尺寸且不得旧控制发布。这是受控driver的在途原生证明，不是人为卡住实际OS ioctl的真实窗口验收。16 terminal-bank用例通过。
- 新自然EOF反例先复现最终Surface被清空：End observer取消输入队列，pending输入取消又落入通路failure导致scope销毁。现在连接明确ended；仅输入取消/迟到输入启用回复不报故障，不再安装输入监听；已经绘制确认的结束视图可完成只读ready。Main停止fit并显示结束，不销毁最后画面。ended view不能announceInputReady/pauseForResize/resize，零布局RPC；之后malformedRaw仍正常拒绝并清理（并非全局ignore失败）。3新增连接回归通过，全446TS/type/format通过(`terminal-eof-full-ui.log`)；3Rust文件Astrolabe Exact，TS Unknown仍不是pass。
- 真实自然EOF初验通过(`workspace-eof-native-run.log`/`workspace-terminal-eof-native.log`/`workspace-terminal-eof-final.png`)：独立键盘输入固定fixture-exit，最后正文含`final-before-eof`及红绿/三echo且界面结束；完整End绘制ACK后监督器发固定`terminal_eof_acked`观测，无新增IPC。根清理独立回执nativeExit7/cancelled=false，actor EOF不代替原生退出码；显式回收会话后窗口有序native/profiles/drain。结束后键盘尝试不增加PTY输入记录。脚本是隔离HOME发现Profile/生产入口/无用户启动配置；该路径不是resize ioctl实际阻塞时EOF的系统验证。新增mise `probe-workspace-eof` 与LinuxCI、指南注册，所有期限/信用/协议版本/字体/CSP保持原值。
- 最后源码还追加late-enable拒绝不清屏与ended布局禁止；当前101native/check/strictlint/binding (`terminal-eof-final-core.log`) 和8实窗模式重新构建组合 (`terminal-eof-final-windows.log`) 已完整通过；最新EOF归档及截图再次核对最终正文、End绘制ACK和native7/false。8模式包含EOF、普通终端、显式close、离散resize、批风暴、确认在途刷新、分散resize和空工作区。CI已注册不等于云CI已执行。本次没有声称真实OS resize阻塞下关窗/EOF已验收；该在途窗口退休信用只由可控driver反例证明。stage仍14文件，未重跑不变app217/存储/KDF/默认探针v1v2，release8blockers/IME/DPI/crossplatform未变。

- 自然EOF后的新页恢复已补齐有界精确数据源，不把SessionReceipt诊断screen当完整恢复。actor真实Ended终结点保存一次private immutable快照：最终boundary/decoder EOF replacement/digest/base palette/2000行suffix≤4MiB/full restore≤1MiB；Closed/Failed不创建快照，不继续保活actor线程。capturegap/approx/超预算仍typed拒绝；小于已保存行数或bytes预算直接OutputTooLarge，不能截断VT或丢颜色。每个新视图独立clone不修改原快照，actor控制仍Closed。SessionStream lease0明确仅FrozenEOF、没有actor控制权；仍由Registry当前WindowToken/page nonce与后端新generation签发绘制权限，既有Raw Begin/Commit/End和有界ACK/捕获quota不变。
- `Prepared`新增严格boolean ended绘制元数据（非授权）；Native真实capture.Ended直接禁止input enable和resize，前端即使用户input:true也仅等待完整End后的readonlyready，不装输入、不input_ready、不启动fit。新增3term最终快照/EOF不编码拒绝测试通过，旧stream_tests实际非零通过（首次错误过滤0项没有算pass）；新增1NativeFrozen新页Raw/ACK/confirm与Input/resize拒绝用例、1TSFrozen等待End输入拒绝用例通过。7相关Rust文件Astrolabe Exact，TS Unknown。无二进制格式／CSP／Typeface变更。
- 首次真实`terminal-eof-refresh`模式通过：完整运行一次Shell、三页键盘回显/第四页最终capture、真正EOF后两个新document/nonce、两个新capture confirmations+两个额外EndACK、输入就绪计数不增加、输入记录不增加、native7/false独立reap与有序退出；最终截图仍正文/红绿/三echo/`final-before-eof`。`workspace-eof-refresh-first.log`、`workspace-terminal-eof-refresh-native.log`和final.png已经读取。脚本插入时精确替换工具将shell dollar-quote重复suffix，syntax gate拒绝，之后按完整已读快照write恢复脚本并过sh-n，未将失败脚本算运行通过。mise既有probe-workspace-eof扩为EOF/EOF-refresh两模式，现有LinuxCI入口相应覆盖；实际云CI运行不声称完成。更小容器上的结束网格重排未实现，不授予native resize或裁剪冒充绘制完成。
- 完整core组合(`ended-refresh-final-core.log`)执行check/test/probe/binding和102native通过，但最后strictlint因新增fixture的String.into失败；仅定点去除并将新诊断sideeffect改inspect_err后strictlint与102native再次通过，没有重跑已过fullsuite。全447TS/115term/217app/220store（2ignored原样）及原恢复/probe/bindings有完整回执。当前构建9-mode workspace组合(`ended-refresh-final-windows.log`)失败，六个模式完成（EOF/EOF-refresh/普通/explicitclose/discrete/storm），第七pending-refresh停在首次document确认等待`workspace.confirm_timeout`，native仅navigation_started/document_confirmed，没有profiles或Shell，后两模式未执行。该原组合仍不算pass，没有证据归因EOFcache。进一步核对等待器发现document实际已confirm，缺的是profiles_listed；现在日志区分profiles_requested/load_failed和真正confirm_timeout/profile_list_timeout，并在同PID/唯一窗口确认后保存失败页截图，原8s预算保持。未执行的pending-refresh/paced-resize/empty-reload在同源码的独立任务全部通过（`ended-refresh-remaining-windows.log`），因此9模式分别有实际完成回执，但不把独立pass回写为原组合已修复；首次Profile等待超时仍未定位，后续stage信息可取证。默认probe兼容、各种恢复/actor结束crosscut由本次全test/probe覆盖。原14staged边界与8audit阻塞不变，未新增依赖或安装LSP。

- Alpha主线标签与剪贴板已产品接入（当前Linux，不算全ID DoD）：`workspace-tabs.ts`真实tablist手动激活，Arrow/Home/End仅移动focus，选中重点击不重捕获；受信显示偏好与新页backend目录交叉核对，storage失败不启动替代会话。关闭默认保留（同scope modal）、确认前不dispose view，批准后再原生close+独立native join；不确定结果不自动重试。8Tab、4close、8Decision/PasteDialog测试通过。双真实PTY各自固定PID与输入文件：切换不关闭、原生键盘End/Return切换、刷新恢复B而非A、B回收后A恢复、有序关窗，完整回显后缀用固定13px字形网格/完整N个8pxcells/15pxrow exact像素对照并拒空参考。初始纯白paint/两按钮误选Reload/未安装wmctrl/OCRmisread的probe失败保留，已分别按实际像素和既有AltF4修fixture，不降低信用/超时；代码不执行文本或OCR输出。`workspace-tabs-exact-pixels.log`及two/survivor.png已读。
- TERM-07安全复制粘贴：真实OS ClipboardEvent text/plain，default/probe未提供策略时继续block；scopeCurrent且已Commit/捕获确认启用的输入才可Paste。候选≤1012 UTF8bytes，预留12bracket框架字节，保持输入1024byte；拒绝C0/C1不支持控制符／替换Unicode／超限，不截断/删改；多行（含Unicode separators）、Tab、Bidi/BOM/隐形格式需明确审批，纯文本预览将隐形字符转义，不插HTML。单文档仅一个决策，cancel默认focus，EOF/resize/detach/pageAbort立即撤销等待，不接受lateapprove；public xterm.paste使用当前bracket模式和原queue，不直接旁路RPC。用户选区Ctrl+C使用受信copy/plain，≤4MiB；无选区正常PTYSIGINT，Endedreadonly也允许选区复制，OSC52仍全部禁止。仅显示缓存，不冒称backend深历史精确复制。
- 7policy+4SurfacePaste+2Copy+6Dialog/2共享Decision反例通过（控制、迟到、单实例、unexpectedDOM构造异常释放、noRetry）；`probe-workspace-clipboard`实际GTK selection owner/private Xvfb独立OSclipboard→nativeWebView→PTY，COPY_SENTINEL_DIFFERENT先置剪贴板防copy假阳性；真实pointer选择CtrlC写回对照、单行唯一native输入、多行defaultCancel/explicitTabApprove、刷新后原PID/零旧输入、无选区SIGINT独立trap文件、resize取消审批后lateReturn仅空新行、有序退出。固定Shellread首次INT导致自然EOF正确中断但不适用于后续resize测试，fixture仅已确认INT分支continue后重验passed，不将其算产品错误修复。`workspace-clipboard-interrupt-resize-final.log`/`workspace-copy-sentinel-final.log`/final.png已读；test-only C通过已安装GTK/cc -Werror，无新运行时依赖/安装/clipboardread IPC。
- 分屏前置：bounded immutable树≤64leaf/127node/16depth/8child，严格catalog成员/unique节点和Session（不能复制一个lease到两活动pane），比例≥5% finite、split/remove/promote/equalize/directionalfocus；cycle/sparse/depth/末子删除反例，100条1800操作流，8模型测试通过。布局只是显示模型不能授执行权。`WorkspacePane`每叶独立host、parentAbort晚prepare释放、captureEpoch阻断旧End/error、knownNativeACK才recapture、不重试uncertain；7控制器测试通过，已把single工作区attach/fit从main迁移，尚未挂分屏DOM。
- 最新受影响`per-pane-current-regression.log`：495TS、当前标签/Clipboard/EOF/EOF-refresh/批storm/inflightrefresh/pacedresize全7GUI通过；上一check/binding和474TS+7GUI组合也passed（tabs-clipboard-final-regression.log），未重跑不变store/KDF/Rust/默认fixedprobe全矩阵。Mise新tabs/clipboard两task、LinuxCI、guide、architecture/plan§15同步；云CI只注册不声称执行。Astrolabe symbol索引stale按当前源交叉核对，TS diagnostics Unknown未当pass未安装；design detector报workspace静态heading比率flat警告，真实Operate14pxbody/12pxsection及隐藏h1是保留现有紧凑终端设计，不为静态默认font推断换世界。stage14/8releaseblocks/跨平台IME/DPI/旧Profiletimeout保持。

- Alpha主线基础双pane已接通：WorkspaceSplit选择两个backend已持有目录Session，前端不传program/args/env，当前2-pane左右/上下；每pane独立host/fit/input租约，公共nonceDocument创建比例stylesheet，只用经model校验id/finite比例，strictCSP不改。LayoutView复用stableleaf只重建wrapper，factory失败清理新增/保旧DOM、同id换session拒绝，oldmount迟到failure仅对当时仍currenthandle生效；新增8layoutDOM合同用例，最新全503TS/format/type通过（workspace-split-full-ui.log）。TS Astrolabe工作区/model/split诊断Unknown仍不是pass，未安装语言服务器。
- 实际注册probe-workspace-split通过（workspace-split-full-pixels.log/native.log/column和survivor.png已读）：2个真PTY、各自input/size文件，row独立45×31、column独立92×15内核stty回执，pointer焦点输入只到对应PID；rootTab退回单pane、刷新保2 PID不新增Shell，再分屏后关闭focusA而非rootB、默认Cancel保两PTY、批准后A物理join而B恢复，最终B六条完整echo。column初次截图Native收到完整行但像素尚半字，未当完整drawing证据；加入两方向每pane全长fixedASCIIcell像素比较（非空canonicalecho vs app echo）后注册任务完整通过，不增8秒预算／缩字体／减样本。旧共享日志改split专属，避免标签探针覆盖。
- 当前split只两个现有会话；刷新回选中root单pane，不称布局持久化、嵌套UI、比例拖拽、方向键服务、pane最大化或跨平台完成。Mise新split task/LinuxCI/guide/plan状态与architecture同步，CI仅注册。最新check/binding+9workspace路径组合workspace-alpha-interaction-final.log完整通过；这是rename/marker之前的构建范围，不冒称后续UI兼容已验收。此前495TS+7GUI全套/本次503TS与split独立passed保留。

- WS09窗口标签显示偏好：backend当前会话目录白名单，window sessionStorage版本1／≤64项／32KiB，非法／future／重复／坏unused记录整张failclosed，大小写canonical u64不可数值化；标题≤80UTF16／256UTF8、trim前拒控制/Bidi/隐形字符／lone-surrogate，固定none/cyan/blue/green/amber/red只Radix标记、不授 arbitraryCSS，label以textContent及含原sessionid的ARIA输出，远端OSC标题不自动写label。存储失败只留本页，既有launch/close权限不因名字／颜色变化。5purelabel反例通过，完整508TS（workspace-labels-full-ui.log）。原生modal编辑避免inlineform触发fit/重新输入focus，把名字误发PTY；保存再校target/catalog/life，CtrlShiftR退休关闭dialog，不增加JSON命令/path。
- 实际window label editor/refresh（workspace-labels-color-final.log/saved/refresh.png已read）：真实指针/键盘提交BUILD_TWO，无文字进入任何PTY（各input文件仍1行），换页不新开Shell/保持两PID，显示名恢复；中文select原c英文typeahead不选色，此初次只证明Alias；改真实Home/Down后固定Cyan9像素srgb(0,162,199)保存/refresh均严格匹配，不降色差或仅点击即pass。stage14/跨平台/存储跨窗口同步与appRestartmetadata仍未做。

- LOCAL02自动Profile管理已产品接入（Unix）：workspace_profile_visibility新handler只nonce＋kind＋hiddenbool，复用session.page的local source/WindowToken/currentDocument双查，program/args/env/目录仍backend决定。ProfileReader freshmetadata检查候选身份，prefs格式／deletion在同一个IMMEDIATE edit事务校验，不oldSnapshot unconditional overwrite或自动revive。StateStore::edit_auto_profiles_authorized是trustedRust同步非阻塞predicate，queued执行前校page/Reader closed，deny/panic ProfileAuthorization不关闭共享DB；worker持有captureIOcredit到SQL真的退出、dropguard后才发回执，消费者可立即freshread；已执行/commit不被latercancel undo，未知不retry。5actualSQLite（queuedretire、closedreceipt、predicatefail/panic、callerAbort保quota+commit重开、Drop时replyEmpty）＋2NativeDB（hide/unhide/reopen/旧page/deletion/foreignDirectory/closed）用例通过。optionalShell缺项按当机catalog选择，全5可用时该子分支未覆盖，不称crossplatformMissingShell验证。
- frontend变更adapter/Manager 4＋4用例：nonce/currentOffer/bool、5s取消/timeout不确定、不optimistic/retry/undo，renderrevision拒旧checkbox/newcatalog，pageexit late结果不得freshread/清新busy。原生modal不改PTY几何，成功只freshreadpicker，不close/startroot；全参数／默认Profile／deleted恢复UI待开发，WinACL未验仍unavailable，schema不变。
- 实际ProfileUI→Tauri→私库→2进程（workspace-profile-manager-first.log；hidden/reopen/restored PNG已read）：2原PID存活，隐藏Sh刷新原B唯一input，picker仅Bash；退出physicalreap后同appData/固定候选新app无自动Shell；modalSh unchecked跨process持久，显式restore显示POSIXsh，不新增会话。hidden最后echo截图partial，仅metadata/真实input存活证明，不称完整绘制。Astrolabe5相关Rust文件Exact，TSUnknown未安装LS。profilesMise/LinuxCI/guide/architecture/plan已同步；fullcheck/test/probe/binding＋104workspace-native/lint完整通过（profile-visibility-full-core.log）：516TS、225storage、217app、115term、104native；当前11型实窗完整通过（profile-manager-current-windows.log）。参数编辑／默认Profile／三平台／发布审计门禁仍未完成。

- M1基础字体设置：固定Root目录/opaque same-page Snapshot/CAS接口已注册独立workspace，5真实磁盘授权反例＋5原生page/IO取消/关机用例通过，4 editor/5 codec/2 display及2constructor反例通过；230storage（另2ignored不计通过）、109workspace-native、check/binding/strictlint，当前全537TS及109workspace-native/check/binding/strictlint通过（settings-ui-final-core.log）；当前13型工作区实窗分别通过（settings-font-current-windows.log＋settings-font-remaining-windows.log），原组合260秒超时不称通过，首任务build耗197秒，补验使用同一产物，不增加单窗80秒／8秒观察。Astrolabe6相关Rust Exact，TS仍Unknown未安装。设置未知结果不重试或undo，必须重读；坏文件不fallback/覆盖。实际`settings-font-grid-ready-real.log`和UI截图（default/saved/refreshed）通过：内置14/default→UI单字段保存18/user→刷新同PID，kernel92×29→67×22与原生resize一致，两unique输入、session_opened=1、排空退出。新配置源也是private测试目录，旧11型probe改为显式User13基线，非静默重设编译defaults14。即时生效／Profile/host/group/主题UI、外部文件watcher和Ended小窗口重排未做；旧Profile等待超时与release/stage门禁不变。\n
### M1 SSH Endpoint 元数据目录基础（产品主机／身份入口仍开发中）

- `pulse-core::ssh_target` 将既有 SSH／存储目标规范化统一为无 I/O 领域实现；IPv4／IPv6、ASCII DNS、253-byte 总长／63-byte label、非零端口保持既有 pin 语义。`ssh_endpoint` 只接收有界显示名、目标和协议用户名；无凭据、Debug 或反序列化绕过构造路径，不派生路径或 Shell 命令。
- `pulse-store::ssh_hosts` 在现有 `app_state` 固定键保存版本化 Endpoint 元数据，最多256条／256KiB；UUIDv7 只在后端生成，不能用编辑恢复删除的 ID。全来源字段／版本／规范化／ID／预算校验，坏数据不当空目录也不覆盖；最后一条删除仍保留单调 revision。快照绑定实际 Store 实例，含空库；事务用 CAS，明确检查执行与提交前撤销，失去回执不自动重发。保存不写信任 pin，不批准首次指纹，不授权连接或执行；没有主机路径／写入 IPC。
- 7项实际 SQLite／生命周期／独立进程用例通过：跨实例及旧版本拒写、重开／删除、整个损坏来源拒绝、三个撤销阶段回滚、关闭回执、数量／revision耗尽、panic监督、取消后实际阻塞操作信用保持，以及两个独立进程旧捕获不能覆盖新提交。私库 fixture 未读取用户目录，进程辅助测试标记 ignored，不作为零测试通过证据。尚未验证断电、非合作写者及跨平台结果。
- 完整 `mise run check/test/probe`、bindings 检查、严格 clippy 及原生工作区回归通过：默认存储237项／可选信任241项（各3个辅助 ignored）、终端115项、应用217项、TS537项、原生109项；新增7个 Rust 文件 Astrolabe Exact。共享目标改变后的真实 SSH host-key/auth/PTY/UTF8/EOF 回归亦通过。没有修改14文件签名暂存边界或已知发布审计门禁。
- Unix 工作区主机元数据入口已接通：`workspace_hosts`／`workspace_host_save` 固定应用私库，当前 local document nonce／WindowToken／原始 PageLaunchScope 校验；单次5分钟捕获票据保存后轮换，页面退休撤销。源快照 CAS 与存储实例标识继续生效，SQL 执行阶段仍检查页／Reader，取消不提前释放真实 I/O 信用。字段仅名称、规范化 host、port、username；没有路径、身份引用、凭据明文、指纹批准或网络连接参数。
- Linux 实窗 `probe-workspace-hosts` 已通过添加 HOSTONE／EXAMPLE.COM／2222／alice、修改 HOSTTWO／example.net／2200／bob、页面刷新和三次原生进程重开、二次确认删除；截图确认重开值与删除后空表单，3个进程均正常排空，0个本地会话启动。初始白页截图、错误 hitbox、保存标题裁切和保存后 Escape 焦点丢失分别定位；前两项是探针问题，后两项用单选择器／单表单及当前回执恢复持久完成按钮焦点修复，不延长截止时间。
- 原生49项工作区测试／严格lint通过，新增前端11项有界解码、取消／超时、字节预算、同捕获旧表单／重复删除、字面显示和晚保存反例通过。受影响 mise check、548项TS／113项原生测试、严格clippy、bindings和真实设置网格链通过。首次组合分屏失败保留：两个PTY已完成输入就绪，新主机标题却移动了旧探针按钮；将主机管理置于会话布局控制之后，独立重建的分屏／比例方向导航及注册主机任务通过（hosts-sidebar-order-windows.log）。原组合仍不宣称exit通过，不重复未变的存储／终端全套。Rust诊断无匹配；TS语言服务器不可用，不能计诊断通过。签名暂存和既有发布审计门禁未改变。
- 下一步：身份引用、指纹确认与同一 TermView 的生产 SSH 数据链；此项仅元数据管理，不能计为 VAULT 或 SSH 产品完成。

## Vault 后端密文持久化验证（不是产品解锁）

- `vault_wrap` 的110-byte版本化固定KDF profile envelope；`vault_record` 的有界密文及域／Vault UUID／credential UUID／kind AAD，修改任何预期header也不能用同一key迁移明文。Debug/serde负编译契约通过，原AEAD/RFC KDF known-answer和epoch调度回归保持。
- `vault_database`／既有SQLite worker的实例绑定空快照、全来源形状拒绝、版本CAS、提交前撤销回滚、4096删除墓碑及不可变kind；预算、未知／重复字段、非目标损坏、revision和墓碑耗尽拒绝且原文不变。
- 三独立子进程均要求固定receipt、同一DEK密码认证和凭据恢复，错误密码拒绝、删除后第三次重开无法再Create旧ID；不读真实用户目录。原生条件变量握手证明取消／shutdown时读写信用随实际工作保留，join前不释放。
- `vault-persistence-regression.log`：mise check/test/probe/bindings，251项存储／217项应用／115项终端／548项TS／113项workspace原生、15项doc测试通过。没有重跑未变的实窗剪贴板／分屏等路径，也不以headless作为IME/平台/绘制证明。
- `vault-persistence-audit-fixed.log`：修正3处本地manifest wildcard，Cargo.lock hash不变；bans/sources通过，仍有既有6项许可证／2项维护公告阻断。14文件签名暂存边界不变。
- 本地schema2已接STRICT vault_envelopes／secrets／删除墓碑表：255项完整存储及7项迁移反例、217项应用／115项终端／548项TS／113项原生、check／probe／bindings和三条Profile／设置／主机实窗通过。旧schema1有界来源先备份再同事务导入／删旧键／更新版本；坏来源、hook故障、目的表或备份占用、v2缺表／双来源与WAL读取一致性已验证。日志vault-schema2-{store,consumers,windows}.log；Astrolabe4实现文件Exact，无新增依赖／权限。
- 持久来源／epoch联合确认7项反例及完整回归通过：265 storage／217 app／115 term／548 TS／113 native／15 doc，check/test/probe/bindings；同revision变更、旧来源、认证envelope错配、旧epoch、错误controller拒绝；原生事务锁覆盖启用点，取消立即清除本轮key但保留I/O信用至native退出，未交付旧激活回执不能锁新epoch。真实密码→同一持久凭据认证通过，仍无password/明文IPC。日志vault-joint-confirmation-{core,audit}.log、5实现文件Astrolabe Exact；audit仍6 license／2维护公告，bans/sources通过。
- 身份／引用元数据：9 core／267 storage／217 app／115 term／548 TS／113 native及check/test/probe/bindings/真实隔离SSH兼容通过，2项SQLite反例涵盖缺Vault／nil／角色错配、删除不复活、跨Vault同UUID、immutable旧presence与锁定key。共享Endpoint/Identity严格校验保持原有规则；SecretRef/身份无Debug/serde/明文/路径，presence从VaultSnapshot派生且不是权限。5实现文件Astrolabe Exact；日志secret-reference-{core,audit}.log，audit仍6 license／2维护公告，bans/sources通过；只是模型／projection，身份保存及目标授权未开放。
- schema3身份本地持久化：271完整storage suite/strict lint后，8身份＋2native read/write取消反例通过；三独立子进程明确receipt验证HLC node/UUID/删除保持。创建时间/HLC/墓碑、精确metadata CAS、同事务Vault派生presence、忽略caller has_value／不新关联删除ID、revision/HLC/active/tombstone预算及全坏源拒绝；旧schema先.bak3再原子DDL+版本，保留原envelope与backup，v2缺表/shadow源迁移拒绝。10core／217app／115term／548TS／113native与check/probe/bindings/真实SSH，三条Profile/设置/主机实窗兼容通过；identity-schema3-{store,consumers,windows}.log。Astrolabe并发首轮超时不计pass，构建后串行6实现文件Exact；14签名暂存及既有6 licence/2维护公告门禁不变。该轮尚无身份页面/IPC/SSH凭据使用授权或远端sync merge，详见architecture/ssh-identity-persistence.md。
- 身份当前页元数据admission：该轮workspace身份read/save已注册，但UI未挂载。Snapshot同步捕获Vault namespace，Create仅label/login并后台预留引用，不接原始secret/Vault ID、password或has_value；缺Vault显式拒绝，无默认DEK。原Host ticket抽为typed通用bank，原300s/单次/准确scope/registry/refresh/exit撤销不变。3新增native真实私库/页面反例、strict lint／原bank及7实现文件Astrolabe Exact通过；277 storage／217 app／115 term／548 TS／116 native与check/test/probe/bindings通过。首次主机probe在profiles_listed前silent native exit、空log未定位，保留failure artifacts；构建结束后同binary/原budget实际CRUD／重开单次复检通过。identity-admission-{core,host-window-recheck}.log；只是backend入口，不计身份页面/IPC绘制或SSH使用权限，发布门禁及14签名暂存边界不变。
- 身份元数据UI：12前端decoder／late read/save／detached form／不确定timeout与no retry／二次删除／same-page focus反例通过，560TS及116native/strict lint通过。固定字段只接名称/login，不接password/refs/flags；namespace不存在显示禁用状态，不生成默认DEK。实际缺Vault read/draw/exit、test-only ciphertext-shape namespace、鼠标/键盘create/update/refresh、三进程reopen/确认删除空表单/每次正常exit及零Shell启动通过；task/Linux CI registered，身份／主机／设置／分屏4组合通过，identity-ui-registered-regression.log。不是认证/解锁或SSH消费。TS MCP Unknown不计pass，native Exact；detectorflat-font警告与实际14/18共享CSS/截图不一致，不改product字体。修正controller TSV列、fixture Cargo cwd与isolated toolchain解析；原80s/8s不变。创建窗口确已保存正确字符串但缺共用CSS选择器造成拥挤/OCR失败，两个共享`:is`selector修复后完整重检通过。原silent native startup风险及6 license/2维护公告发布门禁、14签名暂存边界不变。
- Vault初建／current-page owner：4 encrypted-only creation/CPU取消反例、2same-epoch撤销、4 owner来源/页面/真实密码/receipt-loss/poison反例及3共享ProfileReader native服务/SQL实际取消drain反例通过。283 storage／221app／115term／560TS／119native完整受影响回归及后续SQLcancel单测、5固定status/lock adapter拒绝/abort/timeout反例、strict lint/check/probe/bindings通过，vault-owner-{store,app,native,consumers}.log。Root已接Started／Destroyed／ExitRequested／physical barrier，已accept key跨换页但pending/未交付结果撤销；锁/关闭清key不close PTY/DB。创建解锁仍cfg(test)且无password IPC，生产status/lock只返回version1固定state。release创建+认证+SQL exact test0.42s不当作单KDF／GUI／跨平台延迟保证。首1-thread fixture SQL常驻占满导致150s timeout，SQL+KDF fixture改2、queue保持1、globalKDF1，deadline不变。下一步front-confirm/cancel ticket，JS Promise abort并不自动取消Tauri native工作；成本/密码强度、GUI输入、SecretRef使用与钥匙串仍未计验收，发布/签名边界不变。
- 表布局及联合确认只是后端本地持久化／启用边界，不是完整可同步Vault或创建／解锁UI。系统钥匙串、成本策略、SecretRef协议使用／窗口授权、应用锁、Windows ACL、power-loss和离线回滚检测仍待验收，详见architecture/vault-persistence.md。

### M1 当前页保存身份密码开发预览

- 当前workspace单独注册prepare/submit/cancel；current WindowToken/local source/document nonce、native owner与saved identity/Vault源/原AvailableKeyEpoch/5s单次intent固定。准备保真实I/O至SQL退出，submit不再次reserve；无SecretRef/Vault/path/kind客户端替换，无明文读取或SSH使用许可。输入≤4096-byte非空UTF-8、不套新建主密码规则，字段/owned数组清理不承诺JS/IPC内部副本全零化。
- 18针对性form/protocol反例与596 TS通过；完整295 storage／229 app／125 native（2专用child不能当普通pass）、check/test/probe/bindings通过。Rust Exact，TS LSP Unknown未安装，不计MCP通过。UI沿用单selector/同一视觉系统但密码独立form；未存元数据不能提交密码，旧页/结果不回填、不抢焦点，不确定写入不自动重试。机器detector零findings不代替实窗/a11y验收。
- `mise run probe-workspace-identity-password`及Linux CI已注册：隔离HOME/XDG/固定私库，真实master创建保持Locked→身份保存→Locked拒绝→显式master确认→一次写入→真实刷新及native进程重开。独立Rust child实际恢复同DEK/namespace/role/epoch并比较固定密码字节，明确1 test与private-byte receipt，无任何密码读取IPC；native verifier提前编译、原80s/8s/5s截止不变。valid saved/reopened PNG显示presence已存、密码框为空、保存不授予连接。
- 原身份缺Vault/CRUD/刷新/重开/二次删除、master认证取消/换页、设置真实字号网格、双pane及比例方向两模式兼容均通过。首次组合设置在terminal_input_ready前bootstrap_timeout，未归因且不计组合通过；保留identity-password-settings-bootstrap-timeout.log，独立重检通过并补失败-only截图，不能称风险消除。
- 两次child校验失败分别是Cargo package cwd与误把Vault UUID当credential UUID；仅修夹具，不放宽路径/AAD。新密码失败清除字段时补current-page Done焦点反例，实窗本轮用独立鼠标点Done；键盘关闭/跨DPI另验，GTK focus假定失败不由DOM stub抹去。外围audit20s超时非通过，实际日志仍6 license/2维护公告、bans/sources ok，发布/签名暂存边界不变。身份密码 UI 的证据不等于目标使用或产品 SSH 连接；新增显式后端动作见下节，钥匙串/三平台仍未完成。

### 首次指纹候选、原页 guard 与事务批准后端

- `mise run probe-ssh-first-pin`只在Unix/ssh-target-probe可选构建：pre-dial同一完整源＋后端saved host → 实际TCP offered key（永远拒绝auth）→ 原registry page/explicit confirmation → 同事务完整CAS首次pin → 私库worker重开 → 新strict PinnedTransport。没有frontend raw key、Vault unlock、credentials、自动reconnect或Actor。报告`ssh-first-pin.json`含实际key、foreign registry拒绝、旧页拒绝、最终SQL页撤销rollback、新页批准、reopen strict peer、auth0和productSshOpened=false；必须逐项成立。
- Observer三个反例验证missing source/无SSH bytes、unknown/own changed/system correct/known matched全部auth0/owned I/O admission释放、SDK已spawn握手后的caller abort与原5s native deadline真实EOF/额度释放；不能用outer future取消或proof bool当join。SDK shutdown会覆盖提前closed I/O的UnknownKey为BrokenPipe，已按真实顺序修正，不放宽拒绝或截止。
- First-use实际TCP adapter验证显式cancel、原native guard拒绝（此单测不是WindowToken）、foreign StateStore、提示后host字段改动、known匹配/changed均不first-use覆盖，一次正确写入revision1/auth0。三个SQLite反例含同revision外部host drift、wrong/foreign host/store、final revoke rollback、同target/算法变更拒盖、reopen持久及actual SQL阻塞后caller cancel：guard信用留到worker退出且revision0；fake形状键只测存储，不当公钥认证。
- Upper probe进一步使用actual ProductDocuments/PageLaunchScope/SessionRegistry：原nonce退休→拒绝，foreign registry即使同号window仍拒，SQL最终predicate内Started→rollback，新页确认→revision1，重开后实际peer正确且无auth。不是Tauri Window/WebView用户点击验收，当前无页面审批ticket/指纹UI。
- 完整check/test/probe、308 storage（5专用child不计pass）、22 SSH/4 doc、236 app/8 doc、596 TS、旧auth0/密码auth1、bindings/default desktop无russh通过；相关Rust on-demand Exact，CI已登记但不声称远端CI运行。audit仍6 license＋2维护，bans/sources通过。原型app_state信任、正式known_hosts迁移、host certificate/已变更替换流程/用户SSH读取/SSH actor与三平台独立待验；验收批次后clean-dev保留target/probes。

### 显式后端目标密码动作与真实 SSH 消费

- `mise run probe-ssh-password-use`（Unix、ssh-target-probe可选）使用合成固定密码和临时私库，后端初建／认证同DEK／保存身份密文 → 当前page显式动作 → 精确源/原epoch → 上层actual TCP peer/pins → SDK密码认证 → actual owned-I/O drain。`target/probes/ssh-password-use.json`要求auth=1与精确固定字节、unknown/wrong key pre-auth拒绝、prepared后pin来源变更拒绝、实际错target/retired page拒绝、nativeAdmissionDrained=true、plaintextReturned=false、productSshOpened=false。旧`probe-ssh-target`仍auth=0／credentialUsePermission=false，二者不得混用。
- 真实密文的受控 Rust sink 测试验证普通control不可升级action、wrong sink auth0、同DEK重新认证拒原epoch、结果等待方取消仍保Busy到cleanup退出、已成功auth但锁定期间阻塞drain拒迟到回执；lower原weak epoch source destruction不保controller。不能把受控sink当真实网络auth中取消，也不把owned-I/O receipt当SDK/Actor/远端进程join。5s截止、SQL/CPU/native信用和不确定发送无重试规则不变。
- 完整check/test/probe、305 storage（5专用child忽略另由父进程调用）、236 app、8 app doc、125 workspace native（2专用fixture不计普通pass）、原auth0／bindings与默认workspace normal tree无pulse-ssh/russh通过。5处Rust on-demand diagnostics Exact；TypeScript Unknown状态不变。Linux CI已加独立动作probe。
- 完整链初次链接失败是共享/tmp inode100%，并非剩余字节耗尽；没有清理其他项目或验收证据。改用本项目0700 `target/build-tmp` 为 TMPDIR 后完整440s通过。失败与通过日志分别为`ssh-use-consumers.log`／`ssh-use-consumers-isolated.log`；不把第一次失败抹去，也未扩大单项deadline。
- 此轮验收限于后端显式动作与固定实际密码认证，接口只交付连接关闭后的历史bool，绝不是前端明文读取、首次指纹批准、认证中网络取消、SSH actor接入、用户可启动生产连接、钥匙串或三平台验收。6 license＋2维护公告及签名暂存边界不变。

### schema 4 正式应用信任表与实体元数据

- 默认开库升级 `known_hosts/state`，旧 schema 1–3 公开 pin 文档严格导入；保 `.bak-4`、顺序和空目录 revision，删除旧文档／读回／版本更新同事务。坏 trust 的 v1 升级回滚 Vault、identity、trust 全部新表，备份及原文保留；备份冲突拒覆盖。缺正式表／双来源拒绝，不回退原型。
- UUIDv7、创建时间、稳定节点 HLC 和行删除墓碑持久化。真实 reopen／显式恢复保 ID、推进 HLC；删除清 key。原始合计64KiB、128活跃、256墓碑；额外删除超上限整事务回滚、不隐式清墓碑。长行／129活跃／总预算／错误目录／UUIDv4／时钟耗尽、同revision有效元数据变更拒绝全源及普通edit。
- `ssh-schema4-metadata-consumers.log` 的 check/default test/probe通过；optional Store 仅耗尽版本测试的fixture端口22/2222不匹配失败，改用既有target helper后 `ssh-schema4-metadata-followup.log` 完整317 storage（5专用child忽略）、22 SSH／4 doc、236 app／8 doc、125 workspace native（2专用fixture忽略）、独立进程Profile私库、actual first-pin auth0／target auth0／password-use auth1、bindings通过。前端596测试通过；零测进程不计所有权验收。
- 最终 metadata build 的 `ssh-schema4-metadata-windows.log` 通过实际 hosts add/edit/refresh/reopen/delete、Settings14→18字体内核网格、Vault createLocked/wrong拒绝/确认unlock/lock/refresh/reopen；不算指纹对话框或同步。default workspace normal tree无pulse-ssh/russh；新Rust文件诊断Exact，TS Unknown不提升为通过。Linux CI执行 optional storage 全套及三条SSH动作探针。审计仍6许可＋2维护，不改变签名暂存边界。
- 权限、单项截止及取消保实际I/O信用不变；不含用户SSH文件、host cert、已变更key替换、生产连接、Windows ACL/断电/三平台。

## 工作区嵌套布局（Linux限定验收）

- `WorkspaceSplit`／`WorkspaceLayoutView` 已接通焦点叶嵌套、复用已有控制器、分隔条主指针松开提交、键盘比例、hidden/inert 最大化、sessionStorage 原窗口刷新恢复和关闭后剪枝；显示偏好只与后端当前目录核对，不启动缺失会话，不提供输入授权。最多八窗格、20–80% 双子节点比例；混合 Ended 或原生调整在途拒绝新布局动作。
- 原28项直接相关布局／控制器／偏好测试与新增两项隐藏首次挂载反例通过，UI 类型与 Biome 检查、探针 Pyright 通过。公开 `mise run probe-workspace-nested`／Linux CI 的单条三PTY真实WebView已过：嵌套网格31×45／15×45／15×45，松开后10×45／20×45、Esc零调整、键盘比例、最大化输入隔离、刷新精确PID、关闭焦点后剪枝及全部native退出。证据 `target/probes/workspace-nested-acceptance.json`、`workspace-nested-probe.log`、`workspace-nested-native.log` 与 actual screenshots。首轮下拉操作／按钮定位和最大化刷新零尺寸捕获真实失败均保留；改为隐藏新叶延迟首次attach，已有叶不重建，不放宽geometry／Commit授权门。
- TS LSP Unknown 不计通过；无新增依赖／Rust生产源码改动，沿用既有审计阻断，不为该纯前端变化重复完整check/test/probe/audit矩阵。布局Linux子批已验收，CLI嵌套衔接正在独立定向验收，未执行clean-dev以保留在用二进制；连续拖动性能、IME、DPI、三平台、跨窗口／进程布局持久化和发布门禁仍待。架构见 [工作区布局](../architecture/workspace-layout.md)。

## 尚未验证

S1 已取得 Linux 合成通道/背压的部分证据，但未完成三平台验收。ConPTY、窗口材质/IME、真实协议/桌面/CLI已有部分受控证据，但真实设备与生产接口、Windows管道ACL及CLI窗口启动仍未完成。PTY 已有 Linux Profile→真实交互TermView、输入/ACK、尺寸、标签/双pane、刷新/EOF恢复与退出排空的受控产品链；完整login shell矩阵、Windows ConPTY、系统IME/跨DPI、连续高负载与进程树回收仍待验证，不能把finite helper扩展成三平台完成。类型生成已有 Linux 控制面证据，但三平台与 RC 依赖门禁未完成。已批准的非活动网格只读getter及双屏恢复已接入；隐藏光标分离/非行尾wrap等不能无损编码的状态明确延期，产品attach对不可精确恢复保持typed拒绝；xterm.js 与 alacritty 的边缘序列分歧需要重新同步策略。Unicode 标量表已统一，但更多组合边界、字形与控制序列交互仍待验证。原生依赖门禁审查通过之前，不得打包分发验证应用。
