# 快速地址到保存主机的当前边界

本批复用现有Host目录和单次同页CAS票据，在侧栏提供“快速连接”：输入地址与可选名称，核对目标，保存新主机或复用唯一匹配，再进入原Saved Host身份／指纹／连接批准表单。输入不会启动连接，保存也不会认证。原保存结果只有新Host ID可被交给连接表单，不传原始地址作为启动参数。

这是完整VAULT-05的前置交付；当前明确要求先保存或复用。临时目标的后台来源及两类身份复核已接通，见[临时目标来源](temporary-ssh-target.md)；页面临时入口、实际临时连接和成功后可选保存仍未交付，RDP地址未实现，不把本批计作VAULT-05全部完成。

## 输入与选择

接受 `user@host`、`user@host:port`、`ssh://user@host:port`、方括号IPv6；用户名明确提供，端口省略为22。DNS按现SshTarget标签规则检查并小写，IPv6预览由URL literal parser规范化，保存最终仍由后台SshEndpoint校验。拒绝密码userinfo、路径／query／fragment／zone、其他协议、隐藏控制字符、缺失用户名、非方括号IPv6与零或超限端口。预览纯本地元数据，既不解析ssh_config，也不连接或读取凭据。

唯一现存username＋target＋port匹配时复用其ID和原名称，零写入；同目标多条记录时拒绝自动选择，要求回主机列表。IPv6等价形式只用于元数据匹配，不产生trust或credential authority。新目标保存沿用原目录token和后端UUIDv7／CAS／原页最终授权。失败或不确定结果不自动重试、回滚已完成保存或打开连接。

## 页面与可访问性

输入或名称变更清除旧预览；提交时重新解析并与预览比较，变更需重新核对。读取／保存期间锁输入，继续按钮禁止重复提交。关闭、Escape、换页、signal abort撤销本表单；晚保存结果不能打开连接，即使实际I/O可能已提交，也必须重新读取。关闭先close modal后发布working(false)，仅原页恢复焦点。

目标用textContent展示；字段有明确label，预览和状态使用live region，Escape沿用工作区完整手势消费。新最近视图加入后，主机树三按钮改用“分组／列表／最近”短标签、完整aria-label及固定三列，避免窄侧栏逐字断行。

## 验证

8项定向测试覆盖输入形状、拒绝危险语法、IPv6等价／port／user精确匹配、取消零写、明确单次保存到原连接表单、唯一复用与重复拒绝、未派发input事件的预览更改、取消或换页后的晚保存。TypeScript与局部UI检查通过。TS LSP Unknown不计通过。

公开 `mise run probe-workspace-quick-connect` 与Linux CI已登记，复用隔离私库、固定peer的元数据夹具，不运行本批认证。独立quick-connect证据前缀保护已归档recent-host SHA。实窗仅检查invalid／取消零写、复用零写、IPv6一次修订保存、原连接表单预选与取消、source不含credential/trust改变、刷新和独立重开、零会话启动及正常退场；Linux窄实窗通过，8项关键反例及焦点修正后的5项表单测试通过，证据见 `target/probes/quick-fuzzy-acceptance.json`。本批不计真实网络、跨平台或完整快速连接验收。
