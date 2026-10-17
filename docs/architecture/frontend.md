# 前端架构（React）

> 前置阅读：[overview.md](./overview.md)。
> 前端负责界面外壳和各类视图。**终端和桌面的数据不进入 React 的渲染流程**，这是整个前端最重要的一条规则。

## 1. 技术栈

| 类别 | 选型 | 说明 |
|---|---|---|
| 框架 | React 19 + TypeScript 5（strict） | 启用 React Compiler，自动减少不必要的重新渲染 |
| 构建 | Vite | 按路由和功能拆分代码，Monaco 和 noVNC 用到时才加载 |
| 样式 | Tailwind CSS 4 + CSS 变量（设计 token） | 主题切换只改变量，不重新渲染 React 组件 |
| 色板 | @radix-ui/colors 3（MIT） | 界面配色的来源，见 §10.1 |
| 基础组件 | Radix Primitives + shadcn 风格的组件 | 组件代码复制进 `design/components`，由我们自己维护 |
| 状态 | Zustand | 组件只订阅自己用到的字段（selector），辅以 `subscribeWithSelector` |
| 后端请求缓存 | TanStack Query | 缓存 Vault 列表、目录列表等查询结果，收到后端事件时失效重取 |
| 列表 | TanStack Virtual | 主机列表、文件列表、传输队列都用虚拟滚动 |
| 分屏 | react-resizable-panels | 嵌套的分屏布局和拖动调整 |
| 拖拽 | dnd-kit | 标签排序、标签拖到分屏、主机拖到分组 |
| 命令面板 | cmdk | — |
| 终端显示 | @xterm/xterm 6 + webgl、web-links、search、image（P2）；项目共享 Unicode scalar provider | 只负责显示，不保存权威状态 |
| 远程桌面 | 自研 RemoteCanvas（WebGL2 + WebCodecs）；noVNC 1.7 | — |
| 编辑器 | Monaco（@monaco-editor/react，用到时加载） | — |
| 多语言 | react-i18next | 简体中文、英文 |
| 图标 | lucide-react | — |
| 测试 | Vitest、Testing Library、Playwright | — |
| 与后端的类型绑定 | tauri-specta 生成（备选 ts-rs） | 生成的代码放在 `ipc/gen/`，**不要手动修改** |

## 2. 目录结构

```
ui/src/
  app/              入口；按窗口类型（main/drop）选择根组件；Providers；全局错误边界
  shell/            窗口框架：TitleBar（拖拽区域、窗口控制按钮）、TabStrip、Sidebar、StatusBar、BackdropController
  workspace/        布局树模型与各种操作、PaneContainer、分屏、标签拖拽、焦点管理、广播选择
  viewhost/         管理视图容器的池（见 §5），在 React 组件树之外
  features/
    terminal/       TermView 控制器、按键处理、搜索栏、命令标记、历史查看器
    desktop/        RemoteCanvas、输入采集、扫描码表、工具栏、VncView（noVNC 适配）
    files/          双栏文件管理、传输队列、内置编辑器入口
    vault/          主机树和列表、主机和入口编辑器、身份与密钥、导入向导、快速连接
    tunnels/        端口转发规则与状态
    prompts/        连接过程中的弹窗：主机指纹、MFA、密码、证书确认、高风险操作确认
    palette/        命令面板
    settings/       按设置 schema 自动生成的设置页、快捷键编辑、主题编辑
  ipc/
    gen/            tauri-specta 生成的命令和类型
    frames.ts       二进制帧解码（与 pulse-ipc 共用测试样例）
    channel.ts      创建通道、分发数据、确认已处理的数据量
    events.ts       后端低频事件的订阅
    errors.ts       把错误码翻译成提示文字
    mock/           浏览器开发模式下的模拟后端
  stores/           workspace、sessions、settings、transfers、ui
  keybindings/      把按键组合解析成 action，处理优先级和冲突
  design/           设计 token、主题加载、组件库、图标
  i18n/
  lib/              与业务无关的工具函数
```

**依赖规则**（由 eslint 的 `import/no-restricted-paths` 检查）：
- `features/*` 之间不能互相引用，只能通过 `workspace`、`stores` 或 action 协作。
- 只有 `ipc/` 可以引用 `@tauri-apps/api`；组件中禁止直接调用 `invoke`。
- `viewhost/` 和各视图的控制器不依赖 React，是普通的 TS 类，这样可以单独做单元测试。

## 3. 状态管理

| Store / 缓存 | 内容 | 更新频率 | 是否保存 |
|---|---|---|---|
| `workspaceStore` | 当前窗口的标签、布局树、焦点窗格、广播选择 | 用户操作时 | 空闲时写入后端 `app_state`（去抖 1 秒），用于会话恢复 |
| `sessionStore` | 各会话的元数据：状态、标题、当前目录、未读标记、延迟 | 后端事件，已按 100ms 合并 | 否 |
| `settingsStore` | 后端计算好的设置快照（只读） | `settings.changed` 事件 | 由后端保存 |
| `transfersStore` | 传输队列及进度 | 通道数据，10Hz | 由后端保存 |
| `uiStore` | 打开的弹窗、命令面板、侧边栏宽度等 | 用户操作时 | 部分保存 |
| TanStack Query | Vault 查询、目录列表、主题列表 | 查询时；收到 `vault.changed` 等事件时失效重取 | 否 |

