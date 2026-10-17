# 终端布局控制与输入授权

## 决策

用户已确认分开布局 resize 和输入确认（本决策不改变二进制帧格式、CSP、Profile 执行权限或终端颜色）。`TerminalBank` 的 resize 为布局控制，不要求先通过捕获绘制确认。输入仍必须取得监督器验证的 Commit ACK、原始捕获摘要确认和显式输入启用。

布局请求必须同时满足：真实本地 Tauri 来源和 WindowToken、当前页面 nonce、后端分配的不透明连接票据和绑定代际、会话归属、已连接 Channel、有界字符尺寸及每代际单次调整。旧页／旧代际／跨会话不能升级；布局控制不能执行命令、启用输入或清除风险标记。

## ADR-032：本地与 SSH 的尺寸完成语义

用户已确认采用分开的完成语义，尺寸语义基础已验收：6 个确定性 actor 反例、12 个 page 绑定回归、37 个前端针对性测试、实际无回复 loopback SSH、完整 check/test/probe/bindings/lint 与本地三种真实 WebView resize／refresh／input 路径通过。生产 SSH GUI、OpenSSH OS PTY、三平台与性能仍未验。本地 PTY 的 `LocalNativeConfirmed` 只在实际原生 resize 成功后发布；SSH 的 `SshWindowRequestAdmitted` 仅指 owned SDK 已接收 window-change 请求。RFC4254§6.7 要求 want_reply=false，并不提供远端内核尺寸生效回复。

可信后端 adapter 在 actor 创建前固定 transport basis，不能由页面、远端输出或普通 session ID 声明；driver 必须兑现该完成契约。actor 在成功后应用 Rust 权威网格并撤销旧 lease，才产生 `ResizeReceipt`。原页／owner／generation 在上层再次检查；NoOp 为无新 transport 请求，不伪造确认。前端只将经严格尺寸和 basis 校验的控制回执用于重新捕获；它不是输入、主机或凭据权限。

本地原生确认未放宽，SSH 永远不标记 remote kernel confirmed。终端 v1/v2 二进制帧不变，控制 JSON 增加 columns／rows／basis；两端须协调更新。错误／超时／丢失回执不自动重试，已准入效果不回滚；I/O 信用仍保留到实际后台退出。自然 EOF 的只读恢复不重新开启 resize 或 input。

## 时序与资源

新 Surface 在绘制前发现实际字体网格不匹配时，停止旧帧消费及 Data/Commit 的绘制 ACK（已完成的头和元数据 ACK 不回滚），保留会话和原生租约。沿用 50ms 最新目标合并；只在相应 transport 完成且 Rust 显示网格已应用，或目标在尚未提交时回到原尺寸时，返回 `TerminalLayoutChanged`，由工作区重新捕获新代际。字体大小、DOM 几何／文本校验、两 rAF 门禁和历史恢复策略不变。

已提交请求的超时、错误或不确定结果不能自动重试或声称成功。3s 捕获预算在尚未原生派发时有效；派发后使用原有 6s 布局不确定期限和后台原生超时。隐藏且无可测网格不能提交零尺寸。Observer、合并器和晚结果归原页面／视图生存期，退休即弃发布，不提前释放后台实际 I/O。

原生短准入锁仅串行输入许可、输入信用获取和 `resize_started` 设置，不跨 await。resize 永久撤销本代际输入；迟到的启用或待验证输入不能重新开启。已准入输入不可撤回，布局工作与其实际完成串行。IPC 调用取消不结束原生任务，也不释放其信用；窗口退出仍等待实际排空。

## 自然 EOF 后的只读恢复

actor 真正结束前保存一次不可变精确快照（只在 Ended；Closed/Failed 不伪造 EOF）：基底调色板、历史 suffix、完整屏幕恢复 bytes 和相同最终双坐标／revision／digest。最多 2000 行、4 MiB 历史＋1 MiB 屏幕；无近似恢复、无超预算部分数据。失败或不可编码保留 typed RestoreGap；后续较小预算／行数不能用截断 VT 冒充成功。

结束快照不保活 actor 或 PTY 线程。新的只读订阅没有 actor lease；应用仍签发当前窗口／页面／代际授权并通过原 Raw Begin/Commit/End、捕获确认与有界绘制 ACK 监督器。新页面元数据的 ended 仅用于避免尝试输入启用，Native 同样按真实捕获的 Ended 状态禁止输入和 resize，不因验证 Commit 重新授予权限。最终原生退出码仍由同源 NativeExit 独立证明。

目前结束快照保留原有网格尺寸；更小窗口上的重排／重新编码未实现，不能开启 native resize 或裁剪冒充完整网格绘制。历史限额、错误拒绝和跨平台限制保持明确。

## 验证边界

原生与前端单测覆盖未 Commit 调整、输入拒绝、代际／页面拒绝、取消后的实际信用、明确完成后的重捕获和不确定结果不重试。真实窗口风暴／在途刷新另按 `docs/development/validation.md` 的实际回执验收，不以单测替代系统 IME、跨屏 DPI、连续拖拽性能或跨平台结论。
