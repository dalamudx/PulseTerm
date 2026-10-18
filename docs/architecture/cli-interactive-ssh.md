# CLI SSH 交互认证

`pulse ssh <保存的入口 ID> --identity <保存的账号 ID> --interactive` 明确选择账号 keyboard-interactive／MFA；`--key-interactive` 选择保存私钥与 MFA，`--agent-interactive` 选择保存 Agent 身份与 MFA。三个标志互斥。省略标志继续使用既有保存身份分派，不自动尝试 MFA。命令只接受保存的目标与身份 ID，不接受原始地址、口令、远端命令或自动解锁。原生参数校验只接受封闭的方法值，拒绝未知值和显式 null。命令链沿用八步／64 argv 的整体预检与逐步确认。

三个 CLI 交互标志仅用于保存主机直连：账号 MFA 只用账号元数据，私钥联合必须匹配保存私钥来源，Agent 联合必须匹配保存算法、指纹与用户名。配置跳板、别名或不匹配身份均拒绝。普通保存路线仍走原逐跳批准；GUI 已独立提供临时目标私钥联合及保存路线逐跳私钥联合，不能借 CLI 直连标志绕过保存路线。Agent 临时目标和逐跳联合仍未交付。

暖启动消费最初请求捕获的 `SavedSessionGesture`，收紧为独立 `InteractiveSessionGesture`、`PrivateKeyInteractiveSessionGesture` 或 `AgentInteractiveSessionGesture`，保留原页、Vault epoch、截止和取消条件，不重新捕获后续解锁。转换不读取密码、私钥或 passphrase，也不发现 Agent 或签名。Agent 的普通专用手势没有联合转换入口。冷启动先保留无 epoch 的原页元数据意图；原窗口解锁后仍需单独查看目标，再批准明确方法的精确目标。批准银行消费原意图、捕获当次 epoch，并保留原截止与取消条件。后续锁定、换页、源变更或过期继续拒绝，不延长授权期限。

实际 peer 指纹固定后才发行问题。服务器文字作为序列化数据通过 `emit_to` 定向发送原窗口，绑定原批准 ticket、原生启动 token 和一次性 round，不嵌入脚本。监听在原目标批准前安装；未批准、跨 ticket／token、重复／并发轮次、超过八轮和旧页面数据均不进入回答链。应用标题与操作跟随语言，服务器问题保持原文且按回显标志选择普通／隐藏输入；回答只经既有受限 IPC 提交，提交、取消和关闭后清除，不进入配置或日志。

Esc、CLI 断开、退页或截止会撤销原请求。显式取消通知只携带 key-free 的取消状态，CLI 可及时返回拒绝；银行拥有的启动任务仍持有原 token 和 I/O permit，取消等待不能释放未退出的 native 容量。正常启动完成不发送显式取消通知。Root 已收养的会话独立于 CLI 等待者存活，刷新恢复原会话而不是重新认证。

`mise run probe-workspace-cli-interactive` 使用隔离私库、生产 `pulse`／WebView、真实两轮 russh 服务及自有窗口辅助树，覆盖锁定／目标／问题取消、暖 CLI 断开、冷暖明确批准、两轮回答、刷新前后 peer 收到输入、配置来源不变和窗口／native／端点退出。只有整个验证链成功才写 `target/probes/cli-interactive-result.json`。Linux CI 已接同一任务；结果以该收据与验证记录为准。软件渲染与受控本地服务不建立外部 PAM／企业 MFA、真实多因素设备、辅助技术完整兼容、跨平台或性能验收。


`mise run probe-workspace-cli-key-interactive` 复用相同冷暖原窗口监督与取消反例，选择保存 `GUI_KEY_IDENTITY` 和独立 `--key-interactive` 方法。私钥部分成功必须来自同一实际连接，且服务声明继续 keyboard-interactive；服务器修正和来源校验沿用[私钥联合认证](ssh-private-key-interactive.md)。独立服务器证据写到 `target/probes/cli-key-interactive-test-server.json`，不覆盖前批来源记录。冷暖两轮认证、锁定／目标／问题取消、CLI 断开、刷新后输入恢复及四条 native 连接排空已由生产客户端和 WebView 执行，完整结果见 `target/probes/cli-key-interactive-result.json`。受控 loopback 与软件渲染不建立外部企业认证或跨平台验收。


`mise run probe-workspace-cli-agent-interactive` 使用保存 `GUI_AGENT_IDENTITY`，仅调用保存指纹对应公钥的签名。原窗口的锁定和目标确认都显示 Agent + MFA；只有同一连接接受实际签名并返回部分成功，才进入两轮问题。结果写 `target/probes/cli-agent-interactive-result.json`，独立服务端来源写 `target/probes/cli-agent-interactive-test-server.json`。收据记录四次所选签名、两次成功、两次取消及四条物理连接退出；未选公钥不得签名，失败不改用密码或其他钥匙。
