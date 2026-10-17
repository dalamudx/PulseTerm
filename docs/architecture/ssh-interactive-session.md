# SSH keyboard-interactive / MFA

SSH 连接选择器为已保存的账号元数据提供独立的“交互认证 / MFA”方式。账号没有保存密码时仍可选择此方式；初始不自动改用 MFA，也不尝试密码、私钥或 Agent 回退。直接连接与 CLI 显式 `--interactive` 沿用同一原页／实际 peer 链，冷暖捕获与取消边界见 [CLI SSH 交互认证](cli-interactive-ssh.md)。保存路线另在每一跳的实际指纹确认中明确选择认证方法，见 [逐跳授权](ssh-route-hop-authorization.md)；三跳混合与全 MFA、逐层取消、错误回答无回退及最终回收已通过九段 Linux 生产 WebView／真实 loopback SSH 验证。临时目标复用原页内存句柄和同一账号元数据认证链；问答成功只授权注册会话，保存连接仍需独立明确动作。完整 SSH 多种方法联合仍未交付。

直连“私钥 + MFA”使用独立原点击意图，并要求实际服务器的私钥部分成功和明确 keyboard-interactive 方法声明；实现边界与隔离测试服务器修正见 [私钥联合认证](ssh-private-key-interactive.md)。直连保存及临时目标的十段 Linux 生产 WebView／真实 loopback SSH 合成已通过；CLI、跳板及其他方法联合继续排队。
`InteractiveSessionGesture` 在原窗口点击时捕获 Page 和 Vault epoch，独立消费选择批准。它使用账号元数据和源快照，不读取、借用或解密任何保存的密码。准备与提交继续验证当前页、epoch、账号／目标和源事务；元数据变化或后续解锁不能替代原点击的权限。

实际连接必须先匹配已经核对的 Host 指纹。`InteractiveSessionSink` 在同一受监督连接上调用 keyboard-interactive；每轮绑定单次 UUID、原请求令牌和当前页。重复、并行、过期轮次、错令牌、错回答数量及已撤销 guard 均不能消费待回答操作。回答提交前重新核对保存源；认证成功仍通过既有 Root 注册、PTY、配色／行为冻结和终端 ACK 授权。

每次尝试最多八轮、每轮最多八题。名称最多 1024 UTF-8 字节，说明 4096 字节，每题 1024 字节且题目总量最多 8192 字节；每项回答最多 4096 字节且每轮最多 16384 字节，拒绝 NUL。前端有单次五分钟截止；后端原动作、页、epoch 与传输截止继续独立约束。未知／超限问题在创建输入或提交回答前拒绝。

服务器名称、说明和问题均为不可信字面文本，不进入 HTML 或翻译目录。输入按服务器 echo 标志显示或隐藏，关闭自动补全与拼写检查。中英文切换只更新应用文案，保留同一输入、值及焦点，不提交回答。明确提交、Esc／取消、页面撤销或失败移除表单并清空输入。原生回答使用 Zeroizing，不记录、持久化或放入状态事件；JS、IPC 和上游 SDK 内部复制不提供完整内存擦除承诺。

取消沿原单槽 supervisor 排空，并等待 SDK 与物理 I/O 完成后释放容量。锁定 russh 在认证回答等待点可能忽略 Disconnect；`PinnedTransport::close_and_join` 先撤销有锁序保护的网络读写，再仅向本地 SDK 队列释放等待，之后仍 join SDK 和 native stream。这个本地释放不会发送空回答；真实服务断言取消／源变更后收到零回答，且原生连接计数归零。没有把撤销或 observer 超时当成回收完成。

`mise run test-ssh-authentication` 覆盖原页／epoch／元数据与 CLI capture 转换、Ed25519／NIST ECDSA 格式、broker／真实 loopback SSH、CLI EOF 与退出屏障、显式方法参数及单槽取消容量。真实 SSH 检查覆盖两轮接受与拒绝、无认证回退、取消／源变更、错误指纹、问题／轮次预算和物理退出。另有普通 peer 关闭回归及 Chrome 组件检查；非零执行与实际计数以当批收据为准。

`mise run probe-workspace-ssh-interactive` 复用生产前端、真实 Tauri 命令、隔离私库和真实 russh 服务。Linux 五阶段合成检查验证无保存密码账号明确选择 MFA、解锁不授权 SSH、第一轮 Esc 不认证且连接退出、两轮明确回答、终端输入／刷新及关窗排空；收据 `target/probes/workspace-ssh-interactive-result.json` 记录一次认证与两条原生连接退出。夹具仅用于受控 loopback 验证，不等于外部 PAM／企业 MFA、系统 IME、辅助技术或 macOS／Windows 验收。发送器以普通回调注入 broker，Tauri IPC 留在生产命令层；headless 探针不依赖 Tauri。

实窗控制器在退出后归档固定状态收据，不记录挑战回答。临时私库、套接字、辅助程序和缓存均位于 `/data/temp` 的独立目录，先停止自有 app／peer 再清理；其他构建任务并行时可注入本轮冻结的 fixture 可执行文件，避免默认 xtask 构建覆盖探针入口。
