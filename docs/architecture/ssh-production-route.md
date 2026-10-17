# 原页面生产 SSH 跳板

生产路径接受保存的最终入口和用户明确选择的密码／私钥／Agent 身份。前端没有原始地址、套接字、转发目标或远端命令参数。`workspace_ssh_route_begin/wait/confirm/cancel` 只对原窗口当前页面开放；临时目标不进入路由。CLI 只发送保存的入口与身份 ID，由原窗口先批准目标，再把最初的联合 SQL 快照与原 PageScope／Vault epoch 交给同一银行；页面消费后端发行的 token，不重新调用 GUI begin。CLI 的逐跳实窗验收仍在进行。

一次点击捕获原 PageScope、Vault epoch 和有效期，通过受监督 `ProfileReader` 读取整条链联合来源。路由银行和密码／私钥／Agent 启动银行共享容量。每跳的目标、用户名、身份名称、实际算法和 SHA256 指纹显示在原连接表单，用户逐一批准。Challenge 单次且匹配原页，错误或重复回答不会转授新权限；等待不会捕获后来的解锁 epoch。

首次指纹以实际观察 key 提出批准，在原联合来源和撤权守卫下保存，只有精确新增 pin 能推进来源。认证连接再次验证实际 peer。已保存 pin 仍要求明确使用身份；失配拒绝。中间跳只发行被冻结下一跳的 direct-tcpip 通道，不请求 PTY、shell、exec 或任意地址转发，认证失败没有身份类型回退。

最终跳使用独立路由叶 sink、原协议会话 Root 注册和最近连接记录。最终 owner 的 native join 保留路由根手势及父连接，叶 actor 与 SDK/流退出后才从内到外回收父连接。取消结果等待、换页和关闭窗口不会取消清理。不能确认 SDK 与底层流退出时继续持有共享启动信用及退出栏，sink 构造失败也移交可核验的传输清理。

Host 目录协议 v4 增加只读 jumpProfile；客户端继续严格解码 v1–v3。普通保存入口和临时入口维持原审批链，配置跳板的入口通过显式 route 分支进入，旧直连来源仍拒绝绕行。

针对性检查已通过真实双跳密码链、Ed25519 中间跳与密码叶、原页 Bank 的错误 challenge／重复批准／取消信用、原 epoch 四项回归，以及客户端、v4 目录投影和逐跳焦点／取消表单检查。`probe-workspace-ssh-route` 的真实 Linux WebView 完整通过：取消零写／零认证、实际逐跳身份／指纹批准、精确新增首次 pin、终端整行 `GUI_SSH_ECHO:R`、刷新不重拨、独立重开已提交的原入口与身份，以及叶与父资源回收和两次正常退出。39 份证据已 SHA 核验于 `target/probes/ssh-production-route-acceptance.json`。二进制供继续实施的 Agent 验证复用，清理待本链退出。随后 `probe-workspace-ssh-agent-route` 已通过密码外层、所选 Agent 中间跳与 Agent 叶的真实 Linux WebView 组合：两次所选 Ed25519 签名、逐跳实际描述与精确 SQL pin、终端输入、刷新不重认证、独立重开和叶／父连接退出。Agent 中间角色不能申请终端、取消仍占原生槽、同 DEK 重解锁拒绝后续 hop 的 App 反例亦通过。Agent 证据见 `target/probes/ssh-agent-acceptance.json`（归档链完成后核验）。该验收限 GUI／Linux／本地真实 SSH，不计跨平台、CLI 路由、外部网络、硬件令牌或性能；CLI 对跳板入口仍明确拒绝绕行。