**性能规则**：
1. 终端字节、桌面帧、通道数据**禁止放进任何 store 或 React state**。
2. 频率高于 10Hz 的数值（帧率、吞吐、传输速度）只在状态栏和传输面板的叶子组件中订阅，或者在组件内用 `ref` 直接修改 DOM。
3. Zustand 的 selector 必须返回稳定的值；要返回对象时使用 `useShallow`。

## 4. 工作区模型

```ts
type PaneNode =
  | { type: 'split'; id: NodeId; dir: 'row' | 'column'; children: PaneNode[]; sizes: number[] }
  | { type: 'leaf'; id: PaneId; view: ViewRef };

type ViewRef =
  | { kind: 'terminal'; sessionId: SessionId }
  | { kind: 'rdp' | 'vnc'; sessionId: SessionId }
  | { kind: 'files'; sessionId: SessionId; localPath?: string }
  | { kind: 'page'; page: 'vault' | 'settings' | 'tunnels' | 'welcome' };

type Tab = { id: TabId; root: PaneNode; focusedPane: PaneId; title?: string; color?: string };
```

- 布局树的各种操作（分屏、关闭、移动、交换、平均分配、按方向寻找相邻窗格）都是**纯函数**，并有完整的单元测试。
- **标签在窗口之间移动**：由前端调用 `window_move_tab(tab, targetWindow)`。后端把这个标签的布局交给目标窗口，目标窗口重新关联各个会话并用快照恢复显示。会话始终不断开。
- **会话恢复**（WS-04）：启动时从 `app_state` 读取上次的布局。对本地会话，用保存的 profile 和目录重新打开；对远程会话，按设置自动重连，或显示一个“点击重连”的占位。

## 5. ViewHost：在 React 组件树之外管理视图

**实现状态（基础层）**：`features/terminal/view-host.ts`现有`GenerationViewHost`/`ViewScope`，每pane按后端Begin的u64 generation管理可丢弃surface，不是下方的完整DOM pool。begin只准许更大generation，先同步abort/退役旧scope；retire/父Abort/卸载按身份去重、倒序清理所有resource，一个disposer异常不跳过其它resource且当前host fail closed。异步factory在retire时调用方立即reject，仍监督尾部并dispose晚到对象；旧成功/失败/cleanup不能退役新scope。consumer、renderer/headless、query mask/onData、input queue与idle由scope登记；window visibility/native订阅是跨暂停恢复控制面，仍归pane lifetime。finite sample已接此基础层，每DOM3次resize+1次pool park恢复+1次native恢复共5次重建，主动重复old renderer.dispose验证新.xterm root不被清除。renderer dispose只移除自己root，不清空新host内容；数据仍不入React state。另已新增view-pool.ts的ViewHost与view-slot.tsx/ViewSlot：按window-local key复用稳定container/controller，React19 ref cleanup持有mount票据；park只撤销挂载票据，把container移到hidden+inert parking并同步通知setVisible(false)，controller lifetime保留。默认最多32份（含未结束async cleanup）、6份parked、5分钟TTL，LRU/TTL只淘汰parked。dispose可异步，1.5秒不确定截止不释放实际未settled任务的额度；错误async setVisible adapter也fail closed且保留尾部额度。主probe容器已用实际ViewSlot，factory不打开PTY；finite adapter已通过PaneVisibility把实际pool false/true同步传入当前SurfaceVisibility，停车期间pane lifetime不断开、Rust继续解析；未绑定控制面时false仍fail closed取消样例。新增ViewLease.resume保留单次parked claim，TTL/换挂载/evict后旧claim拒绝，ViewSlotOwner让React cleanup跟随同slot最新resume票据，不丢到旧ticket。生产通用TerminalViewController/长期隐藏仍待接入。33项池/票据测试覆盖StrictMode式mount→release→mount，但尚未跑开发React严格模式的完整系统会话循环。

**要解决的问题**：窗格在布局树中移动时（拖到另一个分屏、拆分、合并），React 会销毁并重建对应的 DOM，xterm、noVNC 和 WebGL 上下文也会跟着重建。

```ts
class ViewHost { // ui/src/features/terminal/view-pool.ts
  mount(key: string, slot: HTMLElement,
    factory: (element: HTMLElement, signal: AbortSignal) => PooledViewController): ViewLease;
  sweep(): void;   // TTL/LRU只淘汰parked；内部单次timer也触发sweep
  dispose(): void; // 不直接关闭backend session
}
interface ViewLease { element: HTMLElement; signal: AbortSignal; isCurrent(): boolean; release(): boolean; resume(slot: HTMLElement): ViewLease; }
interface PooledViewController {
  setVisible(visible: boolean): void; // 同步本地退役，controller内串行backend操作
  dispose(): void | Promise<void>;   // async尾部仍占用pool额度直到真实settled
}
```

