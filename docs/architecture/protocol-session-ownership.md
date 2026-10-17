# 协议原生会话的根所有权

`pulse-app::protocol_session_owner` 是不依赖 SSH 网络库的可信 Rust 生命周期边界，不是目标、指纹、身份、凭据或页面权限。协议适配器仍位于上层，必须独立验证实际连接与原页/source/epoch，并明确授权用户操作。

## 共享准入与不可替换来源

- `ProtocolSessionReservation::reserve` 使用现有 registry 的 64 个会话/启动信用及共享 64 个原生工作信用；不能另开一个不受本地会话预算约束的网络池。
- `begin` 必须先于实际原生 I/O；未开始的预约可以释放，已开始但丢失/失败/未知完成的预约隔离保留，不能当作已清理。
- `StartedProtocolSession::commit` 使用原预约的 registry 与 page generation，在同一 registry 短锁内提交。换页不能将旧预约升级到新页。
- `ProtocolSessionOwner` 的 registry、ID、handle、completion 均不可由外部替换；两个 registry 的相同数字 ID 不等于同一来源。根集合必须检查 `is_managed_by`，不能按数字碰撞回收另一个实例。

## 完成与取消

适配器提供的 owned `native_join` 必须成功等待 actor **及全部实际原生 I/O** 退出。通道关闭、actor EOF/Closed、撤销布尔值、超时或 SDK 发送入队都不是该证明。此接口只接受可信后端实现，不接受插件、页面序列化回调或前端报告。

- 提交拒绝先关闭 actor，但后台 supervisor 持有预约和实际 join 到退出；不能通过取消等待提前释放信用。
- `finish_unregistered` 用于认证/设置失败且尚无 actor 的实际 close-and-join；观察句柄丢失仍继续监督，成功清理才释放。
- 根 owner 不放入 page/ViewHost。换页后已有根会话独立存活；drop/explicit cancel/取消 finish 等待均触发脱离等待者的回收。
- 只有成功原生完成后才调用同来源 `reap_closed`。错误完成、孤失 receipt 或运行时 teardown 不得释放记录/假装退出。
- `seal_native_workers` 覆盖注册的本地和协议 supervisor；旧 `seal_local_workers` 为兼容委托。它本身不关闭 owner、actor 或未注册 I/O，应用退出仍必须先请求关闭并保持运行时到根回收与共享 drain 成功。

## 验收边界

七个无计时睡眠的 actor/oneshot/watch 反例通过：原页退休、actor 已 EOF 而原生仍在执行、提交拒绝期间双预算占满、finish/cleanup waiter 丢失、相同 ID 跨 registry、未知完成隔离、无 actor 的已知清理及未启动预约释放。`mise run check`、默认 246 application／7 doc、ssh-use 248 application／9 doc、原 PTY probe 和严格 lint 通过。

## 独立会话动作与受控实际 SSH 移交

`PageVaultOwner::begin_password_session_action_authorized` 是独立后端动作；普通预备回执或历史 auth bool 不能升级为连接。`PasswordSessionSink` 在精确来源、实际 peer、原 page 与已确认原 epoch 有效时短借密码，随后等待 PTY／shell 设置回复并再次检查原授权；只可一次移交 `AuthenticatedProtocolParts` 给原预约的 root owner。没有密码读取／序列化入口。

丢弃 setup／完成回执、认证后锁定、foreign page 预约与原生清理挂起均有真实密文反例。setup 取消保留原信用到实际清理；已有根会话不保留凭据操作准入。丢失已完成 owner 回执会关闭 actor，但原生 join 仍后台继续。`PinnedTransport::close_and_join` 分别等待 SDK Handle 与实际流释放；这只证明资源退出，不代表认证、远端退出码或进程树成功。必须先释放所有 channel/read/write handles，避免已知接收队列 HOL 阻止 SDK 回收。

`mise run probe-ssh-owned-session` 使用私库合成密码与实际 loopback SSH：auth=2，第一连接在换页后继续 actor 回显并处理有序 EOF，第二连接在挂起读取时明确关闭并完成 actor／SDK／流回收。读写分别有界并监督；它是 32×8→64×16 的合成服务端 PTY，不是 OpenSSH OS PTY 或真实 SSH WebView。ADR-032 已接尺寸通知：可信 adapter 固定 `SshWindowRequestAdmitted`，owned SDK 请求入队后 actor 应用权威网格；当前页 binding 返回 typed receipt，旧 lease 不得输入。实际 TCP 服务端记录 window-change，但故意不回复；report 的 `remoteKernelResizeConfirmed=false` 保持，不能将入队或 callback 冒充 kernel 确认。本地原生完成未放宽。控制 JSON 增加受校验的 columns／rows／basis；终端二进制不变。6 actor 反例、12 page 回归、37 前端 targeted、完整121 terminal／250默认app／253 ssh-use／626 TS／125 native（ignored不计）与 check/test/probe/bindings/strict lint、本地三种实窗尺寸与刷新输入路径通过。显式 workspace-ssh 的保存主机／身份密码会话现已接同一 TermView，默认工作区仍关闭 SSH 工厂；受控 Linux 样本不是完整产品或远端 OS 验收。

