# 密码与 MFA 联合认证

保存及临时目标的GUI直连、保存直连CLI、保存路线每跳明确选择已通过Linux原生验收。临时目标使用独立 `prepare_temporary`，保留原点击期限、原页／代际和临时票据活性；保存 `prepare` 继续拒绝临时来源，`prepare_route` 仅接受冻结保存路线。

## 授权与期限

`PasswordInteractiveSessionGesture` 在原点击捕获页面、Host/Identity、可用Vault代际和固定300秒截止时间；批准不能借用后续解锁。动作和意图不能由普通直连密码请求、只读来源或认证布尔回执转换。保存 `prepare` 仅接受保存直连，`prepare_temporary` 仅接受临时直连；路由、来源类型不匹配、陈旧快照及取消均拒绝。普通密码准备仍为5秒。

`workspace_ssh_password_interactive_session_begin` 是独立命令，复用现有问答Channel、原请求令牌和一次性轮次。共享准备命令通过当前页临时票据解析可信来源，再调用对应的保存／临时授权入口。界面在保存及临时直连中提供“密码 + MFA”，保存路线经独立逐跳入口选择本跳方法；方法不可用或失败时不改用其他认证方法。联合适配器保留精确身份、来源类型一致、原目标活性与实际peer核验；成功注册后临时目标才获得可选保存回执。

## 两因子与明文生命周期

可信 `PasswordInteractiveSessionSink` 的第一阶段只接受实际密码部分成功，且服务端下一步必须包含 keyboard-interactive。密码单因素完整成功、拒绝以及其他后续方法都不能建立联合会话。

第一阶段返回后，原密码借用在既有监督任务中结束并清理。问答在 `prepare_authenticated_terminal` 中执行，适配器只保留用户名，不保存密码。每轮沿用有界 Broker，并在开始、显示／回答边界重新校验原密码来源、页、代际、截止时间及实际 peer。MFA 完成后才准备 PTY，经原来源复核和统一 `AuthenticatedProtocolParts::commit` 注册会话及冻结行为设置。

取消、页变更、锁定、超时、错误回答或来源变化均进入原物理排空链；不把取消确认当作原生退出，不提前释放容量。密码和问答值不进入日志、页面返回值或持久化配置。

## 前端问答与答案回执顺序

共享直连客户端将正在答题与已提交、等待原生回执区分开。服务端下一轮问题可能先于上一轮Tauri答案RPC回执到达；此时最多保留一个经版本、轮次和大小检查的问题，等待有效回执后才显示。原页失效、取消、超时、无效回执或第二个排队问题均拒绝，答题期间不接受另一轮，保留固定300秒期限、八轮上限和唯一轮次。答案数组仍在原回执处理结束时清理，不记录内容。

四种明确MFA方法的正例均在旧实现复现误取消；修复后60项客户端协议／生命周期测试及完整1530项前端回归通过。该修复只涉及共享GUI直连客户端，不据此扩大逐跳或企业MFA验证结论。

## 当前验证状态

当前扩展通过类型／格式／双语检查、57项前端回归、4项授权生命周期回归、8项共享适配器回归及生产严格Clippy。正式任务版本2的15项实窗检查通过：保存链的普通密码不升级、异常方法拒绝、取消／错答／来源变化；临时目标取消不落盘且原成功会话继续存活、暂不保存后的输入／刷新、成功后明确保存精确复用已有目标。共四次两轮MFA成功，六条明确测试输入，十二条连接及原生工作者全部退出。收据、截图、源码与日志归档于 `target/probes/workspace-password-interactive/accepted/eab088ff/`；先前保存直连九连接收据保留在 `accepted/478d3e5c/`。

锁定russh服务端会覆盖密码部分成功位；测试沿用独立解压、归档摘要验证及单分支修正，产品客户端、注册表缓存和仓库锁文件不改。运行前校验服务端二进制摘要及 `partialSuccessMethod=password`；首次未修正服务端的失败保存在 `attempts/c26892e1/`。生产客户端正确拒绝了实际false的部分成功位。企业PAM和其他平台继续待验。新建主机保存已通过版本3的19项实窗检查、6次联合成功和16条连接退出，见 `target/probes/workspace-password-interactive/accepted/bdac59a7/`；保存逐跳验收见下文。

`pulse ssh <保存入口 ID> --identity <保存密码身份 ID> --password-interactive` 已通过 Linux 生产 WebView／真实 CLI／loopback SSH 的冷暖验收。四次实际密码部分成功、两次两轮MFA成功和四条连接退出；锁定／目标取消及目标批准前零密码使用，Esc和CLI断连各物理退出一条连接，刷新前后四条测试输入均被peer收到且不重认证。四个GUI实际退出0，控制端点排空，保存元数据与密文摘要不变。58项前端、28项CLI、9项许可、5项密码授权回归及生产严格Clippy通过。正式任务为 `mise run probe-workspace-cli-password-interactive`，最终证据为 `target/probes/cli-password-interactive/accepted/f1063366/`。测试服务端仅隔离修正密码部分成功分支并校验归档／源码／二进制摘要；生产依赖与仓库锁文件不改。首次原生任务在冷窗口定位期限内失败，日志保存在 `target/probes/cli-password-interactive-attempts/06aa2ca8/`，没有认证事件，本轮成功不建立该启动故障的原因或修复。企业认证和跨平台继续待验。

## 逐跳密码与 MFA 联合认证

开发链新增 `prepare_route`，只接受冻结保存路线源。`RouteHopSelection::into_password_interactive` 消费原密码hop，不重捕获页面、Vault代际或期限。每跳声明账户问答及保存密码能力，界面才可提交独立的 `password-interactive`；未选择时仍为保存方法，私钥和Agent类型不能转换。

中间跳的 `PasswordInteractiveForwardSink` 在密码明文借用释放后运行两轮MFA，再打开冻结下跳；叶终端共用部分成功门禁和问答，保持外观冻结、父链所有权及原退出屏障。六项密码授权回归、79项相关前端回归、类型／双语及完整生产全目标严格Clippy通过。

正式原生任务 `mise run probe-workspace-route-password-interactive` 已接入CI，验证冷暖全联合、刷新不重认证、中间跳密码完整成功拒绝、叶跳取消父链排空、原成功会话保留及来源不变。6项原生检查通过：冷暖两次三跳成功、四条实际输入、20条连接和15条转发通道退出，GUI实际退出0；共享App SSH完整372项／35项文档测试和8项SSH适配器回归全部通过，企业／跨平台不计入本批。测试仍只在校验后的隔离服务端修正密码部分成功位，不改产品依赖。
