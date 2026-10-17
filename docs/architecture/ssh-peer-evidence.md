# 实际 SSH 对端证明边界

## 首次候选与批准后端（受控验证已验）

`peer::observe_host_key`从实际SDK handshake callback取public key，始终返回false，包括alreadyPinned；ObservedHostKey无公开构造/Clone/Debug/serde，绝非VerifiedPeer/可复用连接。原5s总截止包括owned-I/O退出，缺源不发SSH字节，cert拒绝；未知/changed/matched仅是捕获来源下的展示分类，不是source持续有效或批准。SDK正常拒绝后才关闭I/O，避免shutdown BrokenPipe覆盖UnknownKey；外层取消仍由原CloseGuard立刻唤醒，native reservation保持到actual inner drop。

可选managed-store的FirstPinApproval在dial前冻结全源/selected host，逐行parse全部应用pins，再由实际observer mint；仅Unknown允许prompt，Known/Changed不能走首次批准。只有受信任后端原page/action/native guard和显式用户决定可一次消费，300s从观察开始；guard不得保DEK/可用key controller。Drop/cancel撤销未提交native写，已提交但丢失回执不承诺回滚，也不重试。confirm返回历史trust revision、不自动reconnect/认证/创建Actor。

`approve_first_ssh_pin`同一Immediate事务比较完整host/identity/Vault/trust与store实例，核对selected endpoint=record.target，任何既有target都拒绝，再插入/readback/最终native predicate/commit。原SQL credit直到实际exit。已使用schema4正式known_hosts表及含实体元数据／墓碑的完整CAS来源，见 [信任记录持久化](ssh-trust-persistence.md)；无Vault/锁定不阻止公开指纹批准，也不会启用key。

固定real TCP＋原registry页面probe证明退休页拒写、foreign registry/同号window拒绝、最终SQL授权阶段Started回滚、新页明确批准及私库重开strict peer，全部auth0。观察原5s timeout与SDK已spawn后的abort真实EOF/准入释放、real-DB foreign/wrong host/same-rev drift/known-pin/取消保实际信用反例通过；完整消费者308 store/22 SSH/4 doc/236 app/596 TS及旧auth0/auth1/default依赖隔离通过。此段后端验证不代替下节实窗验收；已变更key替换、用户SSH文件、host certificate策略或生产session仍待接。

## 当前页审批与 Linux 指纹对话框（显式 workspace-ssh 已验）

独立工作区仅在 Unix 显式 `workspace-ssh` 构建启用此工厂。默认构建没有网络依赖；每次可信 Finished 向原文档注入的固定 feature bool 只控制显示，不授予权限。页面选择已保存 host UUIDv7，不能提供地址、port、key、认证参数或路径。四个命令分别 prepare、observe、confirm、cancel，均验证 Tauri local source／WindowToken／当前已确认 nonce／原 PageLaunchScope。

原页 `PageTrustActions` 是不持 key 的单槽准入：Ready 后端随机票据30s内单次观察，动作300s从准入开始；取消保原 native 引用和 SQL 信用到实际退出。观察先冻结完整来源和目标，DNS blocking closure 保 native guard 到实际返回，TCP／SDK 截止不因外层取消延长；这些引用不持 DEK／Vault controller。此轮只使用应用 pin，系统 known_hosts 不读取。观察连接实际退出后返回 SHA256 公开指纹；unknown 才可批准，matched／changed 没有批准票据。后续连接必须重新严格验证实际 peer，不能重用此观察连接。

票据状态 Ready → Observing → Approval → Confirming；Confirming 只留 weak 原动作撤销元数据，关闭对话框仍可撤销未提交 SQL，绝不使 I/O slot 提早空闲。提交同原全源比较和原动作授权；附加 native 写 guard 只缩小权限。完成后先清 weak 记录再返回历史 revision，已提交而回执丢失不回滚／不重试。前端严格解固定 DTO、默认 Cancel、明确 Save，旧 epoch／nonce／晚回执不能画回或抢焦点；数据用 textContent，不放宽 CSP。

独立 OpenSSH 与真实 X11 键鼠／WebView 已验证未知默认取消、待批准刷新、明确保存、matched／进程重开以及真实服务器换 key 阻断；私库 public key 字节、UUIDv7／HLC 与原记录一致，认证请求和创建 Shell/session 均为0。4原生私库票据反例、5应用动作反例、317 storage／241 app／22 SSH＋4doc／129 opt-in native＋125 default／612 TS及 check/test/probe、严格lint、绑定检查通过；ignored不计。默认工作区 host CRUD／Settings窗口和无网络 normal 依赖通过。证据在 target/probes/workspace-fingerprint-*，CI已注册。首次 WM ready 超时在 app 启动前，独立复验通过但未定位；共享/tmp inode启动失败也不算通过。5Rust诊断Exact；TS LSP Unknown与stale impact结果不当作验收。

