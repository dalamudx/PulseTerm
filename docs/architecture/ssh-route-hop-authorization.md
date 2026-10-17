# SSH 跳板逐跳来源与原始点击

逐跳批准承接 [联合来源](ssh-route-source.md)，已接入 [GUI 生产路由](ssh-production-route.md) 和 [CLI 保存路线](cli-ssh-jump-route.md)。普通直连准备仍拒绝配置跳板的入口；账号认证方式在每跳实际指纹确认中明确选择。

## 精确来源与用途

`SshRouteSource::hop_source(index)` 固定整条图、密码／私钥／Agent 三目录、Vault 密文、原 trust，以及当前入口、身份类型和 ID。越界、同名替代、跨入口或类型混用拒绝。metadata 选择本身不授权解密、签名、实际 peer 或执行。

密码及私钥使用在既有 SQL 借用和复核边界重读完整路线，包含未选择的 Agent 元数据和墓碑。Agent 的精确来源复核、原页／epoch、peer 绑定和独立签名守卫继续负责单钥使用；失败不尝试其他钥匙或认证类型。来源复核是有界确认，不持有整个网络操作的数据库锁，不承诺 SQL 与网络原子提交。

中间角色只能申请冻结的下一目标的 direct-tcpip 通道；叶角色才能申请终端。Agent 中间角色的终端请求在签名前拒绝。协议清理、底层 stream 退出和信用保留由独立 supervisor 负责，撤权不等同资源退出。

首次 pin 写入限定当前精确入口，提交前撤权回滚。`after_first_pin` 只接受这个 hop 的 opaque 回执：修订推进一次，仅增加该地址／端口的一条非墓碑 pin，保留原 pin 的 ID、时间、HLC 与墓碑及所有非 trust 来源。错误 hop、重复回执、替换旧 pin 或夹带目录／图／Vault 变化均拒绝。实际公钥解析及 peer 的明确批准位于上层适配器。

## 原点击与退出

`SavedSessionGesture` 或 `AgentSessionGesture` 的 `select_route` 必须匹配点击时的最终入口和身份；`RoutedSessionGesture` 保留原 owner、PageScope、Vault epoch 和 30 秒有效期，不重新捕获或延长。`RouteHopSelection` 为密码、私钥或 Agent 的独立 typed 动作；`take_next_selection` 从外到内单次发行。锁定或相同 DEK 重新解锁均使旧路由和已发行手势失效。

`cancel` 和 Drop 撤销已发行动作及 native guard。根手势由 supervisor 保留到派生 I/O 和协议回收实际退出；取消等待不释放活跃信用。首次 pin 只能推进来源，不能改变原页面、epoch 或期限。

## 逐跳交互认证

账号身份在当前跳的实际指纹确认中明确选择保存密码或 MFA；私钥身份可明确选择保存私钥或“私钥 + MFA”，Agent 保留原方式。任何失败不自动回退，也不修改保存配置。`RouteHopSelection` 消费原类型的捕获并转为独立的交互或私钥联合动作，保留原页、epoch、来源、角色及路线原截止。账号不能转换成私钥联合；普通私钥会话意图不能升级成联合意图。`prepare_route` 与联合 forward 标记独立于直连准备，直连入口继续拒绝路由来源。中间跳的动作只能转发冻结的下一目标，申请终端在适配器调用前拒绝；叶跳拒绝转发。账号 MFA 不借用保存密码，私钥联合只在实际签名部分成功且服务器声明 keyboard-interactive 后进入问题。

问题在当前实际 peer 固定后通过原 route token 回传；前端只接受已选 MFA 或私钥联合的当前 hop、一次性 round 和原请求，关闭或取消结束回答等待并清空输入。回答只经受限 IPC 暂存，不记录或持久化。GUI 与原窗口批准后的普通 CLI 保存路线共用此协议；CLI 的全局 `--interactive`／`--key-interactive` 标志仍限定直连，不能替代逐跳选择。取消沿用原 route supervisor，容量保留到叶及父连接实际退出。

`probe-workspace-route-interactive` 已在隔离私库、生产 Linux WebView 和三个真实 loopback SSH 服务上通过九段验证：冷启动 MFA／保存密码／MFA 混合路线、暖启动全 MFA，两轮回答、终端输入及刷新不重认证，逐层 Esc 取消，错误叶回答无回退，精确首次 pin、保存配置／密文不变，以及正常关闭后每个父连接、转发通道和叶连接全部退出。结果保存在 `target/probes/route-interactive-result.json`，正式汇总见 `target/probes/ssh-route-interactive-development/receipt.json`。受控服务和软件渲染不建立外部企业 MFA、系统 IME、完整辅助技术或跨平台验收。

`probe-workspace-route-key-interactive` 已通过十五段生产 WebView／真实三跳 SSH 验证：冷混合与暖全私钥联合、两轮问答及刷新，普通部分成功不自动升级、第一因素拒绝与仅私钥成功均不满足联合策略，逐层取消／错误回答无回退，等待问题时私钥来源变化零回答，精确 pin、保存目录／密文不变，以及每层父连接、转发通道和全部叶流正常退出。指纹探测可能产生额外短连接，按逐层打开／退出余额确认回收，不以固定跳数推断连接数。受控服务器仅隔离修正 SDK 服务端 partial-success 写包并归档来源哈希，生产依赖不改。原文结果为 `target/probes/route-key-interactive-result.json`，正式汇总为 `target/probes/ssh-route-private-key-interactive-development/receipt.json`；上述外部网络与跨平台限制继续有效。

原点击 `read_route` 和桌面 `ProfileReader::ssh_route` 已复用原 I/O 信用与退出栏，在打开数据库前、排队／读取与结果发布时复查同页、原 epoch、期限和最终叶选择。metadata 谓词不能作为凭据使用的 native 权限。生产 Bank 的每跳 challenge 单次且绑定原页，driver 在认证前显示真实 peer、目标和身份并要求明确确认。

## 验证与证据

历史前置验证包括精确 hop／类型／ID／实例、私钥同修订替换、首次 pin 撤权与精确推进、取消／Drop、guard 保持 Busy、同 DEK epoch 拒绝及原页 SQL 信用。读取批九份证据保存在 `target/probes/ssh-route-original-read-acceptance.json`，该批清理 7.6 GiB；其他来源／授权批见最新计划。部分 Astrolabe 请求超时，未计通过。

后续真实双跳密码、Ed25519 中间跳／密码叶，以及 Agent 中间跳／Agent 叶的 Linux WebView 已通过逐跳批准、取消零认证、精确 pin、终端输入、刷新、独立重开及叶／父资源退出。新增 Agent App 反例确认中间用途不能转成终端、取消守卫仍占槽、重新解锁拒绝后续 hop。CI 同时登记 Store `ssh-trust`、App `ssh-use` 和两项生产 WebView 任务。该验收限本地真实 SSH 与 Linux GUI，不计 CLI、跨平台、硬件令牌或外部网络。
