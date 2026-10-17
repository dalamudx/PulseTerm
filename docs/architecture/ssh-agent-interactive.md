# 明确选择的 Agent + MFA

本批入口限 GUI 保存主机直连。在已有身份选择器中分别显示普通 Agent 与 Agent + MFA，默认仍为普通 Agent。临时目标和保存跳板路线禁用该联合选项；原生准备同时拒绝这些来源。CLI、逐跳、临时目标及密码联合认证不从本批意图继承权限。

联合认证使用独立的 AgentInteractiveSessionGesture、Action、Intent 和标记 sink。原点击捕获原页、Vault epoch、保存的 Host／Agent 身份及固定五分钟截止。准备、指纹确认、Agent 发现和问题轮次都不能延长期限，普通 Agent 意图不能转换为联合意图。原 Agent 五秒准备／认证策略保持独立。

后端发现 socket，验证其 owner／peer uid，并仅按保存算法和完整 SHA256 指纹选择公钥。Agent 只接收该公钥的一次实际 SSH 签名请求，页面不接收 socket、私钥或通用签名权限。Agent 发现后及签名前、签名后、每轮回答发送前、最终注册前重新校验原来源与授权。数据库复核不承诺 SQL 与远端签名的原子提交。

只有真实公钥认证已使用所选签名、服务器返回 partial_success 且继续声明 keyboard-interactive 时，才允许同一连接进入 MFA。普通拒绝、仅公钥成功、缺少部分成功标志、未声明该方法均拒绝联合策略；失败不尝试其他 Agent 钥匙、保存密码、私钥或其他认证方法。普通 Agent 不会因服务器声明 MFA 而升级。

问答使用原 startup bank、interaction broker 和 request token，保留原页与 epoch。问题文本为不可信服务器数据，仅作文本呈现。每轮只能交付一次有界回答；提交、关闭、取消、换页及失效清空输入，回答不写数据库、不进入日志。MFA 答案和私钥的生命周期互不代替。

取消先撤销网络并释放本地问答等待，再等待 SDK 与物理流退出；启动容量及窗口退出屏障不会提前释放。只有两个因素都成功并经过原确认链，才由 Root 注册终端；刷新复用已登记会话，不重复认证。

验收任务为 `mise run probe-workspace-agent-interactive`。合成软件 UNIX-domain Agent 广告两个 Ed25519 公钥，仅允许选中钥匙签名；真实 SSH 服务覆盖普通 Agent 不升级、第一因素拒绝、仅公钥成功、Esc、错误回答、第二轮等待期间来源变化、成功及刷新恢复，并比较配置、Agent 元数据和密文与物理退出计数。隔离测试服务器沿用记录来源哈希的 russh 服务端 partial_success 写包修正，生产客户端和锁定依赖不变。实际通过状态与最终收据以开发计划、验证记录为准；该任务不代表外部企业 MFA、硬件 Agent 或跨平台验收。
