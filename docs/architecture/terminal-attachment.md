# 终端重连事务 v1（已实现的基础，非生产 attach）

后端仍唯一权威；xterm 是可丢弃的显示缓存。需要准确原始内容的搜索、复制、导出应读取后端，前端选区/滚动/链接状态须允许重建。已实现窗口无关的原子诊断lease和有界输出背压；窗口无关的协议额度/ACK监督已在pulse-app贯通；注册层已有可信window token、owner授权和generation撤销；真实Tauri window事件/权限适配已通过fixed-helper验证；任意用户会话生产接口与多视图策略仍待实现。

## 一个流，一个事务

`pulse-ipc::attach` 和 `ui/src/ipc/attachment.ts` 定义 Begin → History* → Restore+ → Commit → (Data | Checkpoint | Marker | RepairBegin RepairBytes+ RepairCommit)* → End，全部使用 FrameKind::Attachment=8，同一有序二进制通道，同一 CreditWindow。版本1的20字节 envelope 包含 version/part/reserved/generation/offset；Begin 28字节为columns:u16、rows:u16、revision:u64、parsed_bytes:u64、history_len:u32、restore_len:u32，全部小端。

- generation非零，由会话绑定层分配，不是parsed_bytes。ACK必须同时匹配generation和在途sequence；旧绑定ACK不释放新绑定额度。现有32位sequence遵守CreditWindow窗口规则。
- revision和parsed_bytes在JS中用bigint，不转number。parsed_bytes只表示后端原始输出解析边界，不是ACK，不是转码后的UTF-8字节数。本版Data的offset按发送字节递增，尚不覆盖编码转换后的原始/UTF-8双坐标，不能用于非UTF-8会话的生产边界。
- 片段body≤64KiB、history≤4MiB、restore≤1MiB。History/Restore section-local offset连续，Data从Begin.parsed_bytes开始连续。乱序、长度不符、未知版本/flag和未Commit的Data终止当前事务，没有部分渲染或丢字节后继续。
- sender无额度时phase/offset不变。已经生成的帧必须发送或取消整个generation，不能跳过。receiver先按声明长度有界暂存，Commit收到完整内容后才返回ready。

## 原子live诊断lease

`diagnostic_subscribe` 在同一actor回合捕获history/screen并安装唯一有界订阅。先预留8条队列容量再读取/解析transport，Data带start、原始bytes及after-boundary；full时停止解析和transport读取，不丢块，控制/close仍公平。无subscriber时仍独立解析。普通Data每条≤64KiB；此外每lease最多一份≤1MiB精确修复跨queue/driver持有至修复Commit ACK。修复在queue占一条容量，driver持有修复时queue可再存8条Data。不含driver/current/native块、grid和allocator开销。

- viewer输入采用`SessionRegistry::input_bound(owner,id,generation,bytes)`→AttachmentHandle→actor `input_for_lease`：进入时核对runtime owner/current generation，actor回合同步核对当前lease、取消watch、receiver仍存活及调用reply未放弃；resize/pending replies或满transport队列明确Busy。old generation在registry拒绝，旧lease在actor拒绝，caller cancel在actor接受前拒绝。返回前再核对current binding，但不能撤销已经准入的native input，因此成功只是有界admission，不是物理写入回执；不确定错误/超时绝不自动重发。低率控制面仍按字节预算约束，与output ACK不是同一个方向。
- 有限DOM探针的InputQueue单消息≤1KiB UTF8、最多32项/8KiB包含活动IPC，按FIFO single-flight发送；空/孤立surrogate/超限拒绝，不替换非法文本。一秒不确定截止、取消、native错误或overload拒绝所有pending并停止后续dispatch；晚成功不drain新输入、不重试。host撤销/输入失败调用只撤销current generation的detach；已准入的输入无法回滚。paste阻止，仅合成固定键盘/composition样例，正式粘贴确认/IME/user shell仍待实现。

