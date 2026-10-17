# 自动 Profile 参数与外观编辑

## 产品与启动边界

“本地 Profile → 管理 → 编辑”提供有序字面参数（保留空参数）、工作目录、环境覆盖／移除继承值和稀疏外观。字号、安全字体名和光标闪烁留空时继承当前显示设置；复制和粘贴偏好继续来自原设置。参数不做 Shell 展开，cwd 必须是绝对路径，不展开 `~`；目录不存在或 Shell 自己拒绝参数时启动失败，不选其他 Shell。用户可以显式保存 `-c` 等 Shell 参数，但不会在保存时执行。

编辑保存不启动或关闭终端。窗口、刷新、读取和独立进程重开也不自动启动。启动命令 `workspace_session_open` 仍只接受已发现 Profile 身份，前端不能在该请求中覆盖 executable、参数、cwd 或环境。当前产品编辑发现的 Unix 自动 Shell；独立 UUID 的自定义程序与 Profile 管理由[自定义 Profile](./custom-profiles.md)接通同一保存／启动监督链。Windows／WSL 和 macOS 启动文件验收仍未完成。

## 存储、授权与一致性

`pulse-store::profile_editor` 复用私有 SQLite `app_state` 和共享 worker，不变更数据库 schema。每个 `AutomaticShell` 有版本化 `profiles.editor.unix.<Shell>.v1` 记录（schemaVersion/revision/config）。读取事务同时捕获编辑记录、默认选择、可见性和墓碑；快照绑定原 StateStore 实例。保存比较整个来源、递增编辑 revision，并在同一事务推进默认／生命周期 revision，因此编辑→改回原配置或删除→恢复不能复活旧编辑票据。隐藏本身不会改变已运行会话；删除保留编辑配置，明确恢复后再次发现身份才可启动。

最多64个参数、64个环境条目，参数、cwd、环境和值与字体名合计16KiB UTF-8，序列化记录最多32KiB。环境名只接受 ASCII 标识符；大小写折叠后的重复名、覆盖／移除交叉、`TERM_PROGRAM`、`TERM_PROGRAM_VERSION` 和所有 `PULSE_` 名称拒绝。NUL、相对 cwd、非整数或超出6–72的字号、不安全字体名及未知字段拒绝。未来版本、损坏记录、版本耗尽或失败存储不会覆盖为默认值。配置是普通明文设置，不得保存凭据；Debug 只给出数量／存在性，不打印参数、环境、cwd 和字体内容。Vault 凭据保存与使用权限仍独立。

`workspace_profile_editor_read/save/cancel` 使用独立编辑银行，共用 ProfileReader 的有界 I/O 信用和退出屏障。读取根据后端 PATH／SHELL 获取候选，固定 Shell、原注册表、原页面、原目录、快照和60秒期限；保存只接原票据和有界配置，按原发现目录验证参数，单次消费，不接受替换目标；启动时重新发现身份并读取保存配置。页面、窗口、取消和期限撤销提交许可。gate 在 SQLite 排队、事务和提交前重检；后台任务保留信用直到真实 worker 退出，观察者取消不释放仍在使用的资源。不支持的平台明确拒绝。

前端读／存最多等待5秒，编辑55秒退休，不自动续期。取消或导航退休旧 DOM 和票据；迟到读取只撤销原票据。未确认保存显示不确定状态，禁止自动重发或乐观报告成功。管理页让默认、生命周期和配置编辑互斥，关闭编辑后按原串行链重新读取默认／墓碑。

## 原生选项与会话外观

`start_page_stored_configured_local` 在每次显式启动读取联合快照，验证配置后经原 ProfileCatalog/AutoProfileLaunch/PendingLocalLaunch 进入 PTY。exe 和进程环境标记由后端控制，原页面及注册／根交接授权不变。这是启动时配置快照，后续修改不取消已经注册的 PTY，也不追溯更新其 native 参数。

新增缺失cwd反例发现锁定portable-pty 0.9会用HOME替代不存在的显式目录。Linux修复使用O_PATH/O_DIRECTORY打开目录，持有句柄直到spawn完成，并用/proc/self/fd身份交给native chdir；目录路径随后重命名或被替换不会改选HOME。procfs不可用时拒绝。其他平台目前只做目录预检，原生路径竞态尚无跨平台验收；不得扩大Linux结论。两项目录层、三项编辑原生和八项原未知失败保留测试已通过；缺失cwd返回拒绝且正常退出回收，重命名／替换保持已打开目录。最终源码的公开Profile及兼容设置实窗复验通过；完整check/test/probe、SSH存储／CLI+SSH原生／strict／bindings及回调修正后补验均通过，归档 `target/probes/profile-editor-acceptance.json`。在线审计TLS失败，缓存仍有8许可／2维护阻断；TS LSP Unknown不计通过。

根会话保存同次启动读取的 ProfileAppearance，清理随 native 根会话回收。`workspace_terminal_prepare` 只在原页面和已有根会话／binding授权检查之后投影该快照。前端严格解码并把它合并入 Surface 显示配置，保留复制／粘贴设置及 CSP nonce；原 Raw Begin/Commit、捕获确认、resize和输入门不变。已有 live／Ended 会话刷新或重新附着继续使用原外观，后续编辑只影响新会话；继承字段仍读取该次显示的设置。外观数据不授权启动、输入或 clipboard。

## 当前验证

存储覆盖独立重开、跨实例／旧来源、删除恢复、取消事务、损坏／未来记录和有界字面内容。Native 使用真实 PTY 核对 Unicode、空参数、cwd 和环境字节，并检验取消／单次／原页面和固定外观。前端协议拒绝未知字段、无效绘制数据、迟到票据和重复保存，Raw renderer 合并不授予输入。

公开 `mise run probe-workspace-profile-editor` 和 Linux CI 门使用隔离私库、后端发现的 task-owned `sh`、实际 WebView／AT-SPI／键盘和原生进程收据。已通过取消／导航零写、保留名称拒绝、保存零启动、精确参数／cwd／环境移除字节、13→18实际内核网格92×31→67×22、同PID刷新／固定外观、独立进程重开、其他表内容不变及正常退出。原滚动可达性失败和 WINCH 打断 read 的夹具失败保留；调整为单一滚动区域、夹具继续等待，不放宽期限或权限。证据 `target/probes/workspace-profile-editor-result.json`，三张表单／终端／重开截图已审阅。

合并回归、审计和兼容实窗结果见 [验证记录](../development/validation.md)。这不是 macOS／Windows、系统 IME、网络互通、完整性能或 Alpha 发布验收。
