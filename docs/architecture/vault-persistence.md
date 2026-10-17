# Vault 密文持久化边界

## 当前交付范围

`pulse-store` 的后端 API 复用现有私有 SQLite worker，当前默认实现为 schema 5（独立私钥身份目录，本批完整兼容验收进行中，详见 [身份持久化](ssh-identity-persistence.md)），保留 schema 2 的 STRICT `vault_envelopes`、`secrets` 和 `vault_deleted_credentials`。envelope 保存版本／Vault ID／revision及110-byte BLOB；secret 保存 ID／Vault ID／kind／record_version／nonce／ciphertext，墓碑独立保留。`ssh_private_key` 与 `ssh_key_passphrase` 是独立认证角色；新身份来源不授明文或 SSH 权限。预览已有单向主密码和身份密码接收，仍无凭据明文读取 IPC；本批无新增依赖。

schema 1 的固定键 `app_state/vault.ciphertext.v1` 仅作为迁移输入。打开旧版本先保存 `.bak-<目标schema>`，当前默认目标 5 使用 `.bak-5`（历史 schema 2／3 阶段为 `.bak-2`／`.bak-3`），同一 migration transaction 内建表、验证有界来源、复制密文字节／revision／墓碑、删除旧键、更新 user_version；无 KDF／解密。损坏、目的表占用或 hook 失败拒绝并回滚，原来源／版本不变，备份保留；备份名占用拒绝。v2 及以上不读旧 Vault 文档；缺表或双来源拒绝，不回退／重建密钥。

这是正式**本地**秘密表布局，不是完整可同步Vault或产品创建／解锁入口；本地身份持久化／HLC／墓碑已接，远端同步merge、SecretRef目标使用授权、生产解锁成本策略仍待接。

## 密文格式与认证

- DEK envelope：严格110 bytes，`PVDE` + version1 + fixed-profile1 + Vault UUID16 + salt16 + nonce24 + ciphertext/tag48。固定 Argon2id v1.3，64MiB、t=3、p=1、输出32 bytes；非法长度／版本／profile／预期身份先拒绝，再允许CPU准入。仍沿用验证成本，未证明生产解锁延迟或密码强度策略。
- CredentialRecord：`PVCR` + version1 + profile1 + Vault UUID16 + credential UUID16 + kind长度1及1–32个规范ASCII字节 + nonce24 + BE u32密文长度 + ciphertext/tag。明文最多64KiB，密文至少16-byte tag，总长度必须精确。
- 凭据AEAD AAD是域 `PulseTerm/vault/credential/v1\0`、Vault UUID、credential UUID与kind；只修改头并提交修改后的“预期身份”，即使用同一key也必须认证失败。原始AEAD已发布known-answer与wrapped-DEK上下文不改变。
- 解析只确认形状，不确认密码、认证tag或授予权限。调用方必须使用可信预期Vault／credential／kind认证；失败不返回明文、不生成替代DEK。密钥、SecretBytes、CredentialRecord及ValidationWrappedDek均无Debug/Serialize；只有显式密文codec可输出bytes，明文／key仍零化。

## SQLite 事务与生命周期

