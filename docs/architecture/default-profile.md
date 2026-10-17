# 自动发现 Profile 的默认选择

## 范围

默认选择是独立本地偏好，当前产品在“本地 Profile → 管理”对话框中提供选择、保存、清空和“启动默认 Profile”。保存不启动终端；打开窗口、刷新、读取偏好和进程重开也不自动启动。既有侧栏显式 Shell 启动及 CLI 请求保留原行为。默认选择本身不新增 executable、cwd、参数或环境编辑能力。另行接入的 [Profile 编辑器](./profile-editor.md) 保存后端发现身份的参数、cwd、环境与稀疏外观，启动 IPC 仍只接受身份。

## 存储与授权

`pulse-store::profile_default` 在既有 `app_state` 中保存版本化记录，不改变 SQLite schema。记录保留 revision；清空写入 shell=null 并递增 revision，不删除版本。旧的 Profile 显示偏好文档及快照不会覆盖默认选择。


自定义 Profile 扩展保持原 `selected/offered` 自动 Shell 投影兼容，使用 `customSelected/customOffered/customName` 显式补充保存 UUID 与字面名称。默认记录可保存原 Shell 字符串或规范 UUID；快照同时捕获自定义目录，新增、修改、隐藏、删除和恢复都会使旧保存快照失效。隐藏／删除的 UUID 保留为不可用，禁止回退；保存未启动，启动前仍重新核对实际默认身份。
读取同时捕获默认记录和自动 Profile 显示／删除偏好。快照绑定原 StateStore 实例；写入使用事务、完整快照比较和提交授权 gate，拒绝外部修改、旧 revision 和跨实例使用。未知或损坏记录报错，不自动修复或回退。

`workspace_profile_default_read/save/cancel` 复用 ProfileReader 的共享原生额度和有界、可撤销排队。后端根据自己的 PATH／SHELL 发现可选身份，页面只得到类型标识、当前选择、可用状态及一次性 ticket，不选择路径。票据绑定原注册表、原页、原目录与快照，60 秒到期，保存单次消费。保存重新发现候选；显示／删除变化使旧快照冲突。

显式取消、换页、关闭及期限到达撤销提交许可。读请求将资源保留对象交给 SQLite worker；保存 gate 保留对应资源。等待者取消不代表队列请求或 SQL 已退出，资源直到实际退出才释放，退出排空仍等待 worker。

## 不可用默认与界面生命周期

隐藏、删除或当前未发现的默认身份仍保存在偏好里，页面显示不可用，禁止默认启动，不选用其他 Shell。用户可以恢复显示、另选可见候选或清空默认项。

前端控制器对读取／保存使用有界等待，关闭或换页取消原票据，晚到读取只触发原票据撤销。未确认写入不自动重试或乐观宣布成功。每次表单重绘退休旧控件事件；未保存选择不能用于“启动默认 Profile”。显示偏好修改会退休默认票据，完成后重读。

默认启动先重新读取保存项，只有它仍可用且身份与用户刚看到的一致，才调用原有当前页面显式 自动 `workspace_session_open` 或保存 UUID 的 `workspace_custom_profile_open`。变化时显示新的状态，要求新一次明确点击。该检查不是新的启动授权，启动仍经过既有发现目录、Profile ticket、原页及根会话接收边界。
## 删除与显式恢复（Linux整批通过）

`change_profile_lifecycle` 使用同一个默认／显示／墓碑联合快照，在 SQLite 事务中固定删除或恢复操作，拒绝跨实例与旧来源。删除保留默认身份、去掉该身份的显示覆盖并写入墓碑；恢复必须已有墓碑，移除后仍需发现目录确定可用性。这两种操作均不创建或关闭已有会话。

墓碑实际变化时，默认记录的 revision 在同一事务内递增，包括旧的 `edit_auto_profiles` 与 `save_auto_profiles` 路径，因此删除→恢复不能使旧联合快照重新有效。损坏默认记录或版本耗尽使整个墓碑写入失败，不留下半次修改。新接口在写入两份记录后再次检查授权；等待者在 gate 内取消也不能把旧回执当成提交确认，原生额度保留到 worker 真正退出。

`workspace_profile_deleted` 只读已删除身份。`workspace_profile_lifecycle_prepare` 从后端目录和联合快照固定目标与操作，返回原页60秒一次性票据；`commit` 只接受票据，不接受替换目标或操作。`cancel`、导航和退出撤销提交许可。生命周期银行与默认选择银行隔离，使用同一个 Profile I/O 额度和退出屏障，默认票据不能升级成删除票据。

Linux 产品删除与显式恢复已完成整批验收。固定删除→恢复旧默认快照反例先红后绿，11项默认／生命周期存储、10项旧编辑兼容、4组原页反例及默认3组兼容通过。51项相关前端测试、tsc 和 Pyright覆盖固定目标与操作、票据银行隔离、关闭／过期／换页、迟到结果、默认编辑与取消后串行重读。Rust 当前诊断 Exact；TypeScript LSP Unknown 不计通过。公开 `probe-workspace-profile-lifecycle` 双进程实窗通过取消零写入、删除默认保留原PTY输入及刷新、墓碑跨进程持久化、明确恢复零启动、其他数据库内容不变及两次正常退出，确认／删除／恢复完成画面已审阅。原默认／隐藏实窗兼容、完整基础检查、1065前端、386 SSH存储／2集成／32文档、235组合原生、strict 与 bindings 全部通过，ignored 不计。

验收清单 `target/probes/profile-lifecycle-acceptance.json` 引用 `profile-lifecycle-{red,store,native,serial-window,merged,audit}.log` 及真实窗口证据。首轮取消后同时重读默认／墓碑竞争单槽失败，保留日志与截图；采用串行重读修复，没有增加额度、延长期限或自动重试提交。审计 exit 5 仅原有8项许可／2项维护阻断，发布门禁保持关闭。原生和构建退出后按 `profile-lifecycle-clean-dev.log` 清理开发产物并保留证据。该删除／恢复批次证据限 Linux，不代表 macOS／Windows 验收；后续参数、cwd、环境和外观编辑的边界与证据见 [Profile 编辑器](./profile-editor.md)，executable 仍由后端发现。


## 验证

- 5 组存储反例：重开、版本／联合快照冲突、隐藏／删除／清空、损坏记录、事务撤销，以及取消排队读仍持有资源直到 worker 退出。
- 原页原生 3 组行为、应用层 2 项及 workspace 严格检查通过；当前 Rust 诊断 Exact。
- 26 项相关前端协议、表单及显示偏好兼容测试、类型检查通过。
- `mise run probe-workspace-profile-default` 在 Linux 真 WebView 中通过保存零启动、刷新／双进程重开、隐藏默认不可启动、清空后递增版本、明确一次启动、输入及同 PTY 刷新、退出回收。其他数据库逻辑内容保持不变；保存／不可用／终端画面已审阅。证据：`target/probes/workspace-profile-default-result.json`。
- 原隐藏／恢复公开任务、完整基础检查、1036 前端测试、380 项 SSH 存储测试（6 忽略不计）、2 集成（1 忽略不计）／32 文档测试、231 组合 Native（4 忽略不计）、strict 与 bindings 均通过，见 `default-profile-merged.log`。审计 `default-profile-audit.log` exit 5 仅为 8 项许可／2 项维护阻断，发布门禁仍关闭。完整清单 `default-profile-acceptance.json` 已归档，退出后清理见 `default-profile-clean-dev.log`，证据保留。
- 本次窗口证据限 Linux，不等同于 macOS 启动文件或 Windows Shell／WSL／ACL 验收。
