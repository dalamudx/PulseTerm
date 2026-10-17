# 生产 SSH 跳板链

原连接表单读取 Host v4 的 `via` 元数据；旧 v1–v3 目录仍解码为无跳板。只有明确配置了跳板的保存入口才进入路由 RPC。临时目标和 CLI 仍走原保存 ID／单目标链，不能把机器 ID 或临时目标升级为路由入口。

路由 Bank 复用原 GUI／CLI 启动槽，每个票据固定原 PageScope、Vault epoch 和最终入口／身份。后台读取联合来源后按最外跳到叶入口工作；页面显示精确目标、用户名、身份及实际 peer 指纹，每跳需要一个单次 challenge 的明确回答。错误／跨页／重复回答拒绝，不消耗仍合法的 challenge。丢弃 wait、取消、页面退场和期限到达均撤权；driver 独立退出后才释放启动槽。

中间跳由专门的密码／私钥 `authenticate_forward` 动作借用凭据，来源必须有冻结下一跳。sink 只允许该目标的 direct-tcpip，不能请求 PTY、shell 或 exec。普通 auth-only 结果不能升级为转发。首次指纹观察连接退出后，单次 SQL 确认只保存该跳的精确公钥；后续实际认证连接严格匹配保存的 key，重新握手不继承不匹配 peer 的授权。已有指纹的实际连接也显示给用户确认。每次凭据借用在同一 SQL 事务重读整条路由、两类身份、Vault 和 trust。

最终跳调用专门的 route-terminal 动作，拒绝普通直连来源和非叶跳。原 protocol reservation 在最终认证及终端创建前保留。只有成功 Root 注册的叶会话写入最近连接；中间跳不出现在会话目录。最终 actor 的 native join 持有父连接尾部，叶 SDK 与流退出后从内到外关闭父连接。所有失败分支继续回收；不能取得确证时保留 native exit barrier。

旧直接启动手势继续拒绝配置了跳板的保存入口。Agent 身份、Agent 转发、连接池、HTTP／SOCKS 与跳板的混合生产路由、跨平台及外部设备兼容尚未交付。

## 本批验证状态

真实本地两级中间转发＋叶 SSH 已通过密码链和 Ed25519 中间跳／密码叶链：首跳取消零认证、三跳首次 pins、精确批准顺序、中间零 PTY／shell、叶终端注册及父连接实际退出。原页 Bank 的跨页／错误 challenge／重复回答、wait 取消与 worker 退出前容量保留已通过。55 项协议及直接受影响前端检查、两项模态焦点／取消、四项原始 epoch 回归已通过。生产构造失败的确证回收修正后正在续验混合链和严格 Clippy；唯一 Linux WebView 实窗尚未通过。新 driver Rust LSP 请求超时，不计通过。此阶段不计完整 SSH-03／NET-01 或 Alpha 完成。
