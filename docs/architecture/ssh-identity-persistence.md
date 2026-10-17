# SSH 身份本地持久化边界

## 范围与布局

`pulse-store::ssh_identities` 与 `StateStore::{load_ssh_identities,edit_ssh_identities}` 是密码身份后端元数据接口。schema 3 引入 STRICT `identity_catalog`／`identities`；当前默认实现已推进到 schema 5，新增独立私钥身份表，已通过完整基线和 Linux 窗口兼容验收。存储层不依赖 `pulse-ssh`。工作区已有当前页 nonce／WindowToken 授权的密码身份编辑、接收-only密码保存，以及独立的 saved-password SSH 入口；元数据和保存回执本身不授予凭据消费或连接权限。私钥身份仍只在可信 Rust 后端开放，未注册页面私钥输入／认证命令。

身份行包含backend-generated UUIDv7、created_at、HLC、deleted墓碑、256-byte label／255-byte username、Vault ID／secret ID／ssh_password role及format1。`identity_catalog`保存稳定node、最后HLC及独立本地revision。身份Snapshot绑定StateStore实例（包括空状态），无Debug/serde；不把ID、HLC或revision当窗口/使用权限。

## 事务与引用

- 最大256个active／4096个tombstone，字段/行读取先在SQL限量，4MiB来源预算，整来源任一坏行、重复、错Vault／role／clock均拒绝，不reset或过滤掩盖。
- Read在同一Deferred transaction读取身份与Vault；Create/Update/Remove用Immediate transaction，比较精确身份元数据（不仅revision），由后台读取wall clock并分配HLC／ID，提交前重检native授权。返回同一事务的提交快照，不在commit后另读可能已推进的来源。
- HLC格式`millis:counter:node_uuid`最多67 ASCII bytes；node持久化，墙钟回退／相等时counter递增，overflow拒绝。修改保留created_at，删除推进stamp并保留ID；墓碑不能通过Update恢复。末项删除后revision/node/clock仍保留。仅本地stamp基础；未实现远端HLC merge、同步LWW或可信外部时钟。
- 可保存Vault中未设值的非nil引用；来值has_value一律忽略，读回从同事务Vault快照派生。secret已删除使既有身份presence=false而不推进身份revision；允许改既有名称，不允许新关联已删除ID或复活凭据。wrong role／另一Vault拒绝。presence也不是tag认证／解锁／凭据使用许可。
- native authorize predicate必须拥有I/O信用至实际执行退出。等候者取消不提前释放，worker.cancel/join等待真实退出；提交前撤销回滚，已提交但丢失receipt不自动撤回或重试。共享Vault与数据库不随页面派生上下文关闭。

## 迁移与证据

旧schema1/2→3使用既有SQLite Backup保存未checkpoint来源至`.bak-3`，同名备份不覆盖；DDL／版本在migration transaction中修改，hook验证原Vault形状（无KDF/解密），坏v2／shadow来源拒绝并回滚。v2 envelope保持逐字节相同，backup保留v2来源；future4及无版本既有表拒绝。不是power-loss、跨平台ACL、离线回滚或并发首次迁移验收。

271项完整storage suite/严格lint通过后，8项身份（含三个独立子进程）与2项native read/write取消反例通过。涵盖reopen/CAS/foreign owner、HLC skew/overflow/node稳定、tombstone、same-revision外部元数据变化、active/deleted预算、引用删除与反自报presence、迁移backup和坏来源rollback。新增ignored child由父进程明确要求receipt，不能以ignored/零测试当pass。

10 core／217 app／115 term／548 TS／113 native、check/probe/bindings与isolated真实SSH PASS；Profile隐藏／刷新／进程恢复、设置真实字号网格、主机CRUD三条真实Linux工作区兼容通过。日志`identity-schema3-{store,consumers,windows}.log`。Astrolabe6并发请求初次超时，不计通过；构建后改为串行，6实现文件Exact无诊断，未安装/重启LSP。签名暂存边界14文件与既有6 license／2维护公告发布门禁不变；图与依赖版本未新增。

## 当前页元数据入口

