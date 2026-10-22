# Windows 生产存储前置验证

这是 `pulse-platform` 真实源码测试的独立运行包，不安装 Rust，不启动产品、不测延迟、不读既有 Profile／Vault，不修改系统策略。通过仅表示本轮原生存储前置检查通过，生产接入及性能仍须另行验证。

在真实64位Windows完整解压v3到新目录，同一账户的PowerShell执行：

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

当前回传中符号链接夹具创建报1314（缺少所需特权）。本轮建议在同一账户的**管理员PowerShell**复测，以满足该夹具前置条件；不要使用另一管理员账户，以免改变TokenUser和用户目录所有权。摘要中的`privilegeContext.elevatedAdministrator`记录当前管理员角色状态，无法读取时明确标记未知。测试不会自动提权、开启开发者模式或授予权限；管理员模式的测试证据不能当作普通用户产品启动验收。

发回脚本显示的`storage-results-*.zip`，失败也会归档。测试只操作本轮用户本地Temp下新建的固定夹具，退出后恢复TEMP／TMP并清理；未确认退出或删除失败时保留夹具并报告路径。每次进程最多120秒，超时仅终止本次进程。日志可能含本机路径，不含真实凭据。

脚本先核验SHA256及源码派生清单，必须发现全部22项Windows专属测试，再执行整套39项。零测试、缺项、忽略／过滤、非零退出、未知退出码或未确认清理都不算通过。符号链接权限不足保留失败，不跳过；Wine不算原生Windows证据。stdout／stderr、逐项原生结果、两阶段退出码和进程状态、计数、权限及清理状态均归档，Rust输出按UTF-8解码保存。

v2实机已完整执行39项：37通过、2失败。符号链接用例因1314未建立夹具；`held_parent_and_leaf_cannot_be_renamed`则实际发现持有目录句柄仍可改名。v3在目录打开权限中加入只读`FILE_LIST_DIRECTORY`，让不共享删除参与共享检查，继续拒绝写入和共享删除；回归同时要求持有时阻止改名／删除、释放后改名成功。v3实机已在管理员环境完整通过39项（含22项Windows专属），目录保护、符号链接与释放后改名回归均通过，退出0且夹具清理完成；普通用户生产接入仍须独立验证。v1发现阶段缺少退出码的记录及v2两项失败均保留。

验证还覆盖ACL创建／重开、祖先句柄、宽权限拒绝且不修复、只读及硬链接、有界读取、排他新建、精确快照、目录锁、旧快照／撤销／提交失败。原语不承诺完全TOCTOU保护或目录掉电持久性；页面、Vault和执行授权仍属于上层。

构建入口：`mise run windows-storage-kit`，产物为`target/windows-storage-kit/windows-storage-probe-v3.zip`。Linux需提供`CC_WIN`（MinGW-w64）和`.mise.toml`锁定版本的Windows GNU Rust标准库；隔离sysroot通过RUSTFLAGS传入。临时构建输出都在`/data/temp`，不改全局工具链。构建逐一校验Cargo.lock中的crate归档SHA256并提取完整独立许可／通知；摘要失败或许可缺失阻断交付，同时保留Rust标准库和GCC／MinGW原始材料。编译包清单含构建期依赖，为保守署名范围。

后续仍须接通Windows生产安全存储／Profile、后端选择的Shell和ConPTY，验证输入／resize／刷新／物理退出，再测本地PTY并单独测SSH。正式回显门槛为p95<16ms；独立WebView2页面约33ms的对照不等于产品达标。
