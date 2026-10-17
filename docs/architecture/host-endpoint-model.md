# Host、Endpoint 与 SSH Profile

本批已接通独立机器／入口／SSH Profile 的领域与存储模型、v2当前页主机元数据投射、多SSH入口管理页及按机器分组的主机树。页面可明确迁移旧目录、向同一机器附加独立SSH入口及单独重命名机器；保存不启动连接。原保存 ID 保留为入口 ID，GUI／CLI继续使用既有端点批准、身份选择及实际peer指纹链。本批不计完整SSH Profile默认身份／参数配置或其他协议交付；Linux元数据实窗已通过，证据见 `target/probes/host-endpoint-acceptance.json`；不代表SSH认证、其他系统或其他协议验收。

`pulse-core::host_catalogue` 定义独立 Host、Endpoint、SshProfile 和 HostCatalogue，均为无 I/O 元数据。每个实体保留独立 UUIDv7、创建时间、由 HLC 确定的更新时间以及删除墓碑。Host 只有机器显示名称；Endpoint 引用 Host，保存显式协议及规范化地址／端口；SshProfile 引用特定 SSH Endpoint，并保存用户名和可选的复用 Identity ID，不包含凭据、文件路径或执行命令。缺失 Identity 不授予默认身份回退。

模型预留 SSH、SFTP、RDP、VNC、Telnet 独立协议，构造这些元数据不实现或开放相应 transport。SSH Profile 不能引用其他协议；不支持的入口不能隐式转换为 SSH。地址校验复用现有 DNS／IP／TCP 纯解析器，协议默认端口只是编辑器建议。

完整图限制 256 Host、1024 Endpoint、1024 SSH Profile，墓碑计入额度。全图拒绝跨类型重复 ID、悬空所有者、活跃入口引用已删 Host、活跃 Profile 引用已删或非 SSH 入口、将本图实体 ID 充作外部 Identity ID。外部 Identity 存在性、身份种类、当前 Vault epoch 和实际 peer 指纹由上层保存／连接适配器独立核对。

`pulse-store::host_catalogue` 在原Host保存事务内维护 `catalogue.host-endpoints.v1` STRICT TEXT文档。未迁移库的授权读返回None，不写库；第一次明确保存同时创建图和更新旧目录，最终撤权会回滚两者。已有图先校验旧目录的完整投射与修订，损坏或不一致均拒绝，不从旧目录自动覆盖损坏图。旧 v1 文档继续只读；明确写入使用 v2，Profile 的 `via` 字段必须存在，可为 null 或精确的另一个 Profile ID。v1 注入该字段、v2 缺失该字段、未知字段或版本均拒绝。跳板配置与生产路由边界见 [跳板配置](ssh-jump-preferences.md)。旧目录CAS增加完整记录比较，防止相同修订下的内容替换。旧目录删除将Endpoint/Profile及无其他入口的Host写为墓碑，身份不复活；更新保留独立ID。

多入口保存与旧目录投射在同一个 SQLite worker 事务内完成。原保存主机 ID 保留为兼容 Endpoint ID，新的 Host 和 Profile ID由后端生成，组织及最近列表继续引用同一入口ID，不按名称或用户名重映射。v2主机票据携带同一次SQL读取的完整目录及机器图；附加入口只允许引用当前图的活跃机器。机器重命名不修改入口名称、用户名、地址或端口；删除一个入口保留其他入口与机器，最后入口删除才产生机器墓碑。旧来源票据在修订推进后失效，关联迁移不改变已运行会话。

组织只读投射附带机器ID、名称与入口归属，侧栏在初次读取、保存与刷新后按机器分组；组织来源CAS还核对完整HostSnapshot，拒绝同修订下的图替换。两类投射不互换保存票据。列表、模糊搜索、最近列表及连接按钮始终选择入口ID，不回退到机器的默认入口。兼容目录仍限制256个当前SSH入口；core全图额度不是当前页面的入口额度。旧单入口保存、ssh_config导入、快速保存和批量删除统一经过同事务图维护；独立SSH配置名称、用户名与默认身份引用已接通，见[SSH配置边界](ssh-profile-preferences.md)；其他protocol写入及同一入口的多个可编辑Profile仍待后续迁移。

针对性检查已通过：两个领域反例、四项迁移／worker／双入口存储反例、六项受影响旧Host检查、15项主机协议／表单反例、四项组织协议及两项组织存储检查、TypeScript及组织Rust诊断。修订耗尽夹具在同事务中构造旧目录与图的一致max修订／墓碑，仍验证耗尽后零写。Store与workspace-ssh＋workspace-cli组合Clippy通过；主机适配器Rust诊断超时及TS LSP Unknown不计通过。单条真实WebView元数据窗口已通过：旧库授权读与取消零写、明确迁移推进一修订、同机器双入口、独立机器命名、单入口删除的墓碑与剩余机器保留、原入口ID预选取消、公开刷新、独立进程重开及两次正常退场。凭据／trust／组织／recent内容不变，未执行认证。按钮命名及探针选择参数失败分别保留；33份证据已SHA核验归档，限定清理完成10.9GiB，任务临时根为空。公开任务为 `mise run probe-workspace-host-endpoints`，Linux CI已注册；窗口验证明确迁移、附加、独立命名、入口墓碑、原目标预选取消、刷新、独立重开与正常退场，不重复认证或跨平台矩阵。