`product_identities.rs`注册`workspace_identities`／`workspace_identity_save`，只接受当前workspace本地origin、WindowToken及已确认document nonce。`ProfileReader`复用已有私库/共享worker和真实I/O信用；cancel／page retirement仍保留实际SQL工作信用至退出。通用typed MetadataCaptures复用主机的single-slot、300s expiry、single-use及exact page scope/registry校验，navigation/exit同步retire；新页不能拿旧票据提升权限。

`IdentitySnapshot`在同一事务中捕获Vault namespace并参与metadata CAS，包括空来源。Create只接label/username，后台从capture namespace预留UUIDv7 secret reference；没有Vault返回`workspace.vault_missing`，不自动初始化或替换DEK。Update/Remove只能选capture中的active ID；update复制原secret reference，不接password／Vault ID／secret ID／has_value／path／argv；未知字段拒绝。输出只是id/label/login及secret id/kind/hasValue、vaultPresent；存在标记不是解锁，也不公开envelope/ciphertext/Vault ID。

三项真实native私库/授权反例通过：wrong/duplicate/expired ticket、旧页queued read不建目录、late publish、no default Vault、后台分配引用、stale保存和共享DB存活；strict lint、原主机bank回归及7实现文件Astrolabe Exact通过。完整277 storage（5个专用child由父明确调用）／217 app／115 term／548 TS／116 native与check/test/probe/bindings通过；日志identity-admission-core.log。首次主机兼容窗口在profiles_listed前silent exit/native log为空，原因未定位，保留identity-admission-host-startup-failure-{native,wm}.log；并发构建结束后使用同一binary/原deadline单次重检CRUD／三进程重开通过，identity-admission-host-window-recheck.log。不把它归因于票据，也不抹去风险；probe新增early exit stage/status/reap诊断，不改timing。

## 当前页身份元数据 UI

`workspace-identities.ts`固定version1/字段/UTF8 byte budgets/UUID/role/presence/revision、256项及重复/namespace反例后解码为冻结值；save只编码label/login及opaque identity operation，不接受raw ref、password或hasValue。5s不确定timeout/abort丢弃迟到响应，不自动重试。`WorkspaceIdentityEditor`复用现有dialog/字段样式，一个selector＋一个表单；selector replacement、closed/retired page与late read/save不能消费原票据、重绘或抢focus。删除需二次点击；成功当前页回执将焦点返到Done；不确定结果丢弃旧表单并要求显式重读。无Vault时只有解释和Done，不展示可编辑默认身份或初始化key。初始元数据阶段只显示名称／用户名／只读密码存在状态；当前预览另有下述接收-only保存表单，既不读取密码，也不提供SSH连接按钮。

12 frontend protocol/editor反例，560 TS／116 native（专用ignored window fixture由script明确receipt调用）及native strict lint通过。`mise run probe-workspace-identities`已接Linux CI：probe-owned HOME/XDG私库，缺Vault先实际read/draw/exit，再由受target/workspace-identities.*路径约束的test-only helper初始化ciphertext-shape namespace，无KDF/认证/启用key。实际鼠标/键盘增改、刷新、三native进程reopen、两次确认删除/空表单draw、每次正常exit且零Shell start通过；不是密码认证/解锁/真实SSH消费验收。注册身份／主机／设置／分屏4模式通过，见identity-ui-registered-regression.log及workspace-identities-{no-vault,created,updated,refreshed,reopened,deleted-reopened}.png。

首次heading controller用错TSV索引， corrected left/top/height并要求两个已知sidebar SSH headings，first=host/last=identity，native receipt校验路由。80s fixture超时来自isolated HOME/XDG使mise重新解析/安装工具，先解析pinned Cargo/rustc与cache再offline运行fixture解决，原80s/8s不变；fixture cwd为crate，改compile-time manifest root约束而非放宽路径。首created screenshot显示正确字符串但identity未匹配host-only CSS选择器，沿用两个`:is(host,identity)`选择器修复字段垂直/满宽。保留错误证据；不计旧native silent startup风险已解决。

Native Astro Exact；TS Astro Unknown（初始化找不到typescript），不计通过、未安装。机械detectorflat-type warning用16px假定，实际共享CSS body14/title18/sidebar12及截图已核对；不修复无关设计系统或改product14px。源码／实际截图复核为当前incumbent单表单扩展，未宣称跨平台/完整a11y验收。