- React侧`<ViewSlot pool={pool} viewKey=… factory={factory} />`用memoized React19 callback ref拿mount ticket，cleanup调用该ticket.release；旧cleanup不能按key撤销新挂载。稳定element/controller跨同document slot重排或parking复用，不因React换父节点重建controller。backend generation改变仍由GenerationViewHost重建surface，这是另一层。
- setVisible(false)必须同步停止本地input/render/ACK；异步backend pause/resume由controller内串行处理，不能直接给pool返回Promise。pool本身不发backend命令、不生成generation、不关session，淘汰只abort controller lifetime并调用frontend disposer。生产adapter须detach-only、准确new Begin恢复；finite probe adapter当前已按owner/generation有序pause/恢复；没有活跃controller绑定时park才取消样例，生产daemon/长时间恢复仍待接入。
- 已用ref callback及pool单测模拟StrictMode式mount/cleanup/remount，factory只调用一次；实际App的ViewSlot池两个DOM sample结束后要求主pool仅1entry/factoryCount=1。factory应只创建controller，不能因React mount直接重复open backend session；真实开发StrictMode/生产会话停车的整链仍待验收。

## 6. TermView（features/terminal）

**基础设置接入**：`product_settings.rs`只处理固定app-config目录中的settings.toml和三显示字段，opaque token强绑定后端Snapshot及原page Arc；目标、程序、参数和env不由此开放。原生blocking闭包持IO信用直到实际结束，退出Root等待；保存前／temp fsync后／install前再次核对原页执行准入，grant后在途install不undo，迟到receipt不发布／重试。`workspace-settings.ts`严格解码version/fields/default-user、安全字体名和范围，Modal epoch拒旧DOM与旧结果；恢复仅删除局部覆盖。工作区读取失败不以默认值覆盖文件或展示默认Shell。展示字体不属于VT协议，配置投影进入`SurfaceGeometry`，原Raw v1/v2和调色板/查询权威不变。保存供下次新视图或刷新应用，非外部watcher／即时多文件事务；Ended无法适配较小网格仍typed拒绝。\n

**当前生产工作区接入**：独立 `workspace-main.ts` 使用后端发现的 Profile、根会话目录和页面代际授权，Raw v2 Consumer→Surface→绘制 ACK→捕获确认→输入门禁；并接原生 resize、刷新、Ended只读恢复与退出排空。`workspace-tabs.ts` 为手动激活的 ARIA tablist，Arrow/Home/End 仅移动焦点，显式激活才切租约；旧 DOM 回调不能选择新目录。关闭当前会话后恢复存活项；sessionStorage 只保存当前窗口的选择偏好，必须与新页后端目录交叉校验，不是执行或会话所有权。Linux双真实PTY已验收，关闭通过scope拥有的原生modal默认保留，只有批准且当前页／目标未变才撤租约和调用root回收；取消保留显示和PTY、不重试未知结果。`workspace-layout.ts`已实现有界不可变的布局操作，每个`WorkspacePane`独立ViewHost、capture epoch与fit/observer，防同代际数值或旧End/错误回调互相退役；单显示面已迁移，`WorkspaceSplit`现可把两个backend目录中的已有会话以左右／上下布局并行呈现，`WorkspaceLayoutView`只重建布局wrapper，稳定leaf由各自控制器持有；比例stylesheet通过每页nonce且仅插入经模型校验的节点ID和finite比例，不用inline CSS。关闭按focus所属Session而非rootTab身份；切回rootTab释放双view而保两PTY。刷新当前回到选中root单显示面，当前双窗格支持20–80%有界range比例调整与Ctrl+Shift+方向导航（Ctrl+Alt被Linux Openbox占用，未更改WM配置）；布局重建复用稳定leaf，调整串行且实际fit后恢复原显示焦点，未确认／resize／退休资源拒focus，focus不增加输入许可/RPC。比例仅两live Ready可改，Ended小窗口重排未做而明确禁用。布局持久化／嵌套分屏UI／gutter拖动／最大化仍未接入。重命名、全局快捷键、跨平台／系统IME与完整产品DoD未完成。Unix Profile管理modal已接fixedkind＋hidden布尔写私库，程序／参数／环境仍backend控制；currentPage执行gate带IOcredit进actualSQLite请求，SQL完成先退credit再publish，executing/committed写不随retirement undo。仅隐藏/恢复而不关闭原PTY，刷新及新app私库重开已验证；未知结果只freshread不retry/反向写。完整默认Profile／参数env配置／删除恢复、WindowsACL与三平台待做。下面固定探针段为独立历史验证范围，不再代表生产入口完全不存在。