- viewer尺寸采用`resize_bound(owner,id,generation,size)`→AttachmentHandle→actor `resize_for_lease`：actor回合核对当前lease/取消/放弃请求，pending resize拒绝。同尺寸直接成功，不写native、不改revision、不撤销lease。不同尺寸只有native ACK成功才改grid/state、发Reconfigured并终止旧stream；native失败维持旧grid/lease，未ACK5秒失败关闭原有会话监督不变。已准入的native resize不可回滚；取消/不确定结果不能自动重发。resize本来就终止旧generation，因此registry成功回执只重检owner而不要求已被撤销的旧binding仍相等。新attach必须精确恢复并分配新generation。
- fixed DOM初始fit采用public DOM screen尺寸测cell、host content box（不含padding），grid向下取整，低于2×2/hidden/nonfinite不请求，最多160×60。50ms去抖、single-flight、6秒有限不确定截止，不optimistic调用term.resize；新generation负责后续测量。finite relay最多对3次Reconfigured重捕获；此外只有显式pause票据匹配的Detached允许一次resume重捕获，普通错误不得盲重连。新Begin作废旧consumer/decoder/renderer，旧receipt回调按自己的generation丢弃；恢复尺寸及原seed确认后才继续query/repair/input/EOF。已验证3次受控容器width/height变化，不等于真实系统窗口拖动或跨屏DPI完成。

- 有限visibility gate只是后端小元数据（不是terminal mutex/前端capability）：pause须runtime owner/nonce/current generation检查及registry.detach成功后才记录generation票据；重复pause、wrong generation、未pause或重复resume拒绝。gate在短锁内完成detach/票据发布，防止driver Detached早于票据可见；watch注册再查票据，早到resume不丢失。只有该票据匹配的Detached才在5秒有限期限内等显式resume、验证owner/session仍存活后精确重新capture；非显式取消/ACK timeout/错误不自动重连。隐藏时仅撤销view，Rust grid/history继续更新，terminal session不关闭；finite helper的最终清理由probe guard负责，与产品pause语义区分。
- finite native hide/show验证只能由当前paused generation单次准入。先Tauri runtime确认当前窗口可见且非minimized，安装同incarnation恢复guard，再hide；有限2秒内回读不可见，再让backend产生隐藏期输出/100ms等待，show后2秒内回读可见且非minimized，session仍Running才发native收据。native_inflight期间resume拒绝；错误/取消guard仅尝试show同owner window，绝不自动resume、重试native操作或重新打开已销毁窗口。报告nativeWindowChecked由后台gate记录，不能由客户端flag替代。
- Destroyed回调先释放windows元数据锁，再关闭当前owner的VisibilityGate并registry.close_window：closed永久拒绝pause/resume/native finish，watch唤醒已挂起等待者；关闭在wait注册前或早到resume之后也不能复活旧incarnation，metadata poison仍发布closed及watch通知。不把CloseRequested或Focused(false)误作销毁/隐藏。Tauri跨平台WindowEvent没有可靠的通用desktop minimize事件，此处native cycle是受控验收，不是生产自动可见性调度；Linux已有隔离Xvfb/Openbox的受控native最小化实测，用户触发事件适配与其它WM/平台仍待验收。

- frontend `SurfaceVisibility`是每generation的一次性可见性控制器，来源container/document/native按AND合并，DOM两值以原子snapshot更新且保留native来源，不把focus loss算hidden。收到hidden先同步retire旧ACK/input/idle/renderer，再single-flight pause；show早到也必须等pause成功；仅全部来源visible且pause收据成功才请求resume，并授权一个new Begin。每个IPC1.5秒不确定截止，错误/取消/timeout不自动重试；resume在途重新hidden则fail closed，不能拿旧generation反复pause。handoff成功后交给新controller；旧success/rejection/来源变化不会再操作新view。
- `bindSurfaceVisibility`仅订阅host.hidden的真实MutationObserver和document.visibilitychange，首测读取isConnected/hidden/visibilityState后一次提交snapshot，避免container.show+document.hide逐项更新导致transient resume。handoff/failed/AbortSignal移除observer/监听，DOM卸载由ViewHost的AbortSignal承担，不能声称任意ancestor CSS/disconnection全部被自动观测；production用户触发native minimize事件/长时间hidden恢复仍待验证。

