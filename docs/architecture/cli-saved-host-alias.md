# CLI 保存 SSH 主机别名

`pulse open --host "入口名称"` 定位已经保存的 SSH 入口，现有 `pulse open <本地 Profile 名>` 语义保持独立。名称按原文精确匹配，区分大小写，不裁剪、不模糊匹配、不作为网络地址使用；有界文本采用保存入口的名称规则。无匹配或多个活动同名条目时拒绝，不选择机器的默认入口。

解析只使用一次来源读取得到的联合快照，包括入口图和密码、私钥、Agent 三种身份目录。选中的入口必须有有效的已保存 Profile 默认身份，恰好存在于一份活动身份目录且用户名匹配。缺少默认、墓碑、重复身份、同名入口和外来／临时／跳板派生来源拒绝；不自动迁移旧数据库、保存配置、挑选钥匙或回退另一种身份。结果仍以精确 entry ID 与 identity ID 进入既有来源 CAS、实际 peer 批准和连接链。

暖请求在原窗口捕获最初的 page、Vault epoch、取消条件与截止，再做受监督来源读取；解析后只能把同一 epoch 转入选中身份的 SavedSessionGesture。重新解锁、换页、取消和过期均拒绝。冷请求先发行无 epoch 的 ColdSavedAliasIntent，只登记原页、名称和有界期限；打开 Vault 或解锁本身不确认。原页独立点击查看目标时，批准银行同步捕获该次 epoch，再读取与解析。之后仍须原页精确目标批准和逐跳实际指纹批准。

Capability 的 workspace.ssh 参数只能是闭合的 host/identity ID 对，或单独 alias 字段。两种闭合候选都有独立生成 schema，根对象声明已知属性并拒绝额外字段；alias 不能混入 ID、地址或前端确认标记。类型化语义检查继续验证名称的 UTF-8 字节预算与不可见字符。元数据批准和 CLI 认证不授予网络、解密、签名或终端输入权限。

CLI 成功仍等待 Root 注册及原页会话选择 ACK。取消等待不会提前释放 SQL、SDK 或 native 信用，已注册会话由 Root 独立持有。密码、私钥、Agent 和跳板继续采用既有专用授权与回收链。

Linux 功能链验收与归档完成：同快照解析三项、真实 Vault 两项、Store/App 严格检查、25 项局部前端、16 项 CLI、11 项启动、独立精确目标批准和 Capability 注册反例通过。最终正常窗口管理器关闭的八模式矩阵通过：密码／Agent／三跳冷暖六场景，以及同名／缺默认身份零认证／零写入拒绝。精确默认身份、逐跳实际指纹与两条 SQL pin、所选签名、刷新原文不重拨、本轮 GUI／peer／父连接／端点退出已核验。默认 check/test/probe 通过（1050 Rust、1197 前端，7 忽略不计）；四 Rust 测试线程保留原断言。305 份 SHA 证据见 `target/probes/saved-alias-acceptance.json`，全退出后 clean-dev 回收 20.1 GiB，本批临时目录清空。历史锁定取消后 GUI SIGSEGV、非零退出及四次 GDB 正常关闭均保留，间歇原生关闭故障仍未解释，不能视为已修复；可选 GDB 只观测本任务 PID，不打印参数、局部值或内存。任务为 `mise run probe-workspace-saved-ssh-alias`。审计退出 5，8 项许可／2 项维护仍阻断；未计硬件 Agent、外网、多窗口、跨平台或发行通过。