- 旧文档仍严格version1／未知字段拒绝，最多4MiB、256个凭据和4096个删除UUID，迁移先校验整来源。v2保留等价序列化预算；SQL在读入Rust前限制字段、nonce／ciphertext长度和行数；count仅扫描预算+1条，非法非目标行亦拒绝。不存在Vault必须同时没有secret／墓碑，损坏不回退为空。
- 多表读取使用同一Deferred read transaction；写入在Immediate transaction内读取完整当前来源、比较版本并更新。WAL中另一个连接推进时，旧读transaction仍取得同一revision和密文，退出后才观察新版本。
- Snapshot绑定StateStore实例，空状态也不能跨实例写；revision单调到i64::MAX，末项删除不重置。Initialize只允许显式首次创建，不能替换现有envelope；Create不覆盖存在或已删除UUID；Replace只更新存在的同kind ID；Remove保留墓碑。墓碑满时拒写，不自动清理／复活。
- Native执行predicate拥有I/O信用，队列等待／读写／提交前均检查当前授权。取消或关闭等待方不能提前释放实际工作信用，worker shutdown/join仍等执行结束。提交前撤销回滚；已提交但丢失回执不承诺撤回，也不自动重试。
- KDF、加解密和明文都不在SQL worker执行。解锁的现有CPU准入／epoch／keyless receipt必须继续显式确认；一个持久化Snapshot不升级窗口或SecretRef权限。`StateStore::confirm_vault_unlock`已接原生联合确认：snapshot实例、规范化全密文字节／revision、ready实际认证的envelope及当前lifecycle epoch共同验证。Immediate transaction在启用点阻止其他SQLite writer；同revision下变化亦拒绝。旧验证confirm仍只是内存验证API，产品适配器不可用它替代持久来源核验。
- 调用方持有弱、无密钥的epoch撤销守卫；等待取消立即清除本轮pending/active key，但authorize持有的I/O信用仍随实际原生操作保留至退出。原生回执另有无密钥激活守卫，未送达／未接收时清除仅对应epoch的key，不误锁后来成功的解锁；成功观察回执后才保留本轮启用。SQL执行的是元数据／密文核验与控制器确认，不执行KDF、解密或明文操作。
- 此联合确认只建立启用瞬间的来源／epoch一致性，不授权SecretRef／窗口，不自动订阅外部文件变化，也不能撤回已经交给传输的凭据。页面／owner退休的适配器必须同步撤销lifecycle epoch，不能仅提供一个非原子boolean predicate。产品页面授权／自动应用锁仍未接通。

## 身份／SecretRef 元数据边界

`pulse-core::secret_ref`仅支持已审查的`ssh_password`／`ssh_key_passphrase`角色；UUID不可nil，不推断或降级未知kind。`SshPasswordIdentity`复用Endpoint的label256-byte／username255-byte及控制、空白、bidi／不可见字符校验，仅接受password角色，不接路径、参数、环境或明文。两者无Debug/serde，外发必须有专门DTO审查。

`VaultSnapshot::secret_ref`从私有已校验形状的密文快照派生`has_value`，不信任调用方状态。缺Vault返回None，缺值／删除返回false，存在的错角色拒绝；Vault ID与credential ID共同识别引用，同UUID在另一Vault不混同。presence不是密码／tag认证，也不保证缓存快照仍最新，更不授予Key／窗口／目标使用权限。模型支持显示未设值，但不会创造替代DEK或复活删除ID。

9核心和2项真实SQLite反例通过，完整回归9 core／267 store／217 app／115 term／548 TS／113 native与check/test/probe/bindings/isolated真实SSH PASS。5实现文件Astrolabe Exact；日志`secret-reference-{core,audit}.log`。仅复用已锁定uuid作为core依赖；audit仍6 licences／2 unmaintained，bans/sources通过。身份已接本地schema3保存／HLC／墓碑及原生worker接口；身份当前页列表／写入IPC及metadata-only界面已接（见身份持久化文档）；仍未开放凭据选择或完成SecretRef到SSH目标的授权消费，不把后端持久化称为完整身份管理。

## 持久来源与 epoch 联合确认验收

7项真实SQLite／原生生命周期反例通过：当前来源成功；旧revision／跨实例／错误控制器／旧epoch拒绝；相同revision的密文变化拒绝；认证过的envelope必须与当前来源逐字节相同；第二连接只能获得实际SQLite busy/locked，不能越过确认启用点；取消立即清除candidate而I/O credit／shutdown等待真实native退出；丢失旧回执不清除新epoch；真实密码解锁后恢复同一持久凭据。未新增password/明文IPC，也未启用GUI解锁。

`vault-joint-confirmation-core.log`：265 storage、217 app、115 term、548 TS、113 native、15 doc及mise check/test/probe/bindings通过；5个实现文件Astrolabe Exact无诊断。未改GUI消费／命令注册，不重跑已有三条schema2实窗；不是新的GUI unlock验收。`vault-joint-confirmation-audit.log`仅保留6 licence／2 unmaintained，bans/sources通过，发布阻断不变。
## schema 2 的新增验收

255项完整存储测试及严格lint通过，随后7项迁移／实际worker测试通过（其中新增3项）。证据包括：v1密文字节／revision／删除引用完全保持，`.bak-2`包含提交前来源；坏来源及导入后故障回滚DDL／旧键删除／版本；目的表／备份占用不覆盖；真实StateStore打开v1、join和再次打开v2保持同一凭据；v2缺表或注入旧文档拒绝；WAL事务读不混合新旧状态。