- opt-in Linux `probe-window`在新Xvfb/Openbox下运行，CLI-only mode保持普通probe-channel不需要WM。native cycle仍要求current paused generation单次准入，mode=true改调用runtime.minimize，必须观察is_minimized=true而非!visible/Focused(false)；后台输出推进后unminimize/show，观察visible&&!minimized才finish receipt。NativeVisibility的Visible/Hidden/Minimized状态矩阵明确区分，restore guard仅同incarnation尝试unminimize/show不resume。nativeMinimizeChecked只能由mode&&成功backend gate得到，要求同时nativeWindowChecked与pauseCount=1；server及xtask把请求mode与DOM flag对照、headless总false，防hide报告冒充minimize。受控native调用不证明用户手动最小化的自动事件adapter。

- `NativeWatcher`按runtime登记的每window incarnation启动一个任务；Focused/Resized/ScaleFactorChanged只是hint，经watch合并后最多50/50/100ms有限采样，不持续idle polling。runtime getters在spawn_blocking中执行，单window一份owned semaphore permit随实际job存活；500ms观测timeout不取消GUI job、不释放其额度，后续query不能堆积blocking任务。query invalid/忙/异常发布invalid并终止observer，而非默默继续沿用旧visible；Destroyed/shutdown关闭observer，late commit拒绝。重复state不推进revision、重复revision不再次emit。
- typed `NativeVisibilitySample`只定向同webview_window label，incarnation随机独立UUID与u32递增revision是投影metadata、非控制capability。`probe_session_native_snapshot`按runtime caller+owner nonce验证，采样后再验证WindowToken。前端先typed listen再snapshot，最多4个身份的最新revision缓冲（不可被旧/foreign样例覆盖，满则fail closed），之后NativeVisibilityFilter忽略旧身份/重复/倒序；invalid样例作废view，不fabricate可见。实际native-only probe不先隐藏host，也不绑定页面hidden来源：原生false独立驱动controller pause，原生restore驱动resume；窗口动作仍由受控command发起，不声称人工点击标题栏按钮或全平台事件覆盖完成。

- finite `probe_session_native_cycle`先runtime owner+nonce+SessionRegistry.check_binding只读核对current generation，再gate.reserve_external单次预留（不是pause/lease reservation/capability）。command只hide/minimize并最长2秒等event-driven frontend pause收据，绝不自己detach。gate.pause必须generation匹配且registry.detach成功才发布票据，external预留使native_inflight同时为true，early resume拒绝；watch订阅后再看状态防早pause丢失。close唤醒未pause的等待者并永久拒绝后续动作，失败不自动rollback/retry。check_binding不消费信用/发输入，也不保证未来仍current；后续pause/actor仍各自验证。
- native-only测试容器host.hidden始终false且不绑定document隐藏来源，只有定向native false才能同步retire并请求pause；native restored驱动resume callback，但callback先等待原生动作Promise（visible可能在finish_native之前通知）再核对hidden新增输出boundary与旧Channel无帧，随后授权唯一new Begin并resume，避免提前IPC被native_inflight拒绝。backend nativeOnlyChecked仅external reserve+native收据成功才true，DOM nativePauseChecked仅实际native false且host非hidden退役才true；server/xtask均要求DOM/backend证据，headlessfalse。受控原生动作不等于human按钮或无hint/长期隐藏全平台验收。