**当前实现边界（开发验证）**：`features/probe/dom-renderer.ts`已通过同一AttachmentConsumer接入初始40×12、128行history的只读xterm DOM。与headless共用Rust Unicode17 scalar、查询屏蔽、每流解码及repair；输出不进React state，React callback ref只注册容器，卸载作废lifetime/consumer并dispose旧surface。DOM ACK在parser callback和两次animation机会之后（1秒截止，隐藏/断开明确失败），不是compositor像素回执。动态stylesheet通过xterm公开`documentOverride`在创建时设置Tauri本地HTML style anchor的每页nonce；不改全局document或放开CSP unsafe-inline。无nonce时拒绝DOM启动。这里只验证固定helper，临时启用合成键盘/composition输入，经generation授权的有界队列回写，完成后恢复只读；paste在capture阶段阻止，未开放生产粘贴UX、link addon或user shell。输入队列单批≤1KiB、32项/8KiB（含活动IPC），一秒不确定超时/错误/取消作废且绝不自动重试；卸载或错误请求generation-scoped detach，不关闭registry会话。已在fixed helper中用ResizeObserver+DOM cell尺寸每generation重测，50ms去抖、160×60 cap、6秒不确定截止，连续完成3次受控容器尺寸变化（40×12→104×12→60×10→104×12），native ACK后重捕获而非optimistic调用term.resize。host现在有独立208px高度，样例临时改宽/高并恢复，observer风暴合并为一次native resize。每generation的SurfaceVisibility将container/document/native三种来源AND合并；DOM测量原子更新container/document并保留native来源，不把失焦当作hidden；bindSurfaceVisibility监听实际host.hidden属性及visibilitychange；后端NativeWatcher以Focused/Resized/ScaleFactorChanged作提示，有限50/50/100ms采样runtime visible/minimized，typed定向事件带每window独立incarnation与递增revision。前端先listen再owner授权snapshot，最多4个incarnation的最新样例做bootstrap buffer，旧/重复/乱序信号忽略；invalid回读fail closed。收到不可见通知先同步停用input/idle/consumer/renderer，再串行发owner/current generation授权的pause票据detach；native-only样例先调用有限原生动作入口（仅reserve窗口动作，不detach），Tauri hide/minimize后typed native false独立触发controller暂停；不先改host.hidden，也不绑定页面信号（其source保持true）。backend最长2秒等待controller pause票据后才产生隐藏期输出，由Rust负责100ms、show/unminimize及可见回读而非依赖hidden JS。Rust状态继续处理原生隐藏期间输出，native restored事件触发controller恢复，resume先等待同次native动作Promise收据与hidden期输出边界、确认无旧Channel帧再在5秒有限pause监督内请求resume，事先授权唯一新Begin，再按准确边界重捕获。每个pause/resume IPC有1.5秒不确定截止、无重试；early show不能跳过detach收据，resume在途再隐藏则fail closed。新generation接管后旧controller停止，observer/visibility监听在handoff/失败/abort时释放。新Begin须先有resize/resume许可；旧帧不算activity，旧consumer Promise成功/失败不操作新surface。已验证3次resize、一次容器隐藏及真实native hide/show；另有Linux隔离Xvfb/Openbox的CLI-only最小化模式，backend回读is_minimized=true后隐藏期输出、unminimize并回读可见再恢复，报告nativeMinimizeChecked由后台设置。新增nativeOnlyChecked/backend与nativePauseChecked/DOM证明原生信号作为唯一隐藏来源独立驱动pause→resume。另有probe-window-external：private Xvfb中独立xdotool进程按appPID匹配唯一窗口，外部WM最小化/激活；应用只发固定generation marker并readback，正常路径不调用minimize/unminimize/show，backend externalWmChecked及四动作日志要求通过。仍未验收人工点击标题栏按钮、任意ancestor CSS遮挡检测或应用全局可见性调度；DOM解绑须由ViewHost AbortSignal取消，不仅依赖observer；Destroyed立即关闭pause gate，待决/早到resume不能重新激活旧window；生产ViewHost、真实拖动/跨屏DPI/zoom、IME/selection与长期隐藏恢复策略仍待接入。

**生命周期**：

```
mount(el)
  → new Terminal({ scrollback: 2000, allowProposedApi: true, … })；加载 webgl、web-links addon；注册由 Rust 生成的 Unicode 17 scalar provider
  → fit 计算行列数 → ipc.term.attach(sessionId, viewId, cols, rows, channel)
  → channel.onFrame:
       Data → 每流 Utf8Stream 解码；非空完整标量写入 xterm；额度仍按原始帧字节/序号归还，不按 JS 字符串长度计算
       Snapshot → 使用后端 `restore_sequence`（见 terminal-restore-evaluation.md）；备用屏激活时主屏缺失的处理待确认，视口重放禁止用于 attach
       Title/Cwd/Bell/Mark → 更新 sessionStore；命令标记用 xterm decoration 显示
       Exit → 显示“进程已退出”，并提供重启 / 关闭按钮
onData(s)        → 同一微任务内的输入合并 → ipc.term.input(sessionId, bytes)（广播时发给多个会话）
ResizeObserver   → fit → 去抖 50ms → ipc.term.resize
setVisible(false)→ ipc.term.pause；setVisible(true) → ipc.term.resume（后端先发一帧 Snapshot）
dispose          → ipc.term.detach → term.dispose()
```

- **快捷键优先级**（在 `attachCustomKeyEventHandler` 中处理）：
  1. 用户配置的应用快捷键；
  2. Ctrl+C / Ctrl+V 的双重含义（有选区时复制）；
  3. 会话处于 win32-input-mode 时，由我们自己把按键编码成 `CSI Vk;Sc;Uc;Kd;Cs;Rc _` 发出；
  4. 其余交给 xterm 处理。
  另外提供一个“把下一个按键直接发送给终端”的动作，应对快捷键和终端程序冲突的情况。
