# 私钥与 keyboard-interactive 联合认证

GUI 直连保存主机和原页临时目标提供明确的“私钥 + MFA”方式；原点击、实际部分成功门禁、两轮问答、终端恢复和七条连接排空已通过十段 Linux 生产 WebView／真实 loopback SSH 合成。CLI 通过独立 `--key-interactive` 方法、原窗口明确批准和原捕获收紧接入同一联合策略，详见 [CLI 交互认证](cli-interactive-ssh.md)。保存路线在每跳实际指纹确认中明确选择私钥联合，保留原路线角色与截止；十五段生产 WebView／真实三跳 SSH 已覆盖混合／全联合、逐层取消／错答、来源变化零回答、终端恢复及全部连接／通道退出，见 [逐跳授权](ssh-route-hop-authorization.md)。Agent／密码联合及外部企业 MFA 继续独立验收。

## 原点击和实际连接

`PrivateKeyInteractiveSessionGesture` 从原页、Vault epoch 和选择批准创建独立的一次性意图。它不能从普通私钥会话意图升级。原始五分钟截止在明确联合动作创建时固定；普通私钥动作的截止继续由原路径管理。准备时消费原来源、目标和身份捕获；失败、重新解锁或重新读取目录不能赋予原点击新权限。


CLI 的 `SavedSessionGesture` 只持有保存身份的原始元数据批准捕获；`into_key_interactive` 消费这一捕获并要求精确的保存私钥来源，保留原页、epoch、原截止和取消条件，不重新捕获后续解锁。它不接收或升级普通私钥会话意图。冷启动在独立查看目标时才确认原元数据意图并捕获当次 epoch，随后也不能延长原截止。

保存主机与临时目标共用同一直连准备和启动链。临时句柄须属于原页且仍存活。配置跳板的目标拒绝走联合直连，不能绕过保存路线；路由联合仅允许 `RouteHopSelection::PrivateKey` 消费原捕获，并由 `prepare_route` 保留原角色。中间跳不能申请终端，叶跳不能申请转发，且路线原截止不能替换成直连的五分钟截止。

同一受监督物理连接先完成实际 Host 指纹固定，再提交选中私钥。只有 SDK 返回 `Failure { partial_success: true, remaining_methods: ...KeyboardInteractive... }` 时才进入 MFA。普通拒绝、部分成功但未声明该方法、空方法集合和仅私钥完整成功均拒绝联合策略；普通私钥认证也不会自动进入 MFA。

## 问答、来源与回收

第二因素复用既有有界 broker、原请求 token 和单次轮次协议。每轮回答发送前重新验证原来源／实际连接与原页／epoch；认证后仍经原 Root 注册、PTY、设置冻结与终端 ACK 授权。任一因素失败均不能注册会话。

名称、说明和问题以不可信字面文本处理。应用文案支持中英文，切换不重建输入或提交回答；回显／隐藏、回答预算、清空和不落盘约束沿用 [交互认证](ssh-interactive-session.md)。回答不用于保存身份或连接。临时连接成功后，暂不保存和明确保存仍使用原独立票据。

私钥解析 CPU 许可交给实际解析任务并在任务退出时释放。MFA 等待继续占用原启动单槽和传输回收屏障；取消先撤销网络读写，再释放 SDK 本地等待，等真实 SDK／物理 I/O 退出后才能释放容量。撤销或等待超时不是回收完成。

## 验证服务器的协议修正

锁定 russh 0.63.3 的服务器实现会在 signed-public-key handler 返回后把 `partial_success` 无条件重置为 `false`。`scripts/build-key-interactive-fixture.py` 校验本仓 Cargo.lock 中原始归档摘要，在 `/data/temp` 隔离解压，并只修正测试服务器这一处写包行为。普通拒绝仍清除该位。

测试服务器在隔离工作区离线构建；原仓锁文件、Cargo registry 缓存和生产 SDK 不修改。原始与修正源码摘要、归档和测试二进制 SHA-256 写入 `target/probes/key-interactive-test-server.json`。构建目录在退出时清理，验证任务持有的可执行文件由该任务清理。

`mise run probe-workspace-key-interactive` 用生产客户端与这个真实 loopback SSH 服务验证拒绝门禁、两个因素、取消、终端恢复及原生排空。这些证据仅覆盖合成服务；外部 PAM／企业认证和 Windows／macOS 单独验收。最终实窗结果和非零直接测试计数以当批收据为准。