- opt-in Linux probe-window-external复用private Xvfb/Openbox、native-only pause gate与readback，但窗口动作由独立xdotool进程完成。测试wrapper创建077私有临时dir，只向直系app子进程传路径，appCLI选定模式后atomic写固定minimize/restore:canonical-generation marker；controller只按该app PID选择唯一visible XID、再核对getwindowpid，拒绝歧义/非法动作/未请求restore，每次动作timeout2s。成功正常路径应用不调用minimize/unminimize/show；失败恢复guard仍允许同incarnation恢复，不能计为成功证据。server与xtask核对externalWmChecked mode，且须依赖nativeOnly+minimize真实收据，headlessfalse；xtask强制4条两轮同generation动作配对及递增/有界规范。外部WM消息不是人工点击titlebar的证明。

- `SurfaceGeneration`只在显式resize/resume许可后允许更大的Begin（seq0），旧generation帧在触发activity/ACK前忽略，重复/未请求Begin拒绝；metadata使用共享attachmentEnvelope验证而非第二套codec。每个异步consumer job以consumer identity守卫：旧success不能观测新idle状态，旧error/取消不能detach新view。八项TS回归含U64>2^53精度和late callback错误；精确snapshot/serializer门禁与CreditWindow不变。

- lease_id是actor-local单调serial，不是全应用generation或授权。成功新capture替换旧lease；订阅返回须同时取得capture结果与actor安装完成的commit收据，不能在撤销旧lease前暴露新subscription；捕获失败或actor处理前取消保留旧lease。actor提交后取消已经发生替换，旧lease不自动回滚，必须重新attach。旧renderer及旧排队消息不能复用到新generation。
- watch StreamStatus提供无丢失终止原因和boundary：native resize成功、palette/cellpixels改变会Reconfigured；pending resize拒绝新lease，失败保持旧lease。Receiver放弃/close会Detached，不关闭registry session handle。
- sync deadline的零字节state推进用Fence，映射同stream Part::Checkpoint=6；offset必须等于live cursor，body是little-endian u64 revision且不回退，使用同generation/order/credits。Consumer要求sink处理完成才ACK，缺实现会fail closed；checkpoint本身不证明hidden/parser/history一致。
- 按需只读边界用Part::Marker=8：相同8字节revision/offset/order/credit，但与Part6语义不同。它不feed VT、不结束同步更新、不finish UTF-8。Consumer要求显式sink.marker回调完成才ACK，缺实现fail closed。actor验证当前lease_id（非授权generation），有空队列槽才接纳；旧serial、取消、active BSU、pending resize和满队列不能注入新lease。registry校验window owner/current generation后再请求，返回前重新校验；换绑或取消不能重置每session限频。
- 同边界精确修复采用Part::RepairBegin=9（offset=current live cursor，12字节revision:u64 + restore_len:u32）、RepairBytes=10（section-local连续offset，非空≤64KiB）、RepairCommit=11（offset=current live cursor，空body）。原始Data offset不推进；receiver≤1MiB暂存，Commit前不渲染，拒绝期间插入Data/End/另一Repair。Consumer必须显式支持repair，Commit等待decoder重置/完整VT replay及callback后才ACK；缺实现、取消或失败作废整个generation。
- `request_repair`验证owner/generation，500ms限频、250ms截止。actor在同一回合验证当前lease、全部SessionState及binding取消，重新计算摘要：Equal不排帧，Stale不排帧，Mismatch只在精确serializer无gap/approximation且queue有槽时排Repair。Queued只是admission，不是渲染或摘要确认；控制返回之后actor可继续推进，后续Data仍在Repair之后发送。风险提示不清除。single-repair semaphore持有至Commit ACK，防止8份1MiB排队；撤销/失败释放permit，不关闭会话。
- 修复序列由`repair_screen_bounded`生成：CAN→2026h→screen-only replay→2026l→pending CSI/UTF8。结束同步必须先于pending尾部；sink丢弃旧stream decoder，不能调用清history的terminal.reset。active BSU、非行尾pending wrap、不可编码hidden cursor等明确延期；不强迫提前flush或近似重绘。
- genuine EOF在actor处理finish后停止输出；driver排空剩余Data/Fence/Marker/Repair，再发送Part::End=7，offset必须等于最终parsed_bytes，8字节body为revision且不回退。TS consumer要求sink.finish完成decoder尾部及render callback后才ACK；没有finish实现明确拒绝。End后拒绝任何当前generation的Data/Checkpoint/Marker/再次End。取消、Failed或resize不发送假EOF，必须丢弃旧surface而非finish它。