受影响其他工作区测试、217项应用／115项终端／548项TS／113项原生、check／probe／bindings通过。Profile隐藏／刷新／进程重开恢复、设置字号实际网格、主机CRUD三条Linux真实窗口路径通过，分别记录在 `vault-schema2-store.log`、`vault-schema2-consumers.log`、`vault-schema2-windows.log`；未重跑其他不受影响窗口模式。4个修改实现文件的Astrolabe诊断Exact无错误，签名暂存边界不变。

## 初建、当前页 owner 与 native service

`vault_creation`使用同一global pre-admitted KdfJob，PasswordInput移动到blocking closure并零化；只返回110-byte envelope，暂态DEK/KEK在该closure结束前销毁，不自动激活。source空快照＋当前原生predicate才能Initialize，取消/丢失ReadyCreation撤销persistence predicate，native信用保留到真实退出。4项实际创建／认证及queued/running取消、旧新回执、panic反例通过。首fixture max_blocking_threads(1)被常驻SQL worker占满而超时；仅SQL+KDF fixture改2，queue fixture保留1，global KDF仍1，原deadline未延长。

`UnlockRevocation`是无密钥same-epoch弱controller handle，native退休可清除刚启用的旧epoch，不影响新epoch/另controller。authorized unlock保留独立resource Arc到worker.finish/send/整个blocking closure结束，不能因FnOnce compute消耗而提前放信用。2反例及完整283存储通过。

`pulse-app::vault_owner`复用真实Registry/PageLaunchScope，opaque store origin及envelope固定绑定，foreign/changed source failclosed；weak pending slot在cancel后仍由真实CPU/SQL predicates持有。初建随机UUID在Rust产生，已存在来源不替换；unlock只允许Locked，经DB来源/epoch原子确认后交付move-only PageVaultActivation，再由原Rust等待方显式accept，post-SQL丢失回执/取消清除本epoch。之前已accept的root key可跨换页，但app lock/close销毁；pending在navigation立即撤销。mutex poison立刻close key scope，不只报Denied。4 owner真实私库/页面/来源/退出/receipt-loss与poison反例、221完整app通过；不是SecretRef或连接权限。

真实workspace root已持有owner，native Started同步retire，Destroyed/ExitRequested立即close，physical exit再次验证幂等close；不关闭root PTY/共享DB。ProfileReader同一个私库和单I/O slot，监督background task；weak WaitCancel在native waiter丢失时立即取消，CPU与SQL predicate保信用到退出，shutdown等待真实SQLite callback。3 native service反例通过，包括wrong password/duplicate initialize/actual source auth/app lock、旧页/目录切换拒绝、实际阻塞SQL取消与physical drain。不读取用户HOME；创建／解锁服务现已接显式开发预览的票据入口，测试专用无票据解锁分支仍cfg(test)；真实入口只接收当前页单向主密码，不能读取秘密或绕过前端确认。生产仅注册current nonce/source/window核对后的`workspace_vault_status`／`workspace_vault_lock`，version1+固定四态，无ID/envelope/credential；frontend5严格decode/abort/uncertain5s no retry反例通过，已由预览保险库对话框应用，不能作为凭据／SSH使用权限。

`vault-owner-{store,app,native,consumers}.log`：283 storage／221 app／115 term／560 TS／119 native（随后第三service test单测通过）与check/probe/bindings/strict lint通过；native Astro Exact。优化版exact创建+认证+SQL sample通过，两个64MiB/t3/p1 KDF总test elapsed0.42s（build187s不含在该elapsed），不是单次KDF/真实WebView或三平台生产解锁延迟保证，固定验证profile及强度策略未悄然改动。已接前端明确确认／取消opaque票据与显式主密码预览入口；JS取消Promise仍必须传播为原生撤销，不能仅密码正确就让迟到结果启用。旧native启动/视图deadline风险、14签名暂存及6 license/2维护公告门禁不变。

## SecretRef 来源／epoch 前置校验进度

`AvailableKeyEpoch`是弱controller-bound、不可Debug/Serialize的无密钥元数据证据；只有Available能捕获，Locked／Pending／closed／poison／exhaustion拒绝。`seal_in_epoch`和Rust-only `open_for_adapter_in_epoch`在同一短key mutex内检查controller+epoch并执行crypto，同UUID同DEK的另controller或后来重新解锁不能升级旧请求。它不授予身份、SecretRef、主机或前端权限，`is_current_for`单独检查也不能代替同锁crypto操作。5确定性反例及全部storage/app/native消费者已通过。

