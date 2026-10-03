# 终端重连事务 v1（已实现的基础，非生产 attach）

后端仍唯一权威；xterm 是可丢弃的显示缓存。需要准确原始内容的搜索、复制、导出应读取后端，前端选区/滚动/链接状态须允许重建。已实现窗口无关的原子诊断lease和有界输出背压；窗口无关的协议额度/ACK监督已在pulse-app贯通；生产窗口/权限绑定、多视图策略仍待实现。

## 一个流，一个事务

`pulse-ipc::attach` 和 `ui/src/ipc/attachment.ts` 定义 Begin → History* → Restore+ → Commit → (Data | Checkpoint)* → End，全部使用 FrameKind::Attachment=8，同一有序二进制通道，同一 CreditWindow。版本1的20字节 envelope 包含 version/part/reserved/generation/offset；Begin 28字节为columns:u16、rows:u16、revision:u64、parsed_bytes:u64、history_len:u32、restore_len:u32，全部小端。

- generation非零，由会话绑定层分配，不是parsed_bytes。ACK必须同时匹配generation和在途sequence；旧绑定ACK不释放新绑定额度。现有32位sequence遵守CreditWindow窗口规则。
- revision和parsed_bytes在JS中用bigint，不转number。parsed_bytes只表示后端原始输出解析边界，不是ACK，不是转码后的UTF-8字节数。本版Data的offset按发送字节递增，尚不覆盖编码转换后的原始/UTF-8双坐标，不能用于非UTF-8会话的生产边界。
- 片段body≤64KiB、history≤4MiB、restore≤1MiB。History/Restore section-local offset连续，Data从Begin.parsed_bytes开始连续。乱序、长度不符、未知版本/flag和未Commit的Data终止当前事务，没有部分渲染或丢字节后继续。
- sender无额度时phase/offset不变。已经生成的帧必须发送或取消整个generation，不能跳过。receiver先按声明长度有界暂存，Commit收到完整内容后才返回ready。

## 原子live诊断lease

`diagnostic_subscribe` 在同一actor回合捕获history/screen并安装唯一有界订阅。先预留8条队列容量再读取/解析transport，Data带start、原始bytes及after-boundary；full时停止解析和transport读取，不丢块，控制/close仍公平。无subscriber时仍独立解析。8×64KiB是lease载荷预算，不含driver/current/native块、grid和allocator开销。

- lease_id是actor-local单调serial，不是全应用generation或授权。成功新capture替换旧lease；取消/失败capture保留旧lease。旧renderer及旧排队消息不能复用到新generation。
- watch StreamStatus提供无丢失终止原因和boundary：native resize成功、palette/cellpixels改变会Reconfigured；pending resize拒绝新lease，失败保持旧lease。Receiver放弃/close会Detached，不关闭registry session handle。
- sync deadline的零字节state推进用Fence，映射同stream Part::Checkpoint=6；offset必须等于live cursor，body是little-endian u64 revision且不回退，使用同generation/order/credits。Consumer要求sink处理完成才ACK，缺实现会fail closed；checkpoint本身不证明hidden/parser/history一致。
- genuine EOF在actor处理finish后停止输出；driver排空剩余Data/Fence，再发送Part::End=7，offset必须等于最终parsed_bytes，8字节body为revision且不回退。TS consumer要求sink.finish完成decoder尾部及render callback后才ACK；没有finish实现明确拒绝。End后拒绝任何当前generation的Data/Checkpoint/再次End。取消、Failed或resize不发送假EOF，必须丢弃旧surface而非finish它。

### 有界ACK监督driver（pulse-app）

`attachment::attach`验证config及generation后原子订阅，返回wire receiver、cloneable ACK/detach handle和监督task。bootstrap数组移动而非复制，使用同一AttachmentSender/CreditWindow发送history/restore/live/Fence/End；不读取无界actor输出。单绑定最多8个在途帧、1MiB credit载荷、一个待发送帧、一个分片中的actor块，加8×64KiB lease及现有native/actor预算；这些是代码载荷界限，不是整体RSS。receiver交给Channel后也不能无限缓存/旁路发送。

- 帧生成即预留额度，入wire队列后才可ACK；序号、generation和累计确认按原CreditWindow校验。未发出/duplicate/stale ACK会fail closed，ACK排队成功仅是admission，不代表已经释放额度。绑定层仍须将ACK端点授权给正确window。
- oldest outstanding帧采用固定1ms..30s deadline（默认5s）；pending未入队帧同样有deadline。新数据和新ACK不续期未确认的旧帧。无额度停止发送，lease满时解析/native读链也被背压。
- detach、丢弃receiver/task、超时或invalid ACK释放本lease，不调用SessionHandle.close；Replaced/Reconfigured/Failed/Closed撤销整个generation。调用方必须观察join错误并取消所有已经转交的旧wire及surface，driver不能撤回已经开始的xterm write。
- 正常End的ACK也必须收到，join才返回成功及final boundary/帧数/credit峰值；这只是可信consumer提交的ACK证明，不证明用户看见画面。一个诊断订阅、UTF-8坐标和调用方分配generation的限制不变，尚非窗口registry API。


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

已新增后端透明Handler风险位、REP边界观察及visible digest v1（FNV-1a64，非安全认证）；actor diagnostic_digest/verify_screen只在同SessionState且摘要相同时清提示，不跨boundary丢风险。矩阵覆盖372样例，18个已知分歧全被提示，11个可编码帧screen-only修复后完全一致，7个明确不可编码/延期。新增真实修复探针发现宽head缺spacer及origin域外cursor在xterm不可表示，已修复此前“精确”误报。风险位为保守提示，可有false positive；不能每条输出自动重绘。摘要不覆盖hidden/parser/history、链接、全部样式和palette渲染；不能由digest一致宣称完整状态一致。可见视图idle/resize/reattach的限频调度和generation校验仍须view driver接入。

仍需：生产generation/权限/渲染器生命周期集成、真实WebView callback ACK至native链路组合实测、Windows ConPTY实测、更丰富真实录制和sync频率测量、深历史分页、reflow与滚动/selection UX。原依赖与发布门禁不变。
