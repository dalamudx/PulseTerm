# SSH Profile 跳板配置边界

本批交付跳板引用的模型、原子保存和原页面编辑，不交付生产跳板传输、SSH Agent 或自动回退直连。

`SshProfile::jump_profile_id` 引用另一个活跃 SSH Profile。全图校验拒绝自引用、环、悬空引用、跨类型 ID、墓碑引用及超过八个跳板。`HostCatalogue::ssh_jump_chain` 返回从最外层到目标前一跳的有界顺序。删除仍被活跃 Profile 依赖的跳板入口会回滚；先明确解除引用，再删除。

原 `catalogue.host-endpoints.v1` STRICT TEXT 键保持不变。v1 文档读不写库，且不能注入 `via`；明确保存写 v2，每个 Profile 必须带 `via`，其值为 null 或规范 UUIDv7。未知字段、版本、缺失字段和来源修订／完整内容变化均拒绝。普通旧 Profile 编辑保留现有跳板引用，不因重命名或用户名编辑丢失它。

`edit_ssh_profile_with_jump` 在同一个 SQLite worker 事务中更新名称、用户名、默认身份和跳板。原目录投射、图及修订一起提交，最终撤权或引用校验失败一起回滚。它仍使用含两类身份元数据的 `ProfileSnapshot`，不读取凭据或授予身份使用权限。

原页 `workspace_ssh_profiles` 投射 v2 并带 `jumpProfileId`。`workspace_ssh_profile_save` 的 v1 请求保留原引用；v2 请求必须显式给出 `via`，null 表示清除。原页票据单次消费，仍受 reader、WindowToken、页面、身份完整来源及最终提交授权约束。表单只列其他 Profile，拒绝旧 DOM、注入的自身／外来选项和关闭后的晚结果，保存一次完整配置。

生产路由尚未接通时，`HostSnapshot::supports_direct_ssh` 对配置跳板的入口返回 false。密码／私钥 Intent、保存身份手势及 GUI 指纹直连读取均在原步骤拒绝，CLI 也不能以保存入口绕过。页面明确说明设置后此入口暂不能连接。临时目标没有持久跳板配置，保持原来独立来源和批准链。

两项 core 图反例、一项共享 worker 生命周期及一个覆盖密码和私钥手势的 App 反例通过；完整配置失败回滚／解除引用、旧 v1 读不写与注入拒绝已包含在存储检查。九项协议／编辑器测试、native 请求精确解码反例、TypeScript、Python、App ssh-use 与组合桌面 Clippy 通过。Rust core 与原页解析器 LSP Exact 无诊断。零测试集成输出不计额外通过项。

`mise run probe-workspace-ssh-jump-config` 与 Linux CI 已登记。唯一 Linux 元数据实窗已通过：取消零写、精确跳板引用一次提交、机器／端点／身份／凭据／trust／组织不变、刷新与独立重开、明确解除及零认证，两次原生 drained／shutdown／正常退出均核验。重开画面确认显示已保存的跳板。通过真实 Host 编辑器创建第二 Profile 后捕获基线；目录键初始化、侧栏滚动和原生菜单吞键造成的探针失败日志／截图保留。最终使用真实键盘选项提交并由 SQL 核验，不把菜单 highlight 当 committed value。32 份证据归档在 `target/probes/ssh-jump-config-acceptance.json`，全部 SHA 核验；进程退出后 `clean-dev` 清理 11.9 GiB，临时目录为空。Linux 配置页限定验收完成，生产跳板传输仍未交付。

后续生产路由必须对每跳使用精确保存 Profile、身份与实际 peer 指纹，并保持原页面／来源撤销和中间连接的退出回收。不将已存在的跳板或 Agent 技术探针计为生产能力，不跳过逐跳批准，也不让直连自动替代失败的配置路径。