- **粘贴**：多行内容或包含控制字符时先弹窗确认（SEC-04）；终端开启了 bracketed paste 时交给 xterm 自动包裹。当前工作区仅受信用户 ClipboardEvent 的 text/plain；单次≤1012 UTF8字节，保留12字节bracket框架空间，仍走既有1KiB输入门禁。Tab／换行／Bidi及隐形格式字符需scope拥有的modal批准，取消默认焦点，预览仅纯文本并转义隐形字符；C0/C1不支持控制符及超限直接拒绝，不删改／截断／自动重发。view退役／EOF／resize关闭输入时同步撤销审批，迟到批准不得发送；没有policy的固定probe仍阻断粘贴。用户选区Ctrl+C使用公共getSelection及受信copy事件写text/plain，无选区仍交xterm编码中断；结束画面复制不授输入权。仅当前显示缓存复制，不替代未来backend深历史搜索／精确导出；OSC52依然一律禁止。
- **历史与搜索**：xterm 缓冲区之外的历史，由“历史查看器”浮层分页读取（`term_history_page`）并用虚拟列表显示。搜索在后端执行（`term_search`）：命中的行如果仍在 xterm 缓冲区内，就在终端里高亮；否则在历史查看器中定位。
- **屏蔽 xterm 的查询应答**：后端负责回答程序的查询（见 backend.md §4.5）。前端用 `term.parser.registerCsiHandler` / `registerDcsHandler` / `registerOscHandler` 拦截 DA1/DA2、DSR、DECRQM、XTVERSION、DECRQSS、OSC 4/10/11/12 查询和 CSI t 报告，返回 `true` 表示已处理，xterm 就不会再发出应答。拦截范围以 S2 的测试样例为准。前端切换终端配色时调用 `term_set_palette`，把配色同步给后端。
- **链接**（SEC-09）：OSC 8 超链接和自动识别的 URL，点击后先显示完整地址；`file://` 和自定义协议需要用户确认。
- **字体**：先用 `document.fonts.load` 加载字体，再计算行列数；WebGL 上下文丢失时，重新创建 webgl addon，连续失败则退回 DOM 渲染。

## 7. RemoteCanvas（features/desktop）

- **绘制**：
  - 每个 Surface 对应一张 WebGL2 纹理。
  - `Rect` 帧用 `texSubImage2D` 更新纹理；`H264` 帧交给 `VideoDecoder`，得到的 `VideoFrame` 写入对应区域后立即 `close()`。
  - 每一帧用一次全屏绘制合成。只在收到新帧时才安排下一次 `requestAnimationFrame`。
  - 帧处理完成后调用 `ack(frameSeq)`。
- **显示方式**：适应窗口（保持宽高比居中）、1:1（可滚动）、动态分辨率（尺寸变化时调用 `desktop_resize`，并把 `devicePixelRatio` 换算成远端的缩放比例）。
- **光标**：把后端发来的光标形状转成 data URL，设置为 CSS `cursor`；光标太大时自己绘制。
- **输入**：
  - 键盘：用内置的映射表把 `KeyboardEvent.code` 转成 PS/2 扫描码；标记哪些是扩展键；处理 AltGr；处理大小写、数字锁定等锁定键的状态同步。
  - 中文输入：用一个隐藏的 textarea 接收输入法组字，`compositionend` 时把文字作为 Unicode 事件发送。
  - 鼠标和滚轮：用 Pointer Events，可选 Pointer Lock。
  - 发送方式：同一帧内的输入事件合并成一批，通过 `desktop_input` 发送。
- **系统快捷键捕获**：画布获得焦点且用户开启了捕获时，调用 `desktop_capture_keyboard(true)`；失去焦点时立即调用 `false`。界面上持续提示“正在捕获键盘，按 Ctrl+Alt+Home 释放”。
- **兼容性检测**：连接前用 `VideoDecoder.isConfigSupported({codec:'avc1.42E01F'…})` 检测是否支持 H.264，把结果作为参数传给 `desktop_open`。

## 8. VncView

- 用 `ChannelSocket` 适配层模拟 WebSocket 接口（`binaryType`、`send`、`onmessage`、`onopen`、`onclose`、`readyState`）。服务器数据来自通道中的 `RfbBytes` 帧，发往服务器的数据调用 `vnc_send`。noVNC 的 `RFB` 对象接收这个适配对象。这个适配方式是否可行要在 S15 验证。
- noVNC 按需加载，作为独立的代码块。不修改它的源码，以满足 MPL-2.0 的要求。
- 工具栏、缩放方式和快捷键捕获，和 RemoteCanvas 共用同一套 `DesktopChrome` 组件。

## 9. 文件面板（features/files）

- 每栏是一个虚拟滚动的表格，列有名称、大小、修改时间、权限、属主。目录内容由后端分批返回，先到先显示。
- 支持的操作：
  - 两栏之间拖动即传输；
  - 从系统文件管理器拖入时，把拿到的本地路径交给后端上传；
  - 右键菜单中的各项都对应 Capability 中的 action，因此也能用快捷键和命令面板执行。
- 传输队列面板：列表数据来自 `transfersStore`；进度条组件只订阅自己那一个任务的进度。
- 内置编辑器：在新标签中打开 Monaco；保存时调用 `fs_write_text`；超过 5MB 的文件提示改用外部编辑器。