当前预览已完成主密码创建／解锁和saved identity密码接收-only保存。下阶段为target-bound SecretRef使用与实际SSH trust-before-auth消费；元数据存在/已解锁key/保存回执都不是连接许可。

## 已保存身份的接收-only密码表单

与元数据表单分开提交；先保存名称/用户名，选择后台已保存ID后才能提交密码。原page/identity/source/key epoch固定的一次性5s意图与原生信用负责授权；缺Vault/锁定/换页/重解锁/源变化均不能升级旧意图。按钮仅显示新输入；永不回填旧密码，未确认结果不重试。DOM与拥有型bytes/wire在成功、错误、关闭、换选项时清空，JS/IPC内部副本不承诺完全零化。

当前页错误移除字段后恢复Done焦点；18前端反例与596 TS通过，fake DOM焦点不能算WebView键盘关闭验收。本轮真实错误弹窗用固定960×640视口的独立鼠标Done关闭；其他视口/键盘另验。

`probe-workspace-identity-password`使用独立HOME/XDG/私库：实际master创建保持锁定→身份保存→锁定拒绝→显式解锁→一次密码写入→刷新/进程重开。Rust child只在受target/workspace-identity-password.*路径约束的ignored固定夹具中恢复同DEK，校验role/namespace/current epoch并比较固定字节；明确1 test +固定receipt，不是明文读取IPC或远端使用权限。Native verifier在80s窗口之前预编译，8s阶段截止不延长。原元数据CRUD/缺Vault/重开/删除、主密码取消/换页、设置与双pane两模式兼容均通过。


## schema 5 私钥身份与联合来源

`SshPrivateKeyIdentity` 使用 `ssh_private_key` 与可选 `ssh_key_passphrase` 两种独立角色；引用必须属于同一 Vault 且 ID 不同。密码身份继续拒绝私钥，解密口令不能冒充 SSH 密码。对象无 Debug／serde，引用不含私钥字节、文件路径或连接权限。

`key_identity_catalog`／`key_identities` 与原密码目录分开，保 UUIDv7、created_at、HLC、墓碑、实例绑定 CAS 和同 worker 的事务授权。上限 256 active／4096 tombstone、完整来源 4 MiB；坏行／错角色／跨 Vault 拒绝整体来源。来值 presence 被忽略，两凭据的存在状态由同一 Vault 快照派生，已删除引用不复活。

schema 4→5 先 `.bak-5` 后事务迁移，备份同名不覆盖；完整引用验证失败回滚。已有 Vault／密码目录／known_hosts 保持，trust 明确支持 4／5，future6 拒绝。不是断电、文件系统故障或跨平台 ACL 验收。

`KeyIdentityVaultSource` 在同一 SQLite 视图捕获私钥目录与 Vault 密文，比较完整元数据／密文和原实例；变化、原授权撤销或异常均拒绝。native read／validation 保留 I/O credit 到实际退出。成功比较不激活 DEK、不借出明文、不授目标使用许可；原 unlock epoch 和实际 peer 的使用授权待下一层。

本批 9 项目录／联合源行为、core/store strict、不可 Debug／serde 文档测试、完整基线和最新密码 SSH／身份／CLI 窗口兼容均已通过，见 `ssh-key-directory-joint.log` 与 `ssh-key-schema5-*`。私钥导入、UI、Agent 和生产密钥会话尚未开放。

## 可信后端私钥目标使用

`SshKeyTargetSource` 同事务冻结主机、私钥目录、Vault 完整密文和信任记录。来源比较不授目标许可；独立动作由原页面、实际 peer 守卫、原 available epoch、冻结的主机／身份和原 5 秒期限签发，普通准备结果不能转换。私钥与可选口令分别通过 AEAD 的 Vault／ID／role 认证，只有上层 Rust sink 暂时借用；页面无读取接口。

慢解码结束后，`PrivateKeySourceConfirmation` 再次校验同实例完整来源与原页面／epoch，成功后才进入认证。换页、同 DEK 重解锁、错 DEK、来源变更或取消不能使旧动作有效；认证结果只为历史 bool，不授新连接或终端权限。取消的等待者不取消资源清理，操作额度到 sink 的实际 transport／decode／completion 退出才归还。

