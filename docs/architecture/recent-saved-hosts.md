# 本机最近保存主机

`pulse-store::recent_hosts` 保存最多32个最近成功注册的SSH目标，存储在同一私库SQL worker的 `app_state`，固定键为 `catalogue.recent-ssh-hosts.v1`。版本化JSON文本上限8192字节，每项仅含规范UUIDv7保存主机ID和后端Unix毫秒时间。该列表是本机MRU，不参与同步；没有独立远端实体或凭据。完整失败历史、时长、审计事件和历史清除入口另行实现。

密码和已存私钥、GUI与CLI共用的 `ssh_session_start::run` 在实际协议建立、原页及原Vault epoch最终授权、根会话成功adopt之后才记账。取消、指纹失败、身份失败、握手失败和注册失败不进入列表。记录按worker提交顺序置顶并去重，超过32项截断，时钟回拨不按墙钟重新排序。记录失败只报告不含目标和凭据的固定事件，不把已注册会话变为可重试启动失败，也不回退或关闭该会话。

记录由可信后端写入，没有页面append命令。SQL事务再次核对保存主机仍活跃，目标已删除则拒写；读取同事务核对当前保存目录并过滤已删除目标，不重写历史文档。删除再创建不会把旧ID升级成新目标。损坏、超限或未知字段必须返回失败，不当成空列表或自动覆盖。共享worker序列化写入；请求方放弃等待不取消成功注册后的记账，实际SQL退出才释放其上层请求生命周期信用。

只读 `workspace_recent_hosts` 复用当前页面、窗口owner与唯一原生读槽，SQL gate保留到实际退出，发布前再次核对原页面。不能通过旧nonce、来源身份或最近列表获得网络、指纹、Vault、身份使用权限。页面晚结果在原AbortSignal及nonce门失效；读取失败在最近视图明确显示，树与列表仍沿原来源显示。

侧栏最近视图从已验证组织Host元数据和MRU ID组合，按后端MRU顺序绘制名称、保存目标和最近时间，沿用文字搜索与收藏筛选。显示只用textContent。点击仍只把保存Host ID交给既有SSH表单，再读当前保存目录；选择后仍需显式Connect及原有实际peer、指纹和身份批准。初始加载、GUI成功和CLI成功显示回执后刷新最近投射；页面刷新与独立应用进程重开重新读取私库。

定向检查为两个SQLite/worker反例和三个前端投射/晚结果反例。公开 `mise run probe-workspace-recent-hosts` 使用固定russh peer和真实Linux WebView，计划一次密码认证并核对取消零写、成功一项、最近目标预选取消、原目录/Vault/trust内容哈希不变、公开刷新、独立重开与正常退场；结果已通过Linux限定验收，见 `target/probes/recent-host-result.json`／`recent-host-acceptance.json`（15份SHA证据、9.8GiB限定清理、临时根为空），两个应用正常退场且子进程回收；不计跨平台或OpenSSH互通验收。另修Vault关闭时先关闭modal再发布working(false)，避免树按钮按仍打开的modal残留disabled；九项受影响表单测试通过。实窗由独立只读SQLite收据核对，无页面注入批准或凭据。临时夹具限当前任务 `/data/temp` 私有根，WebView和peer进程组退出后才清理缓存。
