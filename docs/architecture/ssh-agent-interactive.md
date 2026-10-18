# 明确选择的 Agent + MFA

保存主机和临时目标直连的身份选择器分别提供普通 Agent 与 Agent + MFA，默认仍为普通 Agent。CLI 保存直连通过 `pulse ssh <入口 ID> --identity <Agent 身份 ID> --agent-interactive` 独立选择联合认证。保存跳板路线在每跳实际指纹确认时，允许保存 Agent 明确选择普通 Agent 或 Agent + MFA；GUI 叶终端的明确联合选择进入原路线。临时目标联合已通过 Linux 生产 WebView／软件 Agent／真实 SSH 的取消、两轮认证、暂不保存、刷新与明确保存精确已有目标验收；密码联合继续开放，不能从其他意图继承权限。

直连联合认证使用独立的 AgentInteractiveSessionGesture、Action、Intent 和标记 sink，保留原点击的页、Vault epoch、保存 Host／Agent 身份及固定五分钟截止。保存路线的联合准备消费原跳捕获，保留原页、epoch、完整路线来源、用途、取消及原期限，不能按直连策略重新起算。普通 Agent 意图不能转换为联合意图，原 Agent 五秒准备／认证策略保持独立。

后端发现 socket，验证其 owner／peer uid，并仅按保存算法和完整 SHA256 指纹选择公钥。Agent 只接收该公钥的一次实际 SSH 签名请求，页面不接收 socket、私钥或通用签名权限。Agent 发现后及签名前、签名后、每轮回答发送前、最终注册前重新校验原来源与授权。数据库复核不承诺 SQL 与远端签名的原子提交。

只有真实公钥认证已使用所选签名、服务器返回 partial_success 且继续声明 keyboard-interactive 时，才允许同一连接进入 MFA。普通拒绝、仅公钥成功、缺少部分成功标志、未声明该方法均拒绝联合策略；失败不尝试其他 Agent 钥匙、保存密码、私钥或其他认证方法。普通 Agent 不会因服务器声明 MFA 而升级。

中间跳的联合意图只允许转发到已冻结的下一目标，不能创建中间终端；叶跳只允许注册原路线终端，不能改作转发。GUI 保存 Agent 联合选择与 CLI 普通保存路线共用逐跳方法批准；CLI 的 `--agent-interactive` 仍仅明确选择保存直连，不能绕过路线。
问答使用原 startup bank、interaction broker 和 request token，保留原页与 epoch。问题文本为不可信服务器数据，仅作文本呈现。每轮只能交付一次有界回答；提交、关闭、取消、换页及失效清空输入，回答不写数据库、不进入日志。MFA 答案和私钥的生命周期互不代替。

取消先撤销网络并释放本地问答等待，再等待 SDK 与物理流退出；启动容量及窗口退出屏障不会提前释放。只有两个因素都成功并经过原确认链，才由 Root 注册终端；刷新复用已登记会话，不重复认证。

GUI 验收任务为 `mise run probe-workspace-agent-interactive`，CLI 冷暖任务为 `mise run probe-workspace-cli-agent-interactive`。合成软件 UNIX-domain Agent 广告两个 Ed25519 公钥，仅允许选中钥匙签名；真实 SSH 服务覆盖原批准、部分成功及有界问答。CLI 另核对锁定／目标取消零签名、问题取消与等待者断连、冷暖两轮联合认证、刷新前后输入和四条物理连接退出。GUI 保留普通 Agent 不升级、第一因素拒绝、仅公钥成功、错误回答和来源变化反例。隔离测试服务器沿用记录来源哈希的 russh 服务端 partial_success 写包修正，生产客户端和锁定依赖不变。实际通过状态与最终收据以开发计划、验证记录为准；该任务不代表外部企业 MFA、硬件 Agent 或跨平台验收。
