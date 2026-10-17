# Host 与 Identity 搜索及批量删除

三类目录分别复用原有元数据编辑器、MetadataCaptures 和私库 worker。共享搜索列表只展示名称、用户名及主机地址等非敏感元数据；搜索不读取凭据、不连接服务器。现有单项编辑保持可用，列表的编辑动作使用原目录中的 UUID。

批量意图 `remove_many` 含 1–256 个唯一规范 UUIDv7，不接受路径、任意程序、凭据或启动选项。前端验证并复制 ID 数组；后端独立校验形状、唯一性、上限和来源。点击删除先列出全部目标名称，明确删除后果；取消没有保存调用。搜索变更清除全部选择和确认，包括移出结果的项。确认／行重建各使用本地世代，取消后再确认的旧按钮、旧行、旧编辑器和旧页面晚事件均无效。busy 禁止重复提交，失败清空不确定来源并要求重新读取，不自动重试。

批准沿用原页面、窗口 owner 和一次性目录快照票据。存储在一个事务中核对 owner、完整目录 CAS、全部目标和额度；全部成立才删除，并只推进一次目录修订。撤权在原提交门再次检查，任一目标缺失或重复、墓碑额度不足、旧快照、最终撤权都整批回滚。未变更 private worker 的 I/O 容量和退出等待规则。

Host 沿用当前 `app_state` 的版本化主机目录格式，仅删除对应端点元数据；这一步没有迁移 Host 到可同步的独立实体模型。密码和私钥 Identity 在正式实体表中保留 created_at、ID、同一 HLC 删除戳及 tombstone；私钥与 passphrase 引用一并墓碑化，Vault 与已保存密文保持不变。删除元数据不授予 plaintext-read／secret-use，不关闭或重建已运行会话。

公开 `mise run probe-workspace-catalogue-batch` 使用 `/data/temp` 私有夹具，在一次三目录实窗流程中核对搜索／取消零写、删除两项的一次修订、两个身份表的墓碑及相同 HLC、Vault／密文逻辑内容不变、刷新、独立应用进程重开和零终端启动。独立 SQL 收据只读；fixture 的固定无凭据元数据不能作为密码／私钥认证或跨平台验收。Linux 限定实窗已通过，结果 `target/probes/catalogue-batch-result.json`；两次应用进程正常退出。前一次非零退出未定位且复验未复现，失败日志单独保留，不声称退出根因已消除。三项 SQLite 反例、40 项受影响编辑器/UI测试、后台 wire 反例、TS检查和 Store/组合桌面 Clippy 已通过；Rust LSP Exact，TS LSP Unknown。

分组／标签与收藏已接入独立 [组织模型](catalogue-organization.md)，主机树／列表及Saved Host表单预选已接通，Linux联合实窗已通过限定验收；本机最近连接已接入独立[成功注册记录](recent-saved-hosts.md)，Linux限定实窗已通过。此批的批量操作仅为元数据删除，不能作为批量远程执行、分组继承或完整主机模型交付。