候选上层 `ssh_private_key_adapter.rs` 初始仅 OpenSSH Ed25519，私钥 1–65536 字节，口令最多 4096 字节；bcrypt salt 1–64 字节、rounds 1–64，过限参数在派生前拒绝。单 CPU permit 和原 native guard 跟随实际 blocking 解析任务；两个确定性测试在原生 guard 检查处暂停，证明 abort／runtime shutdown 不能提前释放，退出后才可复用。锁定库 RSA 解析仅有界 MPint 解码，RSA 与其他算法在派生前拒绝，本批不代表这些算法支持。

`mise run probe-ssh-private-key-use` 编译同一候选适配器，固定真实 peer 验未加密／加密 Ed25519 各一次已验证签名、错误口令零 offer／签名、无 password／none 回退、真实 KDF 头 0／65／u32::MAX 拒绝及本地／远端资源退出。7 项借用反例、3 项准备反例、6 项全来源反例、2 项 worker 生命周期、strict／doc 已通过；完整 check/test/probe、801 前端、268 默认／282 ssh-use 应用、359 全特性存储（5 ignored 不计）、28 存储文档测试、组合严格检查与原密码 consumer 均通过；审计仍 8 许可／2 维护阻断。生产导入／身份 UI／共享根密钥会话另行授权接入。

## 原子私钥密文保存

`KeyIdentityCiphertextEdit` 固定原实例 `KeyIdentityVaultSource`、已保存身份和私钥／可选口令引用；两角色须同 Vault、不同 ID、正确 role，输入不改目标引用或复活删除凭据。native worker 仅接收 sealed records，原 available epoch 与 caller authorization 在执行和最终提交前核验；单事务成功才写一次 revision。第二份 SQL 写失败回滚第一份与版本，取消等待者保留实际 native guard 到 SQL 退出。九组真实 AEAD／SQLite／独立进程行为测试、368 全特性单元／2 集成／30 文档和严格检查通过，证据 `ssh-key-pair-behavior.log`。

应用层单次 `PrivateKeyIdentityWriteIntent` 冻结原页面及原 epoch，拥有的私钥／口令加密后在 SQL 前丢弃；后续解锁无法升级旧意图。upper adapter 独立验证受支持格式，原页 dispatch gate 与 epoch 随 SQL 原生操作守卫保留。该入口只为可信 Rust 保存，六组应用行为、18 项文档、严格检查与 Exact 诊断通过；独立 `PrivateKeyWriteBank` 七组原页／controller／epoch／5s 实际到期／native 取消资源持有行为与严格检查、Exact 诊断通过。首次全基线旧设置并发 Database 未复现，固定阶段诊断后 341 默认存储测试通过；根因未定位，证据保留，完整基线续验、295 项 ssh-use 应用测试及真实签名复验已通过，不提供明文读、页面命令或 SSH 使用许可。


## 当前页私钥元数据编辑

`workspace_key_identities`／`workspace_key_identity_save`／`workspace_key_identities_cancel` 使用独立原页面一次性快照票据与共享私库。前端仅编辑名称、用户名和新增时的口令需求；后端生成同 Vault 的两个独立引用，存在状态从 Vault 派生。元数据保存不授私钥读取、导入、解锁或连接权限。导航、窗口关闭及退出撤销未提交票据；后台 SQL 信用保留到实际退出。

六项原生反例、38 项相关前端、196 项组合原生与 strict，以及四进程真实窗口 CRUD／刷新／重开／二次删除通过；同帧选择器与字段显示新名称、稳定双引用／UUID／created_at／墓碑和零会话已检查。旧密码目录公开任务兼容通过。见 `probe-workspace-key-identities`。

## 当前页接收式私钥保存

`workspace_identity_key_available/prepare/submit/cancel` 不复用元数据授权；显式 workspace-ssh、原页面／controller／epoch、五秒单次准备与实际解析任务守卫共同限制保存。私钥接收类型有界 64 KiB，口令有界 4096 字节，字节原样保留并用独立角色加密；前端提交即清输入，关闭／换页／迟到结果不恢复内容，不确定写不重试。解析与认证共用一份 CPU 额度，取消／运行时关闭不提前释放正在执行的资源。