`IdentityVaultSource`通过原有有界SQLite worker的一次Deferred事务捕获完整有界身份metadata/has-value派生与encrypted Vault/envelope/credentials/tombstones，不可Debug/serde。native `validate_identity_vault_source`在Immediate writer事务对实际store实例、持久身份完整metadata和Vault canonical全密文来源比对，并再次检查caller predicate；同revision被外部更改也拒绝。源缺失保持absence，不自动生成密钥或默认身份。取消/worker shutdown继续保留实际native predicate/I/O信用到退出；current-thread fixture不得阻塞唯一async线程等待自己尚未入队的请求。

`PageVaultOperation::validate_password_identity_source`只为准备阶段：选定的saved active identity、SshPassword角色、非空存在状态与Vault namespace必须匹配；绑定当前registry/page/native owner source与已确认Available epoch，再在SQL前后复核原operation和epoch。错误ID、未填值（客户端has-value无效）、不同StateStore来源、锁定／删除／换页均拒绝。固定实密码恢复同DEK的actor反例通过；故意用另一DEK加密的合法结构记录仍能通过metadata前置校验，证明这不是凭据AEAD成功或目标使用权限。此方法没有plain返回，也没有新增前端命令；真正使用仍须独立确认SSH peer/host trust、目标操作权限、解密认证和迟到使用撤销。

key-epoch与joint capture基础完整检查已通过（293 storage／225 app／123 native，21 compile-fail docs）；新native exact-source validator与当前页前置方法针对性反例/lint/Exact diagnostics及完整受影响消费者已通过（293 storage／226 app／123 native／21 compile-fail docs、check与bindings）。后续继续接真正的credential写入/目标使用，而非把这个准备校验包装成已完成的SSH/Vault产品授权流程。

## 保存身份密码的后端写入进度

`SecretBytes::from_owned`接管有界已拥有buffer，错误／drop也零化，不增加Debug／serde。`CredentialRecord::seal_in_epoch`从既定scope派生Vault AAD、以已有available epoch同锁加密，不导出DEK；Rust-only `open_for_adapter_in_epoch`必须提供可信expected id/kind并做AEAD，仍不是前端读取或SSH目标权限。有效codec roundtrip与wrong id/role/Vault、tamper、lock/旧epoch拒绝已测。

`PageVaultOperation::write_password_identity_source`只接原native source、saved identity id和拥有的secret。Ref/vault/ssh_password role及create-versus-replace从当前保存来源派生，不能由页面提供原始引用/类型/路径。密封后立即drop plaintext，再送`edit_identity_password_ciphertext`原有bounded worker；SQL线程无KDF／明文。旧`VaultEdit`通过no-op preflight保持原调用契约，新路径在原Immediate事务、修改密文前核对完整身份／Vault来源与exact ref，再由原最终predicate检查同available epoch和current page。丢失回执不回滚已提交密文；未知结果不自动重试。锁定、角色换写、错误id、stale source、deleted identity／credential tombstone均不能升级/复活。

高层write waiter在错误或future drop时立即撤销本操作，而不是锁掉已确认主密钥或提前释放实际SQL信用。固定真实master认证→saved unfilled identity→actual DEK加密→private SQLite→以expected metadata认证实际persisted record通过；直接native等待取消与保留原operation时drop高层future均用latch证明旧权限即刻失效、owner slot保持Busy直到原生callback真正退出、Vault revision不变，master remains Available。异步fixture只在SQL线程挂起，不阻塞自己的single-thread async runtime。

上述行为、strict lint、Rust Exact diagnostics及完整check/test/probe/bindings/doc/native消费者已通过（294 storage／227 app／585 TypeScript／123 native、21 storage compile-fail docs）。该阶段只接受Rust-only写入基础；当前进展已继续到下述一次性epoch/source票据与接收-only开发预览。没有授权连接SSH或读取凭据，旧编辑票据不能在重新解锁后升级权限。

## 操作级撤销唤醒