### 有界ACK监督driver（pulse-app）

`attachment::attach`验证config及generation后原子订阅，返回wire receiver、cloneable ACK/detach handle和监督task。bootstrap数组移动而非复制，使用同一AttachmentSender/CreditWindow发送history/restore/live/Fence/Marker/Repair/End；不读取无界actor输出。单绑定最多8个在途帧、1MiB credit载荷、一个待发送帧、一个分片中的actor块，加8×64KiB Data lease及单份≤1MiB修复、现有native/actor预算；这些是代码载荷界限，不是整体RSS。receiver交给Channel后也不能无限缓存/旁路发送。

- 帧生成即预留额度，入wire队列后才可ACK；序号、generation和累计确认按原CreditWindow校验。未发出/duplicate/stale ACK会fail closed，ACK排队成功仅是admission，不代表已经释放额度。绑定层仍须将ACK端点授权给正确window。
- oldest outstanding帧采用固定1ms..30s deadline（默认5s）；pending未入队帧同样有deadline。新数据和新ACK不续期未确认的旧帧。无额度停止发送，lease满时解析/native读链也被背压。
- detach、丢弃receiver/task、超时或invalid ACK释放本lease，不调用SessionHandle.close；Replaced/Reconfigured/Failed/Closed撤销整个generation。调用方必须观察join错误并取消所有已经转交的旧wire及surface，driver不能撤回已经开始的xterm write。
- 正常End的ACK也必须收到，join才返回成功及final boundary/帧数/credit峰值；这只是可信consumer提交的ACK证明，不证明用户看见画面。一个诊断订阅、UTF-8坐标和调用方分配generation的限制不变，尚非窗口registry API。


### 应用注册层（pulse-app::registry）

- `SessionRegistry` 管理metadata，不锁Term。32个window incarnation、64个session和最多64个正在监督/取消中的binding task；每session capture用try-lock拒绝并发，不排无限等待队列。短std Mutex只保护索引，跨await不持有；注册者仍必须持有/监督native SessionTask或spawn_local receipt。
- `WindowToken` 是可信runtime adapter在window创建时取得的不可序列化能力；跨registry/重新创建均不复用。不能把frontend自报label、windowId或sessionId当owner。公开sessionId/generation应在JS控制面序列化为十进制string，binary仍保留u64。
- 一个application使用一个registry；注册相同actor两次被拒绝，身份由私有command channel比较而非公开watch receiver，避免两个id绕过owner与capture gate。注册层对input/resize/capture/state/resolve/ACK/detach/close先检查owner，未知session和无权限同样Forbidden。generation由backend分配，不复用失败capture的编号；旧ACK/detach在进入driver前返回Stale，不会撤销或释放新binding。晚到monitor完成仅能删除同generation条目。
- window关闭撤销token和其view binding，不close session；旧token和同label的新window都不能继承session权限。孤立session仍由application持有，只有显式shutdown/session close关闭；安全的跨window迁移/恢复授权尚未实现。cap限制避免无限遗留，但这不是完整workspace迁移UX。
- close/shutdown与await capture并发时，capture返回后再次检查owner，禁止死window复活view。成功capture提交后取消可能已替换旧lease；没有snapshot rollback。old wire/surface仍须由adapter丢弃，registry不能撤回已执行的xterm write。
- native 2MiB压力探针已把input/ACK通过registry授权。`probe-channel`进一步接通Tauri实际调用者WebviewWindow、local origin检查（无外部host/port/credentials）、runtime `Destroyed`与application `Exit`事件，owner不来自frontend label/token。
- 新增四个developer-only typed命令：probe_session、probe_session_go、probe_session_ack、probe_session_peer。仅运行固定finite Rust helper，不启动任意shell；Raw Channel载荷仍为二进制。真实WebView中的headless xterm callback完成之后才ACK，终态从joined actor的visible projection生成digest，不向已结束actor发命令。
- 真实隐藏peer window尝试跨owner input/ACK必须被拒绝；销毁后token撤销而其native session仍Running，然后由可信registrar显式close/join。`reap_closed`只清理已终态actor，Running返回Busy，不是frontend command。失败时guard请求撤销peer/关闭helper，后台等候终态最多5秒；固定child另有40秒硬截止。超时不能伪装成完成join。
- 自动连续运行两次，重建相同peer label，backend generation递增；新run拒绝旧nonce、旧generation及错误generation ACK。两轮均有原生EOF、截断UTF-8排空和同边界digest证据；这仍不验证DOM绘制/selection或生产跨窗口迁移。