## 10. 主题与窗口材质

> 配色不自研（ADR-025）：界面用成熟色板加语义变量，终端用通用的 16 色 ANSI 配色格式，主题生态直接复用 VS Code / Windows Terminal / iTerm2 / base16 的现有资源。

### 10.1 界面配色：色板 + 语义变量

- 组件只使用**语义变量**，禁止直接写色值（由 lint 检查）。变量的值来自 **Radix Colors** 的 12 级色板，每级用途固定，深色和浅色两套成对设计、对比度已调好。
- 一套主题由**一个中性色系**（默认 slate）和**一个强调色系**（默认 cyan，等品牌定稿）生成；状态色固定使用 red / amber / green / blue。用户换一个强调色系，整套界面跟着变。

| 语义变量 | 取值（Radix 级别） | 用途 |
|---|---|---|
| `--bg-base` / `--bg-subtle` | 中性色 1 / 2 | 窗口底色、侧边栏和状态栏 |
| `--bg-surface` / `--bg-hover` / `--bg-active` | 中性色 3 / 4 / 5 | 组件背景的默认、悬停、按下状态 |
| `--border-subtle` / `--border` / `--border-strong` | 中性色 6 / 7 / 8 | 分隔线、输入框边框、焦点和悬停边框 |
| `--fg-muted` / `--fg` | 中性色 11 / 12 | 次要文字 / 主要文字 |
| `--accent-bg` / `--accent-bg-hover` | 强调色 3 / 4 | 选中项、激活标签的背景 |
| `--accent` / `--accent-hover` / `--accent-contrast` | 强调色 9 / 10 / 配套前景色 | 主按钮、焦点环、进度条，及其上的文字 |
| `--accent-fg` | 强调色 11 | 链接、强调文字 |
| `--danger-*` / `--warning-*` / `--success-*` / `--info-*` | red / amber / green / blue 的 3、9、11 级 | 错误、警告、成功、提示 |
| `--radius-*`、`--space-*`、`--font-ui`、`--font-mono` | — | 非颜色 token |

- 实现：`design/theme.ts` 根据主题中的 `neutral`、`accent` 和深浅色，从 `@radix-ui/colors` 取值写入 `:root` 的 CSS 变量；Tailwind 的颜色配置只引用这些变量。切换主题只改变量，不重新渲染组件。
- 主题中的 `[ui]` 段可以直接覆盖个别变量（例如导入 VS Code 主题时，用它的 `editor.background` 覆盖 `--bg-base`）。未覆盖的变量仍然从色板生成。
- 管理员窗口的强调色系固定为 amber，不受主题影响，确保一眼能区分。

### 10.2 终端配色：通用 ANSI 格式

- 结构是 16 色 ANSI，再加 `foreground`、`background`、`cursor`、`cursorText`、`selectionBackground`、`selectionForeground`。字段名和 Windows Terminal 的配色方案对齐（`black … brightWhite`），可以直接映射到 xterm.js 的 `ITheme`。
- 终端配色和界面主题**分开选择**：一套主题自带默认的终端配色；另外可以按 profile 或主机单独指定（例如生产环境主机用偏红的配色）。
- **配色库**：随应用附带 iTerm2-Color-Schemes（MIT）的机器可读版本（JSONL），在设置页列出并支持实时预览。配色库按需加载，不计入首屏体积。
- **可读性兜底**：默认开启 xterm 的 `minimumContrastRatio`（默认 4.5，用户可以关闭），文字和背景对比度不足时自动调整亮度。

### 10.3 主题文件与导入

```toml
# themes/pulseterm-dark.toml
name = "PulseTerm Dark"
appearance = "dark"              # dark | light

[ui]
neutral = "slate"                 # Radix 中性色系：gray / mauve / slate / sage / olive / sand
accent  = "cyan"                  # 任一 Radix 强调色系，或直接写色值
# overrides = { "--bg-base" = "#0f1115" }   # 可选：覆盖个别语义变量

[terminal]                        # 字段与 Windows Terminal 配色方案一致
foreground = "#…"
background = "#…"
cursor = "#…"
selectionBackground = "#…"
black = "#…"   # red green yellow blue purple cyan white
brightBlack = "#…"   # brightRed … brightWhite
```

- **绘制投影边界**：后端`ThemeDocument::paint_data`现提供独立version=1、4096字节以内的serialize-only图像：ANSI16、可选cursor/selection、28固定UI色槽与显式appearance/neutral/accent；颜色是u8 RGB三元组，不携带主题名称、路径、任意CSS文本或权限。terminal低于4.5:1只告警，不改原色。已有严格Rust→TypeScript互通、复用真实ViewScope的撤销guard与nonce UI覆盖资源；fixed settings-probe 的peer页面已完成实际文件投影交付，迁移main的read/confirm/drawn与前端ready握手正在独立验收。主页面导航撤销主题document epoch，不关闭PTY；fixed UI色槽的computed读回与恢复不是生产完整主题应用。Radix尺度解析（尤其自定义seed）、UI对比度、源变化撤销通知和xterm默认色/OSC状态同步仍须独立实现及验收；不能将数据或受控页面回执当GUI完成。
- **内置主题**：PulseTerm Dark / Light、One Dark / Light、Catppuccin（Latte / Mocha）、Dracula、Solarized Dark / Light。每套主题随应用分发前都要核对许可证，并登记到 `THIRD_PARTY.md`。
- **跟随系统**：设置项 `theme = { light = "PulseTerm Light", dark = "PulseTerm Dark" }`；系统深浅色变化由 Tauri 的主题事件通知，前端只切换变量。
- **导入**（UI-07）：