此项不是生产 SSH 会话、凭据使用 UI、已知 key 替换、系统文件导入、SSH certificate、真实 DNS 取消时延或三平台验收；发布8项门禁保持。

## 同事务目标源

可选pure `pulse-store/ssh-trust`下的`SshTargetSource`同一Deferred WAL transaction捕获完整host catalogue、identity/Vault密文和application trust原始记录，来源包含StateStore实例。即使empty/no Vault也不跨实例，absent只与same-store、zero revision、无envelope/credential/tombstone的真实absence相等；None↔Some变更拒绝，原initialized-only crypto比较函数不放宽。存储只校验规范化文本形状，SSH上层仍须逐行解析所有key并fail whole source。

`validate_ssh_target_source`以Immediate writer lock比较完整host字段/ID（不只revision）、identity metadata、Vault源与每个trust文本/target，再检查native predicate，read-only rollback。取消等候不释放predicate持有的真实credit；queued关回执跳过，不关闭shared worker。它是point receipt，不是持续源锁、密钥启用、peer或目标使用许可，不返回明文。

5真实DB反例与23 doc/strict lint/304 storage（5专用child不计pass）/15 SSH/原真实互通/check/bindings通过；WAL新提交不能将旧host与新pin混合、same-rev外部endpoint改动/credential presence更新/坏全源/foreign empty/revoked native拒绝。Astrolabe source Exact。无schema、新包或网络层依赖；default desktop仍不含russh。后端显式目标密码动作已受控验证（见vault-persistence.md），但source校验本身仍不能消费SecretRef或授予生产连接。

## 页面／原 epoch 预备边界与上层实连接样本

曾把peer耦合放在app的可选依赖中，`mise run check`仍按现有规则拒绝app→SSH。实连接个例通过不覆盖该失败；未改分层规则。现`pulse-app/ssh-use`仅启用pure store，`SavedPasswordUsePreparation`冻结saved host/identity、全源、原available epoch、page/native guard和原5s deadline；单次`validate`返回point receipt，不解析公钥、不返回密码、不授权目标。真实同DEK重解锁使旧epoch失效，未填credential、native guard撤销、全源改变和page retirement也拒绝。230 app／7 doc／strict check与default workspace desktop normal tree无SSH/Russh通过。

实际Peer/trust逻辑在上层`xtask/src/ssh_target_probe.rs`（Unix、显式ssh-target-probe）中绑定`VerifiedPeer`，比较saved endpoint，逐行解析全application pin源并拒绝changed；仅synthetic loopback／私有临时DB，没有system-only fallback、用户SSH读取或生产网络入口。根guard随actual peer owner drop撤销，传入app原native predicate；正确准备无authentication，wrong target、source change、peer drop、page retirement、changed pin拒绝。`mise run probe-ssh-target`报告`authenticationRequests=0`、`plaintextReturned=false`、`credentialUsePermission=false`、`productSshOpened=false`，只接受预备边界，**不是完整credential-use或三平台验收**。固定报告`target/probes/ssh-target-preparation.json`与Linux CI已注册，upper/current neutral Astrolabe Exact。

纯validate回执无持续source锁，不能在之后跳过来源/epoch/page/实际connection重检。下一步需要独立的受监督凭据使用动作及指纹批准产品流，不增加明文读取IPC。审计仍6license／2maintenance失败，签名/staging边界与其他timeout/平台gate不改。

## 原生 owned I/O 保留与完成

`retain_native_admission`必须先重检当前actual peer与同一connection，foreign proof不能给same-key另一个connection附保留；只允许一个key-free backend session/dial reservation，duplicate Busy。它不保管DEK／密码，不授予认证／SecretRef使用。`NativeIoCompletion`只有watch boolean，不能延长network owner／reservation／actor／thread；取消或丢弃回执不释放实际I/O保留，sender孤立但未完成返回错误，不是成功。

