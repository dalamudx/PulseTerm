# Linux 运行时与呈现性能评估

当前 Linux Xvfb／软件渲染下，完整产品内存与本地回显均未达标。同一 Tauri 宿主最小无授权页面的私有驻留已超过 120MB，缩短前端历史缓存不足以闭合固定成本。下一步需要真实图形环境和受支持的运行时／宿主对照；产品栈、预算、权威解析和授权约束保持不变。

## 测量口径

环境为 WebKitGTK 2.54.0、GTK 3.24.49、X11 Xvfb 1280×900，使用 `LIBGL_ALWAYS_SOFTWARE=1` 和 `WEBKIT_DISABLE_DMABUF_RENDERER=1`。当前只有虚拟显示节点，没有 render 节点，不代表真实 GPU、Windows 或 macOS。未关闭沙箱、CSP 或 JIT，未安装替代驱动／运行时。

内存使用十进制 MB，计入宿主、WebProcess、NetworkProcess 的完整进程树 PSS，并保留 RSS 和私有驻留。每组使用独立私有目录，就绪后等待 0.5 秒、采三次稳定读数取中位数；进程身份变化、缺子进程或未退出都失败。PSS 受共享映射影响，顺序读取的 smaps 不是精确分配器归因，不能再加到总量上。

回显使用实际产品／PTY、XTest 发键和固定像素观察，保留全部原始样本与长尾。原生使用 `CLOCK_MONOTONIC`；浏览器绝对时钟不直接相减。分段分位数不相加，页面基线不从产品分位数中相减，ACK／逻辑核验不代替像素呈现或物理按键到发光。

## 当前产品结果

| 指标 | 观察值 | 目标 |
|---|---:|---:|
| 本地回显（DOM 脏行两轮共 200 项） | p95 58.537ms | p95 <16ms |
| 空工作区 | 285.513MB PSS | 定位固定成本的参考 |
| 单会话 | 333.568MB PSS | <120MB |
| 20 标签 | 446.369MB PSS | <250MB |
| 单会话十万行历史增量 | 80.880MB PSS | ≤20MB |

显示缓存已从 10000 行改为共享 `MAX_RECENT_ROWS=2000`，后端完整保留量、分页与恢复不变。一次顺序前后观测中，历史增量由 102.554MB 降至 80.880MB，20 标签由 463.885MB 降至 446.369MB；不证明稳定收益。完整十万行保留、20 个原生会话，以及 600 条 Unicode 软换行／80425 字节剪贴板逐字节比对、同会话刷新和退出已有各自证据。复制探针固定 13px 仅用于验证几何，产品仍为 14px。

## 回显定位与候选结论

诊断仅在显式验证构建开启，每连接最多 256 批／帧，只记录时点与双坐标，不记录终端内容。自然 EOF 后通过原页面授权导出，并按序列、UTF-8 区间和源字节边界关联；失败、撤销或身份不一致拒绝导出。绘制核验、双帧和 ACK 顺序保持。

| 对照 | 结果 | 结论 |
|---|---|---|
| 初始完整产品 100 项 | 回显 p95 50.849ms；写回至像素 p95 47.275ms | 仅定位写回后区间，未达标 |
| 浏览器诊断关／开各 100 项 | p95 60.489／49.721ms；首 RAF 和剩余屏障各自 p95 14／17ms | 单次顺序对照不能判定诊断开销或改善；批次与像素尚未逐项配对 |
| 原生 Channel 关联 100 项／102 帧 | 回显 p95 58.486ms；写出起点至提交 0.284ms，提交至像素 53.259ms，提交至 ACK 33.605ms | 主要耗时在提交之后，但不能独立归因 IPC／浏览器／compositor；53 项像素早于 ACK，47 项晚于 ACK |
| 交付边界 100 项 | 回显 p95 57.782ms；交付上界分布 p95 7.069ms；43 项在逻辑核验后仍等待像素 | 依赖未独立校准的每次时钟误差 ≤2ms、相对速率误差 ≤100ppm；是保守上界而非单向 IPC 实测 |
| DOM 脏行两轮 | p95 58.251／60.005ms，合并 58.537ms | 保留已确认冗余整屏刷新的削减；未证明延迟、CPU 或分配收益。首屏／修复／视口改变及 WebGL 仍整屏刷新 |
| 双帧后任务让出，同二进制 ABBA 共 400 项 | 原调度／候选合并 p95 57.138／53.725ms，候选中位数和最大值更高 | 后半程两模式都变快，未证明一致收益；默认保持原调度，候选仅供验证 |
| 固定／变化发键间隔 ABBA 共 400 项 | 合并 p95 58.782／59.236ms | 变化计划为可复现的 5–35ms、成对合计 40ms；未支持固定 20ms 是主要原因，默认采样模式不变 |