## 解析状态、历史与显示

`SessionHandle::diagnostic_bootstrap` 在actor同一命令序列点捕获recent history和screen-only restore，返回SessionState边界。拒绝已知近似、RestoreGap及额度错误，取消且未开始的请求跳过；不会主动更改解析器。**这不是实时订阅**：调用后数据仍会推进，driver还需要原子冻结/有界留存边界之后的输出，不能直接将此诊断结果用作生产detach/attach。

历史使用主屏，备用屏激活时用只读inactive getter。最近≤2000个物理行，截断若落在软换行逻辑行中则向后缩减到下一hard边界；不为了装满2000行伪造第一行语义。极长软换行占满预算时可以没有可重放历史；深历史另行分页/search。历史颜色/属性及wrap保留，不能编码的wide/leading spacer显式HistoryUnsupported，而非丢内容。

完整fresh restore可用RIS；history后和已有缓存重新同步必须用`restore_screen_bounded`，不含RIS，使用逐行EL擦除viewport及明确模式/charset/tabstop/scroll margin。xterm每buffer有独立tabstop和scroll margin，主屏/备用屏都建立状态，不能只改active。screen-only不是针对任意旧xterm状态的完整兼容保证，已有测试覆盖模式/历史/同尺寸及8个真实软件input/resize续流场景；仍需验证生产显示同步。禁用ED2：它会把alacritty主屏推进history，原恢复多出一行空历史，导致vim后续grow时隐藏光标偏移。新增Rust回归确认EL不制造历史且保留原history。原始.vt不含外部resize事件，复现以software.json的尺寸转换元数据为准。

压缩器省略逐行EL之后的默认空白尾部、相同pen连续运行不重复SGR/链接，每段只定位一次。所有SGR也经Emitter预算，修复颜色helper直接写buf绕过限制的问题。最坏每格不同属性仍约160KB，不宣称所有样例10倍压缩。

## 前端callback监督

`AttachmentConsumer` 以有界队列串行处理帧，包含active帧计数；ready先reset fresh surface → history → screen，live保持同一UTF-8 decoder（不能在每片段或section结束时finish）。staged拷贝入有界暂存后可ACK，Commit/Data等待渲染callback才ACK。队列溢出取消generation，不丢帧。取消中断等待、不发送迟到ACK；native/xterm已开始的write无法撤回，旧surface须丢弃，绝不能给新generation复用。

sink仍为适配接口，不是Tauri listener或窗口view管理器。它负责有界callback写入、AbortSignal和query抑制。receiver暂存≤5MiB + queued wire上限 + driver/sink自有状态，不能把5MiB当全应用RSS上限。并发/已完成快照的保留须由调用方限制。