RevocableIo内层显式Option：销毁时先close证据、标记不再可附新保留，短锁内take后在锁外**先drop owned inner流，后drop保留，最后发送完成**。不能在Rust Drop body先释放保留再等隐式字段Drop。可阻塞内层析构／Runtime shutdown两反例确认保留和完成不会早释放；实际TCP owner drop及远端EOF确认receipt完成和pin消失，foreign/duplicate反例也通过。18 SSH all-features、2 NoDebug doc、strict lint、原真实SSH、上层source/page/epoch/peer auth0 probe和check通过；native_io／peer_io Astrolabe Exact。

这里只确认owned stream销毁及保留dispose，**不等同russh session task join、远端进程退出、Actor drain、真实路由压力或三平台验收**。上层session准入还须采用自己的registered completion／回收事务；不要把Vault单job保留不加区分地挂到整条长期SSH连接。真实credential-use supervisor仍未开放，source point receipt不能替代最终使用准入。

## 网络对端


`pulse-ssh::peer::PinnedTransport`只接可信Rust后台选择的canonical HostTarget、backend已校验own/system pin源、owned route stream与client config。缺来源不作空、完整pin预算沿用128；own优先／changed不降级／未知不TOFU，host certificate仍拒绝。仅实际russh0.63.3握手callback验证presented public key后可取`VerifiedPeer`，纯verify_host_key成功、saved ID和unlocked Vault不能mint或替代它。

Proof无外部构造／Debug/serde，持weak connection状态与原key claim Arc；不保网络任务。foreign连接即使目标/公钥相同也拒绝；相同key的rekey保持claim，另一个已允许key换claim使旧proof失效。owner drop、peer disconnect/handler析构或poison也失效。此处只证明对端，不授予任何身份、SecretRef或目标使用；client低层handle只供可信Rust适配器在proof与当前connection再次匹配后取用，消费还须独立page/source/epoch/use准入。

真实取消反例：已有SSH ID和KEX packet，外层future abort完成却在2s内收不到owned流EOF，SDK内部任务仍持流。增加RevocableIo于spawn之前：共享closed flag与两有界read/write waker；取消/原5s timeout使pending I/O醒来返回ConnectionAborted，真实流生命周期/其携带资源保至实际drop。闭合/替换waker均在短mutex外drop/wake，poison释放authority guard后再关闭，不以bool撤销当实际资源join。已开始写入不能回滚、不自动重试。不是应用退出/真实路由压力/连接复用HOL或跨平台完成。

6peer针对性反例通过：callback重key/poison/drop；缺源在SSH ID之前拒绝；实际TCP wrong key pre-auth0、两same-key连接隔离及fixed caller明确auth1（仅测试非Vault授权）、远端shutdown且owner尚存的proof/handle失效/server active0；SDK已spawn握手后的caller abort真实EOF；原5s native timeout真实EOF；两个pending read/write唤醒并在callback重入中证明两短mutex均空闲。callback模拟rekey不当作真实网络rekey矩阵。

15项all-feature/managed-store与1项NoDebug doc、严格lint、mise check、原真实SSH probe及默认/工作区desktop normal tree不含pulse-ssh/russh通过；Astrolabe peer/peer_io Exact，早期symbol stale不计。移用原workspace Tokio到SSH normal依赖，无新包/版本；store仍不依赖网络层。same-WAL host/identity/Vault/trust捕获已补，见下节；显式后端目标密码动作与 weak原epoch/page/完整源/实际peer联合边界已受控验证（probe-ssh-password-use）；普通auth0预备回执仍不授予使用。实际合成私库密文经已确认DEK借给上层协议sink，正确TCP密码认证auth1并关闭/排空；错/未知pin pre-auth0、源修改、实际错target及换页均拒绝。native admission只保key-free token/weakepoch，不保DEK/controller或明文；NativeIoCompletion仍不等于SDK join。历史auth bool不是重用连接/session能力，敏感值不回到调用方/SQL/IPC，参见vault-persistence.md。首次指纹审批与 Linux 实窗见前文；独立会话动作与实际 loopback SSH→根 actor 移交已受控验证（probe-ssh-owned-session），不能由旧 auth bool 升级；新 close_and_join 分别等待 SDK task 与 owned stream，只有资源回收含义，非远端正常退出。换页保持连接、EOF及挂起读取关闭通过，固定合成PTY／拒绝resize不是OS远端PTY或GUI；生产根集合与 TermView 接入仍待完成。协议无关原生根所有权已通过 actor/latch 基础回归，见 [protocol-session-ownership.md](protocol-session-ownership.md)，不得把现有 drain-only auth bool 升级成连接能力。产品 SSH 会话入口、明文读取IPC、用户SSH文件、钥匙串、三平台、审计8项与签名边界不变。