六项 Native 保存反例、17 协议／20 表单测试与真实两进程导入验证通过：锁定和错解密口令明确拒绝、正确 GTK 剪贴板导入只增加一次 Vault revision、双引用保持、刷新与重开、独立 Rust 原 DEK 解密后两份精确字节一致、零会话与原生退出。记录与公开任务见 `probe-workspace-key-import`；最新合并完整基线和公开复验均通过，最终审计仍以 5 阻断。此保存仍不授私钥读取或 SSH 使用权，共享根私钥会话采用独立动作另行验证。

## 独立私钥共享根会话基础

`PrivateKeySessionSink` 只在独立 session 动作中建立 PTY/shell，并一次转移 actor 与真实 SDK/native join；认证成功布尔值和 source receipt 不能升级为 root 权限。私钥与密码适配器共享无凭据 `ssh_terminal_io`，每个 crate 仅注册一次；SSH resize 回执仍仅表示后台网格应用和 window-change 入队，不声称远端内核确认。

`mise run probe-ssh-private-key-session` 使用同份生产 adapter、实际加密 Ed25519 双角色密文与四条真实连接验证：旧完整源零认证、PTY 建立阶段页面撤销并实际退出、提交后的同 root 跨导航存活／输入回显／尺寸通知、自然 EOF，以及原生读挂起时显式关闭。实际签名三次、密码回退零次；SDK/native 退出及 root capacity 归还确认。原密码 root 与私钥签名兼容、严格检查及 Rust Exact 诊断通过。记录见 `target/probes/ssh-key-root-peer-compat.log` 和 `ssh-private-key-session.json`；这不计真实 WebView 或产品私钥启动入口完成。

`PrivateKeySessionGesture` 在可信动作开始时冻结原页面／owner／可用 epoch 与已存 host/identity ID，不占原生额度、不读取私钥、不读源或连接；独立类型不能转成密码授权，批准后仅能进入私钥 session action。页面、owner、原批准 predicate、30 秒决定期与原 epoch 与密码 gesture 共用同一核验，后续五秒原生启动期限不续期；消费后仍 pin 原 epoch。三组私钥加原密码的实际 counterexamples、22 文档与严格检查／Rust Exact 通过；同一 DEK 经真实封装认证重新解锁在消费前或后都不能升级旧意图，新意图仍可建立 root 并完整回收。此模型不计产品按钮或 CLI 私钥入口完成。

## 产品私钥连接与共享启动银行

`workspace_ssh_key_session_begin/prepare` 从原页面冻结私钥 gesture，经独立联合源和实际指纹核验后批准；共享 `start/cancel` 按银行中的强类型分派，不能由前端字段升级密码／私钥票据。两类共用一个等待槽与 native 退出监督，错类型准备在源读取前拒绝。页面只提交已存 ID，密钥、口令和解密结果不返回页面。

31 项前端反例、真实生产银行／原密码 root 兼容及严格检查通过。公开 `probe-workspace-ssh-private-key-session` 四条真实 Linux WebView 路径已通过实际签名、绘制、输入、同连接刷新、布局通知、EOF 与取消／关闭回收；每个成功场景只签名一次，密码回退为零。支持 OpenSSH Ed25519 与有界 bcrypt 解密；不承诺远端内核尺寸确认、其他算法、三平台或私钥 CLI 验收。完整合并回归、全特性存储、组合原生及五条密码实窗兼容通过；审计仍为 8 许可／2 维护阻断。

私钥 CLI：`pulse ssh` 仍只接收已存主机／身份 ID，原窗口明确批准。一次冻结原页面／owner／解锁 epoch，再读取同事务密码／私钥目录及 Vault／主机／信任来源，由后端确定唯一类型；两个活跃目录存在同一 ID 时拒绝，删除墓碑不参与冲突。不可捕获两次 epoch、从后来解锁补权、接受客户端指定角色或把源快照当执行许可。批准文本明确私钥身份，固定类型与显示目标，进入对应强类型 gesture／共享启动银行。三组真实数据库与原 epoch 反例、25 文档测试、模型／组合原生严格检查和 Exact 诊断通过；私钥／密码 CLI 真实窗口验证锁定、取消、断开、换页、旧源批准零认证，重新读取后批准一次认证、终端恢复和 native 排空。独立截图已审阅，Linux CI 已注册；完整合并回归、全特性存储与组合 Native／绑定检查通过，发布审计仍 8 许可／2 维护阻断，尚不计冷 SSH、其他算法、三平台或发行通过。