| 格式 | 得到的内容 |
|---|---|
| VS Code 主题 JSON | `terminal.ansi*`、`terminal.background/foreground/selectionBackground` → 终端配色；`editor.background`、`sideBar.background`、`focusBorder`、`button.background` 等 → 界面变量覆盖。**一次导入同时覆盖界面和终端** |
| Windows Terminal 配色方案 JSON | 终端配色（字段一一对应） |
| iTerm2 `.itermcolors`（plist） | 终端配色 |
| Alacritty TOML / Ghostty 配置 | 终端配色 |
| base16 / base24 YAML（tinted-theming） | 按规范中的映射表同时生成终端配色和界面变量覆盖（base00–07 → 背景和文字层级，base08–0F → ANSI 和状态色） |

- 解析和转换在后端完成（见 backend.md §10.3）；导入时检查对比度，前景与背景低于 WCAG 4.5:1 时提示用户。
- 当前 `pulse-store::theme::import_theme` 是显式格式、64 KiB 限额的纯 Rust 导入入口，输出不是完整应用主题。Windows Terminal／VS Code／base16/base24 保留文件内名称，Alacritty／Ghostty／iTerm 必须提供独立显示名称，不嗅探扩展名或加载外部引用。VS Code 严格 JSON 同时提取完整终端色与六个固定 UI 覆盖；未知终端色、透明色、include 与 JSONC 拒绝，未映射编辑器色会明确计数，token grammar 不应用。iTerm 仅接受明确 sRGB、无透明度的 XML 子集，有限浮点分量按最近 RGB8 量化；不支持的色域、binary plist、未知实体及重复键拒绝，标准 Apple DOCTYPE 仅惯性识别，不取网络。
- 统一结果保留组合覆盖与转换说明，不把 VS Code 组合片段降为终端片段；UI 基底／appearance 必须单独选择。`ThemeDocument::from_import` 和 `parse_theme_document` 已连接显式 scalar `appearance = "light" | "dark"`、六种 Radix 中性色、已知色系／opaque RGB8 强调色与 28 个固定语义颜色覆盖，完整 TOML 与片段互相不能误认。纯数据阶段不生成 Radix CSS（包含直接 RGB seed 的色阶）、保存文件或应用 GUI；七格式适配、固定探针和原生消费者通过不代表目录监听／GUI 应用已完成。
- base16/base24 YAML 必须显式选择系统，支持 common 与受限 legacy 结构。按照官方 ANSI 映射处理，不把 base16 的重复亮色擅自替换成 orange/brown；原始 16/24 色和元数据保留于导入结果。64 KiB／192 事件／两层 mapping，拒绝 anchor／alias／tag、重复键、多个文档、深层容器及未知／额外色槽，不调用 YAML tree loader 或模板。`ThemeImportNotes::base_palette_projection` 标记 ANSI16 投影；普通完整文档／片段导出返回 `Projection`，必须显式调用 `from_projected_import`／`to_projected_toml_fragment` 才能导出终端模型未覆盖的 base 槽位。投影后的文件不保存这些额外源色，不是原始方案无损序列化。

- 受控主题目标交付已验证只读源撤销观察：每 publication 是一次 false→true 的通知，不使用可回绕事件序号；观察本身不授予重绘权限、不保活 owner、也不能释放尚在运行的 I/O 额度。注册层先校验 exact WindowToken/session/generation/registry 和当前 snapshot，替换发布、失效或 source 关闭即通知。前端明确区分 `invalidate`（保留 stale-last-good 像素）与 `revoke`（authority 失效即移除样式，读槽待操作退出）；cleanup failure 使原 ViewHost fail-closed，不影响独立 view 或 session。
- 固定 `settings-probe` 的实际移窗主视图已接单次 `probeTargetThemeWaitRevoked`：先真实绘制→native 显式失效→同 claim/document epoch/generation 复验通知→删除28色槽并读回原值→single-use 清理回执。challenge 导航／退出立即取消等待，整体维持原1秒截止；这不是生产连续主题订阅、外部文件 watcher、自动重取或终端配色同步，默认工厂仍关闭。