交付边界由同一帧的原生“提交→ACK”闭环和浏览器“接收→发起 ACK”区间保守推导；因果矛盾拒绝分析。它不消除时钟假设，也不授权提前 ACK 或删除第二帧。公式与合成边界反例以分析器及其测试为准。

同一像素观察器的 1200 项页面基线中，原生 X11 p95 为 1.083ms，WebKit 立即 DOM 为 17.118ms，等待一帧为 28.118ms，页面 xterm 脏行／整屏为 29.425／28.866ms。没有统一的约 60ms 观察下限；页面与产品宿主、历史及数据链不同，不能借相减得到产品单项成本。

## 固定内存成本

| 对照 | 全进程 PSS | 私有驻留 |
|---|---:|---:|
| 最小 WebKit，两轮 | 221.371／218.154MB | 146.338／145.445MB |
| 页面 xterm，两轮 | 260.555／262.218MB | 176.808／178.500MB |
| 同 Tauri 宿主空白 A1／A2 | 238.200／235.846MB | 161.272／160.117MB |
| 同 Tauri 宿主工作区 B1／B2 | 287.070／288.791MB | 200.511／202.273MB |

同宿主以同一 release 二进制、960×640 窗口和原生服务执行 ABBA，四轮零终端、12 次读数及物理退出均核验。空白页使用固定无脚本／子资源 HTML、严格 CSP 和精确 `about:blank` 来源；导航仍撤销旧权限，不注入 nonce，不准入生产页面。相邻工作区增量为 48.870／52.945MB，包含设置／键位／Profile 的按需初始化，不能全部视为可删除 UI 成本。小 C／GTK 宿主与 Tauri 的总量也不能直接相减。

宿主、WebProcess、NetworkProcess 均有固定成本；约 24.34MB LLVM 驻留映射只是调查线索，尚未证明可安全移除。原生 X11 内存控制仍链接 GTK／WebKit，不是原生终端替代方案的估算。

公开 `hardware-acceleration-policy` 候选已关闭：同一夹具默认→never→never→默认，40 组 WebKit getter 全部为 never，未形成有效策略差异。72 次 PSS 与 2400 项像素、48 组退出及私有目录清理已核验；最小 WebKit 为 217.637–225.677MB、xterm 为 258.026–260.330MB。不把名义 A/B 波动解释为收益，不新增产品开关。

## 复现和证据

测量命令使用新的批次名并拒绝覆盖旧结果。检查入口：`check-workspace-echo-analysis`、`check-echo-pacing`、`check-presentation-floor`、`check-tauri-memory`。测量入口：`probe-presentation-floor`、`probe-runtime-memory`、`probe-tauri-memory`、`probe-webkit-policy`；具体参数以 `.mise.toml` 和采集器为准。

下列收据均相对被 Git 忽略的 `target/probes/`；原始样本、失败、截图、源码／二进制指纹和清理记录保留在对应批次。

| 范围 | 收据 |
|---|---|
| 产品初始基线／浏览器时点 | `workspace-load/echo-product-core-v2/analysis.json` · `workspace-load/echo-stages-pair-v1.json` |
| 原生 Channel／条件交付边界 | `workspace-load/echo-native-channel-v1/verification.json` · `workspace-load/echo-delivery-bounds-v1/verification.json` |
| DOM 脏行／任务让出／发键间隔 | `workspace-load/echo-dom-dirty-pair-v1.json` · `workspace-load/echo-draw-yield-abba-v1.json` · `workspace-load/echo-pacing-abba-v1.json` |
| X11／WebKit／xterm 呈现基线 | `presentation-floor/verification-v1.json` |
| 显示缓存／复制 | `workspace-performance/display-cache-comparison-v1.json` · `display-cache-2000-copy-v1/`（失败）· `display-cache-2000-copy-v2/`（通过） |
| 最小宿主内存 | `presentation-floor/runtime-memory-verification-v2.json`；未隔离 v1 单独保留，不混合统计 |
| 同 Tauri 宿主 | `tauri-memory/same-host-verification-v2.json`；首次空白就绪超时和构建超时独立保留，不登记为正常关闭或产品启动失败 |
| 公开合成策略 | `presentation-floor/webkit-public-policy-verification-v1.json` |

下一步固定同一产品、字体／缓存／窗口和全进程计账，在真实 GPU／驱动／刷新率环境对照内存与回显。若需改变运行时／宿主桥接，先评审维护、许可与分发成本，再复验 CSP／nonce、宽度／UTF-8、查询应答所有权、恢复、授权和物理退出。不能提高预算、删后端历史或用无效策略对照替代这一决策。