## 验证与剩余工作

`mise run terminal-api-check` 在workspace之外实际编译default-feature消费端：正常TerminalProbe可用，EchoTransport及screen_replay无法调用；xtask显式开启validation。

`mise run probe-attachment`：Rust构造真实history/state，用7字节片段、每次1个额度窗口、2字节live发送至TS Receiver/Consumer和真实xterm headless；检查history每格颜色/宽度/属性/wrap，UTF-8/CSI跨restore-live续流、无query泄漏及callback ACK。5个案例通过，新增supervised driver的End与截断UTF-8尾部排空；包括actor原子订阅、部分UTF-8、live备用屏退出、BSU超时Fence映射为Checkpoint，并在sink处理时严格比较后端state才ACK。报表中的sender ACK是受控模拟，callback监督已在TS真实headless执行；它不是实际网络/Channel交互，也不是WebView/窗口重连。真实PTY lease压力另由probe-local-session检查。

已新增后端透明Handler风险位、REP边界观察及visible digest v1（FNV-1a64，非安全认证）；actor diagnostic_digest/verify_screen只在同SessionState且摘要相同时清提示，不跨boundary丢风险。矩阵覆盖372样例，18个已知分歧全被提示，11个可编码帧screen-only修复后完全一致，7个明确不可编码/延期。新增真实修复探针发现宽head缺spacer及origin域外cursor在xterm不可表示，已修复此前“精确”误报。风险位为保守提示，可有false positive；不能每条输出自动重绘。摘要不覆盖hidden/parser/history、链接、全部样式和palette渲染；不能由digest一致宣称完整状态一致。`SessionRegistry::compare_visible`已提供owner/generation前后校验的只读摘要比较：每session single-flight、500ms限频（包括失败/取消/换绑）、250ms actor截止。旧boundary返回Stale；错误/timeout不是Mismatch。它不调用会清risk的Verify，因此撤销后仍在队列中的检查也不能修改新绑定风险位。Equal只是采样证据，不是后续应用restore的权限。

前端`IdleComparison`对已知Commit/Checkpoint/Marker边界在600ms空闲后检查，最多3次尝试；隐藏/新输出入队/销毁/换generation作废迟到结果，350ms证据期限。不结束的IPC占住single-flight，而非超时后不断生成新IPC。Mismatch仅通知调用方，不重绘、不绕过精确恢复门禁。普通Data没有完整revision，`activity()`须清除已知边界，不能用offset猜revision；已新增同通道只读Marker请求：新Data入队后600ms空闲请求Marker（最多3次），IPC仅证明admission；观察有序Marker回调且没有后来入队Data时建立边界，再600ms后比较。registry请求采用500ms限频/250ms截止，actor lease身份和队列容量双重校验；active BSU/pending resize/full queue明确Busy，不旁路等待。真实WebView已在fixed helper半条CSI的输出暂停点自动请求Marker、比较并继续到EOF。已实现同generation/boundary原子compare→repair授权及受控自动精确重绘：真实WebView在半CSI暂停点损坏renderer，Mismatch后请求Repair，Commit渲染/ACK后再次采样Equal才续流。自动修复在该probe最多一次，失败明确退出，不不断retry重绘。已在固定40×12的只读DOM renderer复用该链路，逐行核对绘制文本/有序ACK/历史及EOF，见validation.md DOM节。生产DOM绑定、普遍用户会话策略/频率与失败UX仍待接入。

仍需：任意用户会话的生产命令/权限与DOM renderer生命周期集成、可见view自动idle同步限频、跨窗口迁移与重新授权、Windows ConPTY实测、更丰富真实录制和sync频率测量、深历史分页、reflow与滚动/selection UX。当前真实窗口测试仅执行固定finite helper，不能宣称生产attach或S2/S11整体验收。原依赖与发布门禁不变。
