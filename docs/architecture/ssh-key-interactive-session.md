# 显式私钥 + MFA 直连

## 范围与方法选择

保存主机与原页临时目标的连接表单为已保存、凭据完整的私钥身份提供独立“私钥 + MFA（直连）”选项。默认私钥、密码、Agent 和单独 MFA 均不自动升级为联合认证。配置跳板的保存主机禁止此直连选项；GUI 分派与原生来源角色检查均拒绝绕过配置路线。CLI、跳板、Agent 与密码联合认证仍未开放。

## 原捕获与不可转换意图

`PageVaultOwner::begin_private_key_interactive_session_gesture_authorized` 在原点击捕获页面、Vault epoch、保存身份及目标 UUID、原授权和一次性截止。独立 `PrivateKeyInteractiveSessionGesture/Action/Intent` 不接受普通私钥意图转换，公开启动接口要求独立的可信 `PrivateKeyInteractiveSessionSink`。普通私钥仍使用原五秒期限；明确联合动作使用原点击起五分钟总期限，准备与每轮回答不能重新计时。

临时目标仅接收原页颁发的 UUID，后端按页解析临时句柄，读取相同的私钥元数据来源。临时句柄撤销、页面撤销、Vault epoch 变化、原动作取消、过期或源快照变化均使操作失效。私钥、可选口令和 MFA 回答不传递到页面或日志，不写入连接配置；私钥只借给受监督的认证适配器。

## 第一因素与第二因素

先按实际连接指纹通过原私钥源确认，执行既有有界私钥解析与签名。只有锁定 russh 返回 `AuthResult::Failure { partial_success: true, remaining_methods }` 且明确包含 `KeyboardInteractive`，可信联合适配器才调用既有 MFA 函数。普通拒绝、错误方法、空方法集合和仅私钥完全成功均拒绝该联合策略，不能创建 Root 会话；不会回退到密码、Agent 或其他私钥。

私钥协商等待后、开始 MFA 前及每轮答案发送前，继续调用原私钥源确认，重验源快照、原页、epoch 和实际连接。服务器问题通过数据 Channel 发送，按原 token 和一次性轮次处理；问题文本作为不可信文字显示。复用已有问题数、轮次数、答案大小及清空约束。第二因素成功后，再经过原来的终端准备、源确认、Root 注册和终端配置冻结。

## 取消和物理退出

与现有认证共用单一启动槽、Native IO permit、Root 启动票据和 supervisor。Esc、关闭问题、页面撤销、原窗口关闭或截止撤销当前请求；先撤销网络读写，再释放 SDK 的本地问题等待，不向远端发送空回答。等待者取消或权限失效不等于资源退出，permit 继续持有到 SDK、物理流及阻塞解析全部结束。成功注册后刷新仅恢复终端，不重新认证。

## 验证入口与边界

`mise run test-ssh-authentication` 包含独立捕获、原页/epoch/截止、临时目标准备前后撤销以及 SDK 部分成功的定点检查。`mise run probe-workspace-key-interactive` 使用自有 loopback 服务、固定加密 Ed25519 和生产 WebView，分别验证普通私钥部分成功、拒绝第一因素、仅私钥完全成功、Esc、错误回答、保存主机和临时目标成功、刷新及七条物理连接退出。脚本成功记录以实际收据为准。

真实外部企业 MFA、跨平台、CLI/跳板联合认证与其他联合方法不由本批夹具证明。软件 RSA 仍禁用。
