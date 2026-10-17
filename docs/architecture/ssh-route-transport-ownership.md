# SSH 路由传输所有权

`pulse-ssh::peer::RouteTransports` 是后台资源收集器，不是身份批准、转发能力或会话注册凭据。生产路由 driver 尚未接入；中间认证成功后的单次精确目标转发仍须独立实现。

## 收集和移交

后台传入保留的应用 runtime 和预定连接数，允许一至九条连接，与八跳加最终入口一致。按外到内收集当前有效的 `PinnedTransport`。超过预定数、移交之后再插入或 peer 已失效时，`RejectedRouteTransport::into_parts` 返还静态失败及原始资源，调用者必须继续监督其关闭，不能把拒绝视为资源退出。

只有完整收集后才能单次 `take_final`，只移动最后一条连接；它不证明任何认证、Profile 匹配或终端注册。driver 仍须用联合来源绑定逐跳目标，检查实际 peer，批准身份并成功认证后才能创建下一跳。最终连接由原有最终 sink／actor 关闭并确认退出后，才清理剩余中间连接。

## 退出和取消

所有外部 channel handle 必须先退出。`begin_close` 将整个收集器移入独立任务，从最内层逐条执行已有 `close_and_join`，确认 SDK 任务与底层流退出后再处理外层；某一条失败仍继续清理其他条目，并返回失败。结果等待者丢弃不会取消这个任务。调用者保留独立路由／启动容量直到取得已知结果，丢失监督器和运行时退出均不是成功退出。

收集器直接 Drop 只关闭 peer 证据，不提供 SDK／native 完成证明。未来生产 driver 必须将清理回执接入 root 退出屏障，不能通过 collection Drop 归还容量。

## 验证边界

两项针对性检查使用两条本地真实 TCP SSH 握手，覆盖界限拒绝的原连接返还、最终移交单次性，以及阻塞内层流析构时外层仍有效、丢弃清理等待者仍完成反向清理、native admission 直到流退出才释放。夹具没有发送认证请求，不请求通道、PTY 或 shell；它们不是嵌套 direct-tcpip、多跳生产连接或外部网络验收。去掉夹具的不必要 Option 后的两项重验、`managed-store` 全目标严格 Clippy 和受影响 Rust 格式检查均通过；资源源码 Rust Exact 无诊断。四份证据 SHA 已核验归档到 `target/probes/ssh-route-transports-acceptance.json`（含静默格式日志），构建与原生夹具退出后 `clean-dev` 清理 1.5 GiB，任务临时目录已确认不存在。