`PageVaultCancellation::wait_cancelled`用于后端protocol supervisor，不轮询：在原operation显式取消／drop、owner lock／close／drop、page retirement、foreign source绑定拒绝／authority poison时唤醒；Notify注册enable后重新读取原weak owner/token，避免lost wake。只保key-free通知数据，不延长key、Owner、Token或实际CPU／SQL信用，不选择／撤销新的operation。

先同步live=false／清候选；candidate revoke take在mutex外执行，通知在authority mutex释放后发出，包括Scope错误和poison分支。五针对性反例证明重入waker可读authority/candidate状态、保留真实native predicate时仍Busy至其退出，丢弃waiter不取消operation，owner drop不被观察者阻止、双私有state.db外来绑定fail-closed。235 app／7doc、125 native（2专用window fixture另行实测不计默认pass）、check／bindings／upper实际peer auth0，以及重建后的Vault取消／navigation与身份密码save／refresh／process reopen＋独立bytes校验通过，Astrolabe owner Exact。

这是**原操作显式撤销通知**，不是持续source／epoch／page／native guard观察，也不是实际I/O已退出、使用许可或credential-read入口。Supervisor仍需原epoch/source/actual peer重检、自己的原截止和NativeIoCompletion／其他native join证据；信号／waiter取消不能提前释放活动资源。尚未增加真实Vault→SSH认证或明文返回IPC。

## 密码编辑意图／敏感票据进度

`PasswordIdentityWriteIntent`保存最初native PageVaultOperation、原始完整source、saved identity id和AvailableKeyEpoch，不可Clone/Debug；单次write不再捕获后来epoch。真实同密码恢复同DEK到新的Available时，原page/source和operation仍live，旧intent仍被拒绝而非升级；以新epoch认证原persisted record确认DEK没有替换。换页也拒绝，密文revision不变。

`PasswordWriteBank`从指定PageVaultOwner派生registry/controller，不能传另一registry或用同UUID/page伪装不同owner。one ready/active ticket，5秒原始monotonic截止，runtime弱timer主动retire并以watch及时退出已取消timer；take一次、错ID/foreign page/duplicate不消耗有效意图。deadline不仅依赖timer，taken permit把current/time gate交给原最终SQL predicate并拒绝迟到结果。cancel/permit drop清理前端票据，但原native owner/reader I/O信用仍由实际predicate持有，不能在任务退出前释放。取消不会锁掉已确认master或另一个epoch。

固定真实master→saved identity→bank→同DEK持久密文验证通过；wrong owner samepage、foreign page/id、duplicate、ready取消、实际SQL callback active取消/abort、quota Busy至Exit且revision不变均通过。ready timer在没有任何后续frontend take/cancel/read时5秒后销毁intent/释放非活动slot，随后原owner可重新begin；这不把活动native I/O提前释放。Root workspace导航、系统窗口销毁和物理exit barrier已挂retire/close，123 native+strict lint/Exact通过。完整229 app／6 app doc消费者回归通过。受监督的shared私库prepare/submit、接收-only原生入口与下述界面已验证；准备信用沿intent保留，submit不再次reserve。原生writer监督测试覆盖caller取消、callback真实退出及namespace/导航拒绝。两个真实KDF原生夹具共享test-only异步锁，避免其并行抢生产global1 CPU许可；不修改生产额度或重试策略。

票据已接到独立workspace开发预览的身份密码prepare/submit/cancel，不进入默认validation入口；页面只选择saved identity UUID与opaque ticket，密码以≤4096-byte拥有型数组接收，拒绝空值/非UTF-8、不套用新建主密码12-scalar规则。普通MetadataCaptures的300秒lazy metadata票据不能复用为敏感密码权限。准备IO信用不能在submit再次reserve造成自占死锁，也不能借编辑票据读取密码、改SecretRef/Vault/path或连接SSH。

## 当前页主密码开发预览

工作区单独注册`prepare/submit/confirm/cancel`：原生window/local origin、已确认document nonce、同一page/owner/controller及单次UUIDv7票据均须匹配；仅显式`master_password`模式，页面不能传Vault ID、路径、envelope、KDF参数或凭据ID。创建输入至少12 Unicode scalar且确认完全相同、不trim/normalize；既有envelope的解锁不套用新建强度规则。原生`PasswordIngress`只有Deserialize，拒绝Debug/Serialize与超过4096-byte输入；一次有界预分配避免增长释放旧明文副本，部分失败buffer也零化。CPU仍64MiB/t3/p1/global1，票据/工作/客户端现有5s截止不增加。

