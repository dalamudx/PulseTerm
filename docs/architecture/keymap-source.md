# 键位来源、投影与后台 I/O 边界

## 已实现的后台基础

`pulse-store::keymap` 是纯 schema 1 模型，不读文件或授予 Native/Capability 权限。源是独立 `keymap.toml`，不是终端 `settings.toml`。`schema_version` 必填，未知字段、重复动作、未来版本、无效完整来源全部拒绝，不降级默认。

```toml
schema_version = 1
[[bindings]]
action = "settings"
keys = ["ctrl+shift+s", "meta+shift+s"]
when = "workspaceFocus && !editableFocus && !modalOpen"
```

缺少条目继承内置：palette 为 Ctrl/Meta+Shift+P，find 为 Ctrl/Meta+Shift+F，其余动作默认无键位。find默认仅终端聚焦、无普通输入框／模态／广播时生效，不占用Ctrl+F。keys=[]禁用，Restore删除覆盖恢复内置。源最多16KiB、9固定UI动作、每动作4别名、合并最多16绑定；动作仅原七显示入口、find和palette，不接受任意action、程序、地址、凭据或路径。

按键ASCII/128字节/最多2步、固定修饰与命名键；when只5固定布尔字段的&&和!。不是JavaScript求值器。同context条件重叠时精确或序列前缀冲突拒绝，条件互斥可复用键位。普通Shell前缀例如CtrlK只有显式!terminalFocus可用；终端候选限modified app chord，候选不是完整Shell兼容证明。Clipboard/reload当前路由保留，ShiftP不能改成其他动作。

JSON投影版本1：`bindings`仅action/keys/when，`sources`恰好9项来源及disabled状态；不含执行回调。前端 `keybindings/projection.ts` 精确字段与版本校验、唯一来源、绑定可用性一致检查，再复用条件/冲突/保留键编译；来源标签不是权限。

## 固定文件与取消

`KeymapFile`只打开后端明确提供的可信私有目录；basename固定keymap.toml，lock固定.keymap.lock。共享`FileStore`闭合slot新增Keymap，不改变Settings/Theme算法。缺文件继承；存在空文件属于无效完整来源，不能fallback。Unix私有目录／600文件／所有者／no-follow／非硬链接与替换目录检查沿用原边界；不是完整目录链TOCTOU或power-loss证明，Windows ACL仍closed，macOS实机未验。

`KeymapSnapshot`不Debug/serde，绑定原实例Arc身份、目录、slot、文件stamp和原文；同目录另一实例也不能消费其空快照。typed action Set/Restore先验证合并，再整个来源CAS比较后原子替换。此入口canonical序列化：保其他动作的语义，但**不保留注释或原始字节**；未来UI启用前必须明确告知，不将其当现Settings的byte-preserving edit。外部修改或损坏整个来源拒写，不重置。

授权在首次调用、temp fsync后和rename前重检，panic拒绝；rename后撤销不能撤回真实提交，adapter需拒迟到UI receipt并明确重读，不能自动重复写。`KeymapIoOwner`使用明确runtime Handle，避免同步Tauri主线程隐式tokio::spawn失上下文。单process IO quota在enqueue前取得，quota和caller guard归blocking闭包；取消/owner close/runtime shutdown不能提前释放active信用，queued取消跳过工作，request/cancelled receipt只能观测真实退出。弱handle不能保活或复活owner。取消save的接收端也参与native precommit检查；已提交结果丢失仍非回滚证据。

## 同来源与当前上下文确认组合点

`KeymapFile::confirm_current_authorized` 先检查原实例Arc，再在cooperating文件锁内比较整个原image与stamp，随后调用可信上下文检查；same-values的comment修改也视为新源，foreign/stale先拒且不调用callback。`KeymapIoHandle::begin_confirm_retaining` 持有原snapshot、guard与quota直到真实join，接收端取消／owner close也拒；成功仅返回同一image的snapshot。

12项storage confirmation相关回归、文件锁内callback/迟到context信用验证以及上层Native原PageLaunchScope、foreign registry、实际ProductDocuments导航后的旧scope拒绝通过；Workspace126消费者及store/desktop严格检查通过。该primitive**不是原生页许可或页面绑定票据**：上层必须保存原scope并比较当前nonce/window/page，不能从前端传入任意callback或把旧scope改成新scope。成功是point-in-time比较，不冻结无锁external editors；后续生产IPC与GUI基本路径已经接入（见下）；primitive本身不授予该权限。

## 当前页Root服务与Linux GUI路径

`product_keymap.rs` 已挂在独立工作区Root：固定app_config_dir／同实例cache、read→confirm→save→reconfirm不同原page Arc／registry／300s单次票据。源确认仅启用8个既有UI选择动作；当前WindowToken/nonce由真实Tauri local-source链验证，前端不提交文件路径或回调。canonical丢注释须checkbox独立consent，修改输入清consent；无consent、重复、过期、换页、其他registry、目录切换与sourceCAS冲突均拒。