- 前端 `features/theme/radix-theme.ts` 已生成全部28个固定UI语义槽，复用锁定Radix3的opaque sRGB light/dark色阶；仅stock家族，不支持直接RGB seed色阶且不做近似。完整解析不修改终端数据，行政窗口amber仅能由可信调用选项指定并屏蔽6个accent覆盖，不授予任何后端权限。accent-contrast在黑/白中最大化9/10两态最低对比；生成器报告25个正文色对的WCAG告警，显式reject策略在创建sheet前拒绝低于4.5:1的结果，绝不暗中修色。
- Radix11在某些light有色背景不保证正文4.5:1；不能仅凭采用Radix宣布全部AA通过。受控light主题的4个高对比文字覆盖明确来自原主题数据和官方light12，不改变默认11级映射。实际peer light→dark与target dark已在原nonce/generation/撤销链通过，但这不是生产全局外壳、主题编辑界面、自定义seed、文件监听或终端查询配色同步；普通工厂不启用，生成器只在实际主题绘制时dynamic import。
- 终端基础色与 UI 色槽为两个不同投影：后端 actor 捕获内的 19 色基础 QueryPalette 按绑定 generation 只读取得，在 xterm 恢复字节前配置实例；远端 OSC 覆盖仍由终端维护。RIS 保留远端 OSC 色覆盖（与既有后端查询语义一致），OSC 104/110/111/112 显式重置才回退基础色；不能为通过测试引入清除覆盖的 parser 钩子。不将基础色注入远端 VT，也不把读取颜色查询回复写回 PTY。真实 WebView 的一次捕获色验证不等于生产全局主题热切换或全平台性能/系统 IME 验收。

### 10.4 窗口材质与标题栏

- **窗口材质**（UI-02）：
  - 后端下发实际生效的材质，前端设置 `<html data-backdrop="mica|mica-alt|acrylic|vibrancy|solid">`；
  - 外框的背景使用 `color-mix(in srgb, var(--bg-surface) var(--surface-alpha), transparent)`；材质为 `solid` 时 `--surface-alpha` 取 100%；
  - 终端区域默认不透明。只有用户把终端不透明度调到 100% 以下时，才开启 xterm 的 `allowTransparency`；
  - 远程桌面画面、编辑器和弹窗始终不透明。
- **自绘标题栏**：用 `data-tauri-drag-region` 标记拖拽区域；窗口控制按钮的位置和样式遵循各平台习惯（macOS 使用系统红绿灯按钮并留出左侧空间）。管理员窗口在标题栏上显示盾牌图标和不同的强调色。

## 11. 快捷键与命令面板

当前七固定动作的产品边界见 [workspace-command-palette.md](./workspace-command-palette.md)。有界 chord/when/定义冲突模型已测试，默认面板键位复用 matcher；当前页 keymap IPC／文件源联合确认保存、序列运行时和Settings快捷键表单已接，749TS／134Native／Linux受控GUI基本链与focus隔离通过，见 [keymap-source.md](./keymap-source.md)。`cap_list` 通用执行目录、三平台／系统IME与完整键位生态 **尚未交付**。以下是目标架构，不表示当前产品已实现。

- 快捷键的来源是内置方案加上用户的 `keymap.toml`，由后端合并后下发。`KeybindingService` 支持连续按键（终端候选如 `ctrl+shift+k ctrl+shift+s`）和生效条件（`when: terminalFocus && !broadcast`）；普通 Shell 的 `ctrl+k ctrl+s` 只能用于显式 `!terminalFocus` 条件，不能为连续键功能吞掉 Shell 的 CtrlK。
- 命令面板的数据来自 `cap_list`（过滤掉不支持从面板调用的 action），再加上 Vault 主机和片段。各项先按模糊匹配打分，再按最近使用加权排序。
- macOS 的默认快捷键用 Cmd，另外两个平台用 Ctrl+Shift，避免和终端程序抢按键。

## 12. 多语言与无障碍

- 当前生产工作区先接通用户层语言保存、显式文案绑定及工作区／设置／快捷键／命令面板双语，设计与剩余 UI-05 范围见[工作区语言](workspace-language.md)。终端输出与用户名称不参与翻译，保存后即时应用，读回同步实际值；原生与跨平台验收保持开放。
- 文案按功能模块分文件（namespace），文案 key 由 TS 类型约束；错误码的翻译统一放在 `errors` 中。
- 所有可交互元素都能用键盘操作；优先使用 Radix 组件以获得正确的 ARIA 属性；开启 xterm 的 `screenReaderMode` 选项（P2）。

## 13. 性能预算与加载策略

- 首屏只加载窗口框架、工作区和 TermView。Vault 页面在空闲时预取。设置页、文件面板、远程桌面、Monaco 和 noVNC 都用 `lazy()` 按需加载。
- 首屏 JS 不超过 300KB（gzip），由 CI 在构建后检查体积。
- 字体：界面字体使用系统字体；终端只内置一个子集化的 Nerd Font，用 `font-display: block`，并由 Rust 在启动时预先加载。

## 14. 前端测试策略

| 层级 | 手段 |
|---|---|
| 纯逻辑 | Vitest：布局树操作、帧解码（与 Rust 共用样例）、扫描码映射、快捷键解析、各 store |
| 组件 | Testing Library + `ipc/mock` |
| 浏览器开发模式 | `pnpm dev:web`：用模拟后端在普通浏览器中开发界面。模拟后端包含一个回显终端和假的 Vault 数据 |
| 端到端 | Playwright + tauri-driver（支持 Windows / Linux；macOS 上的 WKWebView 没有 WebDriver，只能人工测试和做冒烟脚本） |
| 视觉回归 | Playwright 截图对比：主题、材质降级、深浅色（P1） |