创建只持久110-byte envelope并保持Locked。Unlock首先完成真实密文来源与epoch联合确认，激活回执驻留native bank返回Pending；仍活跃前端再confirm才能accept。cancel/窗口/换页/原生主动到期都撤销本轮候选，丢失回执不能遗留可用DEK，不误锁后来epoch。bank从既定owner派生registry/controller来源，另owner不能替换；expiry timer主动执行，无需GUI轮询。前端对话框关闭、刷新、未知结果清空密码DOM与自己持有的bytes/wire数组、旧表单/锁按钮不能操作新render；失败只重新读取真实状态，不自动重试创建/认证。JS字符串、浏览器/框架JSON内部副本、swap/register等不承诺完全清零。

Profile列表在旧认证实际退出前可以等待读取准入：每个reader最多1 waiter，现有5s上限，Started/close取消等待且最终重新验证原页。只等待读许可，不提前释放旧CPU/SQL信用、不重试不确定写入，不造默认Shell。真实取消／换页曾触发新页Profile Busy，3确定性反例和实窗已证明改为等待后恢复。

Linux固定合成密码/隔离HOME实窗验证创建Locked、错误密码拒绝、Pending→当前前端确认、手动锁定、刷新及进程重开；认证已准入后Escape与Ctrl+Shift+R都未增加确认计数，新页和重开仍Locked。截图和native无内容阶段在`target/probes/workspace-vault-*`，两项mise/LinuxCI任务已注册；完整check/test/probe/bindings及285存储／225应用／115终端／585TS／123原生／17doc通过；已注册两项Vault实窗和身份／主机／设置／基本分屏通过，比例导航单独复检通过。合并运行在ratio slider焦点处输入超时，探针改为明确点击已就绪窗格再输入；不得把尺寸ready当作焦点或新增输入权限。当前会话锁定不关闭现有会话的实窗组合、钥匙串/记住我/凭据写入/真实SecretRef使用、生产成本策略、Windows/macOS仍独立待验。

Cargo仅为dev/test的`argon2`依赖开opt-level3，避免未优化算术贴近固定截止；KDF参数与期限不改。首次未带阶段的创建拒绝仍未归因，后续调试约4455ms及优化依赖后约1302ms只是此机样本，不是原因消除或生产解锁延迟保证。Rust diagnostics Exact，TypeScript LSP仍Unknown；已有签名暂存边界、6 license/2维护公告与窗口deadline风险不变。

## 已保存身份密码的当前页开发预览

独立workspace注册`workspace_identity_password_prepare/submit/cancel`，原生window/local source/document nonce与bank原page/controller都须匹配。prepare从shared private DB同一事务读取完整identity/Vault源，选择已保存password identity并冻结原AvailableKeyEpoch；与主密码不同bank、单次5s UUIDv7票据。submit只接ticket与拥有型≤4096-byte非空UTF-8密码，页面不能传Vault/ref/kind/path/ciphertext或新epoch；后台同epoch密封，在SQLite Immediate writer lock内精确核验全来源、身份引用/role与最终page/epoch许可，SQL只处理密文。

界面采用独立密码form，未保存的元数据必须先保存；显示新输入不读取旧值。成功后读回真实presence、清空输入及owned bytes/wire，不把presence当解锁/目标许可。错误/未知回执不重试、不承诺已提交写入回滚；原页/表单已撤销的迟到响应不重绘/取焦点。JS字符串及框架内部副本零化限制不变。共享Root native任务监督与退出排空保实际CPU/SQL信用，取消不释放尚在执行的许可，也不锁掉另一个已确认epoch。

Linux受控实窗通过锁定拒绝、解锁后保存、真实刷新/进程重开与独立Rust fixed-byte解密校验；child仅读probe-owned路径并显式运行1项ignored fixture，绝非plaintext-read IPC。原8s/80s与5s敏感意图截止未增加，依赖优化不改KDF参数。18针对性前端、295 storage／229 app／596 TS／125 native（2个专用fixture不计普通pass，按脚本明确调用）与完整check/test/probe/bindings通过；已注册mise/Linux CI。