Root独立supervisor持原page任务、信用和实际nativejoin；RPC结果丢失或显式本页cancel不提前释放quota、不关共享IO/PTY。导航退役原scope/ticket，退出共享drain观测不能因取消一个观察者丢唯一join。6实际文件/原页/取消/shutdown测试与严格检查通过，完整mise check/test/probe、workspace134、workspace-ssh严格检查通过，最新749前端测试通过；bank诊断此前Exact无诊断；本次符号查询因Rust服务不可用/索引stale返回Unknown，不当新MCP通过且未安装，TS仍Unknown。

前端17protocol（含同RPC取消确认栅栏）、8editor、3focus分类和19dispatcher反例通过。旧页级cancel须确认后才派发新flow，失败直到reload拒绝后续派发，不把cancelACK当IO退出。focusin/blur主动取消，focus A→B→A和source空表→新表保原completion墓碑，不延长800ms。原xterm helper textarea误作普通表单导致default快捷键失效已修：终端先分类、普通表单仍受保护。真实Linux缺文件default三页以及Settings GUI独立consent保存Palette Y、禁用keys=[]、恢复移除override、保存Palette K→Y物理两步三页已过；每次source确认、同PID/PTY精确输入、无搜索字节漏写、实际Root退出均验证。保存后scroll与Done焦点只在原currentepoch恢复；只静态CSP样式。Nativeconfirm先于绘制，探针用实际像素form门禁，不把Native回执当GUI画完；原8s/80s与800ms不变。

`mise run probe-workspace-keymap`／Linux CI登记；fixtures统一/data/temp唯一目录，screens/native/TOML证据仍在target/probes。来源确认是point-in-time，三页focus A→B→A实际按钮切换后旧completion不打开模态、同页source替换墓碑和clipboard兼容已过；三组前后截图均正常非遮罩。实际GUI原票据期间独立atomic外改：旧保存收到keymap_stale、新文件bytes未被覆盖，重新确认Z在三页生效/同PTY精确输入与Root退出通过。两真实应用进程同目录/同GUI保存Y文件重开，各3页，新旧Native PID不同且均已reaped、文件bytes不改；不是复制artifact到新目录当持久化。probe-workspace-keymap-state/Linux CI登记。真实Reload手势前arm旧prefix、后续两新页completion不打开modal再valid序列通过；Reload键本身也退役前缀，不当navigation-only隔离证明。三平台/系统IME与生产watch仍不计已验。默认/禁用palette不走旧快捷键旁路；其它动作仍原按钮／capture与Native权限，无新exec。

## 设置中的入口整合

快捷键产品入口现已加入设置的“键盘与剪贴板”分类及名称／配置键搜索，稳定按钮 ID 保持为 `workspace-keymap-open`。进入时挂起父设置，关闭快捷键或按 Esc 返回同一筛选、草稿与发起控件；下一次进入快捷键仍重新读取并确认来源。此导航不改变 read→confirm→save→reconfirm、文件规范化同意、源冲突或原页取消边界，详见[设置导航](workspace-settings-navigation.md)。

`probe-workspace-reload.sh` 同步为搜索 keymap、三次物理 Tab 和回车进入；冲突后返回入口重读，退出到终端需要两次 Esc。浏览器已核对相同物理路径，脚本静态语法通过；本批未执行该原生脚本，不把既有实窗结果自动延伸为新入口验收。

## 验收与未交付

9模型、8真实文件、7确定性后台IO测试通过；576modifier/key往返、32contexts/64条件对，6实际Rust JSON投影和1158Rust/TS策略对照通过。三独立进程各自持快照：先提交、旧来源拒写、第三进程重开唯一保存别名；所有自有进程/目录退出清理。原Settings17/Theme6文件消费者及完整mise check/test/probe、702TS、strictlint通过。Rust keymap/keymap_file/keymap_io诊断Exact；TS MCP Unknown不计。`mise run probe-keymap`与Linux CI登记，证据`target/probes/keymap-*`。Audit仍6license+2maintenance失败，不放宽。

后续 `ui/src/keybindings/dispatcher.ts` 纯runtime组件已通过14确定性反例、716全TS／strict，TS MCP仍Unknown。固定800ms、一个pending组、原nonce/focus/全contextcapture；deadline/变页/窗格/广播/可用性变化撤销不重新解释完成键，源替换／proactive取消保留有界retired completion防旧完成升级新动作。repeat／重复down不重复执行或延期，keyup在修饰/focus变化后仍消费；IME/AltGraph/未知键不执行，普通输入不重放。无timer回调执行，时钟倒退／非finite拒绝。仅固定既有UI action回调，不授权Native调用；后续已挂工作区，实际物理modifier-only过渡反例带来3新测试（共17），修饰键不清pending/retired墓碑，不延长800ms。不是实际系统IME或三平台验收。

**仍未交付／未验**：通用cap_list执行目录、普通CtrlP完整终端矩阵、navigation-only原前缀隔离证明、自动文件watch、系统IME与三平台。当前9固定动作可通过Settings快捷键表单配置，并已验上述受控Linux基本链；不代表完整键位生态或Alpha交付。detector记录现有workspace.html层级flat-type-hierarchy启发式warning，实际模态沿既有样式/截图，未把warning或截图当完整accessibility审计。
