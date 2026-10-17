# 目录组织与主机树

`pulse-store::catalogue_organization` 在同一私库 worker 中维护 `catalogue.organization.v1` 组织文档。它独立于连接参数、Identity 与 Vault 内容；归组、标签和收藏只修改组织文档，不读凭据、不授权连接。当前 Host 仍是已有单 SSH Endpoint 目录，组织覆层不构成 Host 多 Endpoint 模型迁移。本机最近连接已接入独立[成功注册记录](recent-saved-hosts.md)，Linux限定实窗已通过；模糊排序已接入主机树、批量目录和组织筛选，共享连续优先／非连续匹配／多词过滤，并保留原保存ID。三项关键排序测试、组织筛选与窄Linux实窗通过；分组默认设置继承仍待实现。

分组、标签与成员引用各有后端生成 UUIDv7、createdAt、updatedAt、HLC 和 deletion tombstone；文档保存 node、clock 与单调 revision。标签无父级；分组最多八层，拒绝循环与悬空父级。每项引用以目录种类和保存 ID 区分，最多一个分组和十六个标签。限制为 256 个分组、256 个标签、1024 个成员引用和 1 MiB 文档，包含墓碑，不能靠删除规避额度。ID 不重用；名称拒绝控制和隐藏字符。

读快照同时捕获三类目录的修订号。保存使用原页面、WindowToken 与 MetadataCaptures 的单次票据，在一个 SQLite 事务中核对组织文档 CAS、三类来源原修订、当前项目存在性以及最终撤权；变更再还原的目录不能重新激活旧来源。一次保存仅推进一次组织修订，归组失败整批回滚。新标签与成员一起原子创建；已有标签复用 ID。分组改父级再次验证整棵树；有子分组不能删除，无子分组删除时成员移至未分组并保留分组墓碑。导航、页面退休和 workspace 退出撤销票据，worker/native credit 继续由原退出屏障保管到实际退出。

组织页复用请求/保存/取消世代，异步晚结果和旧表单不得重获权限。项目提交还会比较当前 select 的真实值与显示表单的目标，选择变化没有触发 change 时只刷新表单，不发送旧目标保存。分组编辑与删除确认也核对当前选择。

侧栏 `WorkspaceHostTree` 只消费已验证绘图投射，支持嵌套树、列表、名称/用户名/标签搜索与仅收藏。分组折叠、搜索和视图切换是当前页偏好；组织数据本身持久化。显示使用 textContent，原始标题不能变成 HTML。每次绘图替换推进世代，旧树按钮不能选择新来源；忙、尺寸转换、页面退出和打开模态时禁止主机选择。目录编辑后更新投射；组织保存后更新整棵树。

点击树项只传保存 Host ID 给已有 SSH 对话框，并重新读取保存目录。目标缺失或非本目录时拒绝，不回退到首项；读取和预选不启动连接。实际 Connect 继续走既有原页面/原 Vault epoch/实际 peer 指纹/身份确认和受监督 SSH 生命周期。

主机目录读取成功后，即使尚未保存可用凭据也允许选择目标，连接按钮继续由凭据条件控制。选择器通过 `aria-describedby` 关联屏幕阅读器说明，说明从当前 DOM 选项生成、随真实 change 更新，关闭时清空；不含凭据。Linux AT-SPI Description 已核对非默认第三项及组织树筛选后的预选，读取说明不修改选择。展开菜单的 Selection 状态只记录高亮，不能替代当前 DOM 提交值；刷新验证使用公开的“重新加载工作区”按钮。

定向证据包括两个 SQLite 用例（循环/深度/CAS/最终撤权/墓碑/重开与端点不变）、相关 Clippy、组织解码与 Saved Host 预选反例。公开 `mise run probe-workspace-organization` 使用 SSH-enabled 真实 Linux WebView和私有无凭据夹具，核对三类归组、两行标签、收藏筛选、取消零写、删除墓碑、树/列表与Host预选取消、刷新和独立进程重开、端点/Vault逻辑哈希不变及正常退出。AT-SPI选项收据和真实键输入分开处理，读回已选 Host 不修改选择。该联合实窗已通过 Linux 限定验收，结果与源哈希见 `target/probes/catalogue-organization-result.json`；刷新和独立进程重开、两个应用正常退场及 WebView 子进程回收均已核对。非默认第三主机的独立预选、随真实选择变化更新及关闭清空的可访问说明也已验证。归档见 `target/probes/catalogue-organization-acceptance.json`。本批不能视为认证、网络互通或跨平台通过；Rust LSP Exact，TypeScript LSP Unknown 未计诊断通过。
