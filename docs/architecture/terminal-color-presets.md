# 新会话终端配色预设

工作区“设置 → 外观与配色”提供十套固定终端预设。PulseTerm Dark/Light 沿用原午夜／纸白的存储 ID 与原始 RGB；经典黑底仍为默认，与此前 QueryPalette 的全部字节相同，14px 字号不变。命名目录同时用于全局、本地 Profile、直属分组及 SSH 入口。终端预设与界面浅／深／跟随系统独立；按系统分别选择深浅主题仍待实现。

| 名称 | 稳定 ID |
| --- | --- |
| 经典黑底 | `classic` |
| PulseTerm Dark（午夜） | `midnight` |
| PulseTerm Light（纸白） | `paper` |
| One Dark | `one_dark` |
| One Light | `one_light` |
| Catppuccin Mocha（深色） | `catppuccin_mocha` |
| Catppuccin Latte（浅色） | `catppuccin_latte` |
| Dracula（深色） | `dracula` |
| Solarized Dark | `solarized_dark` |
| Solarized Light | `solarized_light` |

色值设计参考 [One Dark](https://github.com/atom/one-dark-syntax)、[One Light](https://github.com/atom/one-light-syntax)、[Catppuccin](https://github.com/catppuccin/palette)、[Dracula](https://github.com/dracula/terminal-app) 与 [Solarized](https://ethanschoonover.com/solarized/)。Solarized Light 正文选择该色板的 base01（`#586e75`），全部十套基础正文对比度至少 4.5:1；ANSI 色块保留各自固定原色。颜色仅在后端维护，前端目录保留 ID／名称及严格协议校验。

```toml
[terminal]
color_preset = "midnight"
```

选择只更新表单内的局部预览。点击保存沿用原页快照、单字段 CAS、注释保留、取消及实际 I/O 退出监督；恢复继承只移除 `color_preset`。保存用于下一次新建的本地或 SSH 终端，GUI 和 CLI（含本地分屏、SSH 冷启动及保存别名）共享后端预设。已有会话及刷新恢复不因修改设置而换色；没有新增在线调色板权限、自动重启或重拨。

## 跟随系统分别选择深浅主题

全局设置的“终端配色方式”支持固定配色和跟随系统；启用后可分别选择系统浅色／深色时的新会话配色，并独立预览。模式与两套选择作为一组 `terminal.theme` 策略通过同一设置票据、CAS 和原子文件替换保存，恢复继承只移除这组策略，保留已有 `color_preset`。缺少策略保持原固定行为；默认深色为 PulseTerm Dark、浅色为 PulseTerm Light。可使用内联表、点键或标准 `[terminal.theme]` 分表，修改和移除保留相邻字段及注释。

```toml
[terminal]
color_preset = "classic"
theme = { mode = "system", light = "catppuccin_latte", dark = "dracula" }
```

原生 GUI／CLI 的本地、SSH 直接及跳板启动从原窗口读取系统外观，和本次设置结果一起选定并冻结基色；未知系统外观明确采用浅色分支。系统外观变化影响之后新建的会话，已有会话不在线换色。页面不提交系统深浅状态或任意 RGB。Profile 的显式配色，以及 SSH 入口／直属分组的固定覆盖，优先于全局系统策略；继承预览使用同一原生外观快照，显示实际选中值和来源。设置投影 v14 强制校验完整策略，v1～13 保持原固定选择兼容。

## Profile、分组与 SSH 入口选择

本地 Profile 的“终端配色与按键”提供继承全局及上述全部十套配色。配色以 `appearance.colorPreset` 和参数、环境及其他外观值一起通过原页票据保存；旧记录缺字段或显式 null 均继承。新建本地会话读取并校验保存的 Profile，在 PTY 分配前以显式配色覆盖本次全局启动值；GUI／CLI 共用此路径。已有会话及捕获基色保留启动值。

组织管理的[终端配色与行为](terminal-scoped-behavior.md)支持直属分组／SSH 入口覆盖，字段为 `terminal_preset`，持久层仍使用已有的 `[terminal].color_preset`。入口优先于直属组、全局及默认；分组恢复继承回到全局。编辑器显示当前有效值及来源，与四项行为统一保存；配色缺字段或非法固定值拒绝。SSH 启动复用原页组织快照解析结果，直接连接与跳板叶使用同一后端基色。配色选择与界面外观／强调色独立，完整自定义主题库与导入仍未交付。

## 启动与恢复

本地原生入口在当前原页的共享设置 I/O 中读取有效预设，冻结在本次 ProfileStart。SSH 直接入口在网络启动前读取，跳板路由在开始执行前读取一次，冻结在 StartTarget／StartContext 或 route::Context。只读查询不消耗或替换设置编辑票据，并在 I/O 前后检查原页；无效设置或撤权不能退回经典后继续启动。GUI、CLI 及其冷启动沿用原页确认流程；设置读取不授予目标、指纹、凭据或终端输入权限。

密码、私钥及 Agent 的直接 sink 和跳板叶 sink 都持有同一份启动基色。跳板逐跳期间不重读设置；中间连接不创建终端。叶端认证及 PTY／shell 成功后，共用 ssh_terminal_io 在向 Actor 交付初始缓冲及后续远端字节前安装基色。原来的认证与资源回收监督保持。

预设的 16 个 ANSI 色、前景、背景和光标来自同一份后端固定 RGB 数据，供预览、Actor 初始基色、OSC 查询及现有捕获／恢复流程使用。远端 OSC 设置覆盖仍优先，远端重置回到本会话冻结基色；修改全局设置不会重写已运行会话。没有解析输出字节来猜颜色，也不向 PTY／SSH 发送预览文本。

生产设置投影为 v14，要求十条固定且无重复的 `terminal_presets` 绘制数据和完整的用户层 `terminal_theme` 策略。前端兼容 v1–v13：v1–v4 无配色，v5–v12 严格保持原三条目录及其选中值，v13 使用十套目录及固定选择。所有版本检查闭合字段、固定 ID、16 个 ANSI 色和 0–255 RGB 分量；v14 另验证完整模式／深浅 ID，不接受部分策略、URL、文件路径或 CSS 字符串。预览的 ANSI 原色以带边框色块展示，说明文字使用本主题正文色，避免黑字黑底。

## 开发验证

本次命名目录扩展：41 项前端、2 项目录／正文对比度、3 项组织、5 项 Profile、16 项原生设置检查，以及 Actor 和真实 PTY 的各 1 项检查通过。真实 PTY 以 Catppuccin Mocha 验证 Profile 覆盖并以午夜验证恢复；原生单项票据测试另用 Dracula 保存重读。Chrome 组件使用后端序列化 RGB，覆盖十套局部预览零写入、保存重开、恢复继承、搜索和 390px 布局、Profile 字段与 SSH 入口来源。组件不替代原生 WebView 或 SSH 网络验收。详细结果见[验证记录](../development/validation.md)。

本次作用域扩展：3 项组织存储、5 项 Profile 存储、40 项前端和 2 项 App 直接检查通过，其中单个真实 PTY 探针用 OSC 11 验证 Profile 纸白覆盖及恢复全局午夜。Chrome 组件覆盖保存／重开／继承来源、冲突重读和迟到响应；Store／App 全目标与 CLI＋SSH 桌面库严格 Clippy、类型、格式、产品构建通过。未在本批复跑 SSH 网络／原生 WebView／跨平台矩阵；桌面全部目标检查仍受既有 CLI 测试 unwrap lint 阻断。轻量收据见 `target/probes/scoped-terminal-presets-development/receipt.json`，详细范围见[验证记录](../development/validation.md)。

首批本地功能已有 2 项存储、1 项 Actor、1 项 App、1 项原生文件、20 项前端检查，以及类型、格式、schema、产品构建及 Chrome 组件证据，见 `target/probes/terminal-presets-development/`。

本批 SSH 只执行直接检查：

- CLI＋SSH 原生库测试入口完成编译，覆盖三种身份、GUI／CLI 及跳板叶的必填配色传参；1 项原生只读设置检查复验不消耗保存票据及旧页拒绝。
- 4 项配色组件检查及 TypeScript 通过，保存提示明确新本地或 SSH 终端生效，已有会话保持。
- 现有 `probe-ssh-owned-session` 使用真实回环 TCP、固定密码身份及两次原有连接。纸白和午夜各自首条 OSC 11 查询在远端收到准确回复，Actor 捕获包含对应基色，页面导航恢复保留纸白基色；输入、EOF、取消和原生退出条件保留。新增等待使用实际回复和字节增量，不扩大既有超时。
- 格式与 CLI＋SSH 原生严格 Clippy 通过；两个改动原生入口的 Astrolabe 为 Exact、零诊断。现有 Linux CI 已登记上述探针和工作区原生编译，未在本机证明远端 CI 执行成功；不新增重型验收任务，不跑完整回归。

本批日志和源码摘要位于 `target/probes/ssh-terminal-presets-development/`。真实网络检查覆盖密码直接连接；私钥、Agent、跳板及 GUI／CLI 产品入口的配色实窗、跨平台、在线修改和主题导入／自定义库仍待验收。原有认证验收不能替代本次配色验收。