原元数据CRUD、主密码撤销、设置与双pane两模式兼容通过；组合设置首次view bootstrap_timeout仍未归因，重检成功不消除风险，保留identity-password-settings-bootstrap-timeout.log，添加失败-only阶段截图。两次native verifier缺陷分别是package cwd与误传Vault UUID而非credential UUID，已改测试定位/期望，不放宽生产路径/AAD。真实失败弹窗本轮仅独立鼠标Done关闭；前端focus/cancel单测不能替代真实键盘/跨DPI。Rust Exact，TS LSP Unknown，未安装服务器。Audit日志仍6 license/2维护与bans/sources ok；外围20s超时不算audit通过，发布门禁不变。首次指纹批准／SSH actor 产品连接、钥匙串和三平台仍待实现；显式后端目标密码动作见下节。

## 显式后端目标密码动作（受控验证已验）

`pulse-app/ssh-use`保持网络中立：后端显式`PasswordTargetAction`与普通控制／`SavedPasswordUsePreparation`分开，后者不能转换成`PasswordTargetUseIntent`（compile-fail）。只有受信任上层适配器先授权实际动作、实际连接与完整 trust 来源，才能准备一次固定 host/identity 的意图；不接受前端 guard、可调用插件或任意 SecretRef。

意图冻结原页、来源、可用 epoch 与原 5s 截止。精确 SQL 来源检查、角色/Vault/ref 校验后，在 SQL 外、同 crypto key lock 下认证密文；UTF-8 密码最多4096 bytes，仅借给受信任 Rust sink，接口不返回密码、DEK、可重用连接或会话许可。解密后、协议调用前及回复后重新检查源；这些是动作线性化点，不是持续文件锁或 SQL→网络原子事务，不能撤回已释放到获准传输的字节，不重试不确定发送。

后台任务拥有意图、临时零化值和 sink；丢失结果等待方只撤销原动作，不 abort cleanup。协议适配器在解密前将 key-free 授权保留到**实际** I/O 生命周期，不能只放在等待 future。`AvailableKeyEpoch::is_original_current`只临时升级原 weak controller，不能保活／升级 key scope；真正解密仍使用原 `guard_epoch`。排空后只可交付历史认证 bool，原 page/token/epoch/截止仍须有效；自身关闭后的 peer 失效不能被当作另一连接使用许可。验证路径始终关闭连接，尚不注册 SSH session。

`probe-ssh-password-use`用临时私库和合成固定密码完成真实 TCP 认证，auth=1、精确字节与 native admission drain；unknown/wrong pin、源修改、实际错 target、换页均未新增认证。Root 单测覆盖同DEK新epoch拒旧意图、等待方取消、锁定期间迟到成功拒绝及 drain 前 Busy。锁定版 russh 的 `Method::Password`在 Drop 零化 String、Debug隐藏密码已检查；不声称所有 OS/SDK 加密缓冲都已完成内存审计。完整消费者与默认依赖隔离通过，见开发验收记录；首次指纹批准、实际认证中网络取消／actor接续、生产 SSH 命令、钥匙串及三平台独立待验。明文读取 IPC 仍关闭。

## 已有针对性证据与未覆盖项

真实私库重开和三独立进程验证了同一DEK认证／凭据恢复、错误密码拒绝、删除后重开不复活；deterministic原生阻塞测试验证取消／shutdown保留读写信用；反例覆盖跨实例／旧revision、kind替换、未知格式、非目标损坏、重复项、预算／revision／墓碑耗尽。联合确认后的完整受影响回归通过：265项存储（4个专用child fixture忽略，由父进程明确调用）、217项应用、115项终端、548项TS、113项workspace原生和15项doc测试，mise check/test/probe/bindings通过。Argon验证debug时长不当作生产解锁延迟。Manifest提取留下的3处path wildcard已归到既有versioned workspace依赖，锁文件hash不变；audit仅余6项license拒绝及2项维护公告，bans/sources通过，发布仍阻断。

这些不是系统钥匙串、真实SSH凭据消费、完整生产GUI解锁策略、三平台ACL或power-loss／磁盘失败验收。局部版本CAS与行级AEAD不提供离线数据库回滚检测或整个catalog认证manifest；不得把同UID外部篡改的metadata称为已认证。并发首次迁移、power-loss、同步与授权层须独立验收；已有同名备份时拒绝，不自动删除、覆盖或无限重试。