受影响回归：默认 application 246、ssh-use 249、SSH 22、storage ssh-trust 317（ignored 不计）、TS 612 与 check／probe／bindings／strict lint 已分别通过；组合原始运行曾失败于独立 worker `Database`，后续又遇 cancellation fixture 将 schema open 计入 2s join。仅 fixture 将准备阶段移出原取消截止，生产超时与权限未改；后者针对性和完整存储复验通过，前者来源未定位且不得被复验抹除。核心、适配器及 fixture Astrolabe Exact；stale graph 不算验证。

共享根集合已接协议 owner：`SessionRoot` 为原 `LocalSessionRoot` 的兼容别名，同一对象的 local／protocol owners 共用 occupied=64、retiring watches 与 registry 双预算。原 local API不变；adopt_protocol 拒 foreign registry／same-ID，EOF／未知失败不清槽，close_all／shutdown覆盖全部实际native监督。工作区根状态与授权后的 close 已识别协议资源回收，不能伪造本地 native exit_code。四个原生latch反例、原local18回归、默认250／ssh-use253 application、125原生（2ignored不计）、实际SSH shared-root auth2与check/probe/bindings/lint通过；root LSP独立复验Exact，之前MCP超时不是通过。并行网络任务曾在Cargo artifact锁上耗尽180s，未执行行为；native完成后只补实际probe并通过，原crypto／peer／取消截止未改。

## 显式工作区密码会话入口与 TermView

`workspace-ssh` 的 Unix 工厂通过 `workspace_ssh_session_begin/prepare/start/cancel` 接入；默认构建仍返回 disabled，不能继承验证 peer、私库 seed 或固定密码。页面仅选择已保存的 host/identity UUID，不提交地址、key、程序、环境或凭据。`SessionRequests` 的单次有界票据冻结原 page/WindowToken/available epoch，准备阶段持有实际读信用，启动再验证完整源和实际 peer；过期、取消、换页、失败与丢失响应不能借新 epoch 或提前释放正在执行的信用。Root 已接收的会话不属于页面；迟到的 session ID 不能发布到新页，但 Root 继续持有并排空资源。

请求 bank 显式持有应用 Tokio RuntimeHandle，所有监督/到期任务由该 Handle 启动；不能假设同步 Tauri 主线程处于 Tokio context。真实窗口发现的无 reactor panic已修；普通未 enter test 及实际 SSH auth2/后台读取消 Pending→exit/Root drain验证通过。Runtime退出不是成功回收。

复用现有 v2 Raw Channel/Begin palette/Commit draw ACK/capture confirmation/input credit/ADR-032布局授权与 TermView，无二进制格式或权限放宽。真实Linux隔离peer/私库/Vault UI流程auth1、三唯一键盘echo、刷新同Root无重认证、SSH window-change 92×29→97×30、window close与明确close默认保留/再次确认后的protocol nativejoin/peer stream退出通过。自然EOF实际窗口补验通过：合成peer发送remote exit-status7与EOF/close，actualstream退出与End drawACK独立；final五行保留，原三echo与末尾可见，两次真实refresh共3次capture/EndACK恢复同一Ended Root，不重新认证、启动、增inputready，afterEOF键盘无写入，正常app退出排空。沿用已有bounded immutable ended snapshot，产品状态机未改。remote7仅fixture发送的SSH exit-status，不冒充本地NativeExitReceipt或远端OS进程结果。fixture严格lint/Astrolabe Exact。测试驱动修了native read日志早于frontend busy解除的刺激次序与OCR大小写误读，不改变原5s/8s/1s/80s截止。挂起握手实窗负向亦通过：受控TCP peer只有banner而无KEX/keyproof，不能完成凭据认证；dialog明确取消与navigation原页退休两次实际owned stream EOF均在2s观察窗内，不等原5s expiry。第一次actualexit后第二次admit通过，新页Profile读正常，Root/session/inputready均零，应用正常退出排空。只新增fixture与成功cancel日志，严格检查和对应原生command/fixture Astrolabe Exact；不把撤销bool/SDK请求入队当资源退出，也不称通用网络取消性能。使用 `mise run probe-workspace-ssh-session` 的window/close/eof/cancel/commands五mode；commands补验当前页palette connect选择原dialog，不增加目标/凭据权限，密文auth1/同Root/3echo/resize/exit过；fixture为真实russh协议，不是远端OS。SSH回执仍不称remote kernel ACK。完整基础／253 ssh-use／642 TS、20 frontend targeted、132 workspace-ssh原生(2ignored不计)、严格检查、原OpenSSH审批auth0与默认工厂三页刷新/drain兼容通过。bank/command Astrolabe Exact；starter两次2.5s诊断timeout与TS初始化失败为Unknown，不当通过也未安装/重启语言服务。

初次Profile列表8s迟到仍未定位；之后setup默认xtask被基础probe覆盖、误等未调用的Vault read日志与OCR字形误读已单独定位，不能抹除首次时限风险。系统IME、远端OS PTY、连续负载、进程树、其他认证方式及三平台验收仍未通过，审计八项及签名边界不变。
