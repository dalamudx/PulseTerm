# SSH 应用信任记录本地持久化

## 已验边界

`pulse-store` 默认实现已推进到 schema 5（本批完整兼容验收进行中）；信任数据继续使用 schema 4 的 `known_hosts`／`known_hosts_state`，不回退 `app_state/ssh.trusted_hosts.v1`。读写明确支持 4／5，future6 拒绝。信任数据不是 Vault 密文或连接权限；公开读写仍由 `ssh-trust` feature 控制，存储层不依赖 `pulse-ssh`／`russh`。

每个目标使用规范化 host/port 唯一键，持久化后端 UUIDv7、正整数 `created_at`、有界规范 HLC 与 `deleted`。目录保存独立 revision、稳定节点及 HLC；HLC 后退时使用逻辑计数推进，溢出拒绝。删除保留 ID、创建时间和新 HLC，key 清空为 NULL；旧快照不能复活。显式批准后的 pin 可恢复同目标墓碑，保留实体 ID、推进 HLC，但不能因此授予认证或 SSH 会话权限。同步传输、远端合并与同步后的信任批准尚未实现。

## 来源、预算和事务

- 最多 128 活跃 pin、256 删除墓碑。所有行的 host/key 原始字节合计至多 64KiB，活跃记录还要通过原有 64KiB 规范文档预算；单 key 8192 字节。行查询最多取 385 条，取字符串前检查长度和合计；身份、HLC、目录、ordinal、目标和行形状损坏均拒绝整个来源。
- 快照绑定 StateStore 实例。普通 edit 比较 revision、时钟、全部行元数据与活跃目标/key；首次批准在同一 IMMEDIATE 事务额外比较 host、identity、Vault 和完整 trust 来源，检查目标及最终原生授权。相同 revision 的外部 ID/时间/HLC 改动不能覆盖。
- `records()` 只交付活跃公开 key 的存储形状，不进行 SSH 公钥认证。上层适配器必须逐一解析，坏 key 不能回退到另一信任源。墓碑不参与有效 pin。
- 取消的请求不释放正在执行的 SQL 信用；提交前撤销回滚。已提交但丢失回执不承诺回滚，不自动重试。

## schema 1–3 升级

沿用私有文件准入及 SQLite Backup API，升级前保存 `.bak-4`，包含未 checkpoint WAL；既有备份拒绝覆盖。表创建、严格解析旧公开 pin 文档、保留顺序/revision、生成实体元数据、删除旧文档、读回和版本更新在同一迁移事务完成。旧格式已删除目标无法推导实体历史，不伪造墓碑；其空目录 revision 仍保留。坏 trust 会回滚同批 Vault/identity 升级步骤及版本，原数据不变，备份保留。schema 4 出现旧文档或缺失/坏正式表时拒绝，不回退。

## 验收与未交付

迁移/预算、UUID/HLC/墓碑/显式恢复、时钟耗尽、同 revision 元数据漂移和目录墓碑上限回滚已验证。完整存储、SSH、应用、工作区原生与默认前端回归，独立进程私库、实际 TCP 首次批准（auth=0）及独立受监督密码动作（auth=1）通过；实际 hosts CRUD/重开、Settings 字体、Vault 基础窗口兼容通过。默认桌面 normal 依赖树仍无 SSH 网络层。

存储验收自身不证明前端；显式 workspace-ssh 的当前页审批 IPC 与 Linux 独立 OpenSSH 实窗另已验，默认工厂仍关闭，见下方对端文档。未交付包括生产 SSH 会话、变更 key 替换、服务器证书、系统用户 SSH 文件导入、同步、Windows ACL、断电/文件系统故障或三平台验收。首次审批与实际对端边界见 [SSH 对端证明](ssh-peer-evidence.md)，进度见 [开发计划](../development/plan.md)。既有 6 项许可与 2 项维护公告仍阻断发布。
