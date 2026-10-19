# Windows 私有目录边界

当前实现位于 `pulse-platform::windows::PrivateDirectory`。这是安全存储的基础原语，尚未接入生产 Profile／SQLite／设置文件；Windows 设置原子替换、文件级 ACL／链接检查、IPC 和实际生产纵向链仍未完成。交叉编译不计为 Windows ACL 或原生运行验收。

## 准入与生命周期

- 只接受本地固定磁盘、支持持久 ACL 的卷和有界的绝对路径；拒绝 UNC／设备路径、相对路径、点段、ADS、DOS 设备名及尾随空格／点别名。初始 UTF-16 分配和组件数量均有上限。
- 逐层打开目录，使用 `OPEN_REPARSE_POINT` 检查每一层，不跟随重解析点或卷挂载点。句柄只申请安全描述符和文件属性读取权限，不共享删除；保留祖先句柄直到对象释放。
- 所有者来自真实进程 TokenUser，不读取环境用户名。祖先所有者限当前用户、SYSTEM、Administrators 或 TrustedInstaller；非可信主体只允许读取／遍历／添加新项，不得修改或删除现有项、改 ACL 或改所有者。私有叶目录必须归当前用户、DACL 受保护，且显式赋予当前用户完整访问。
- 叶目录只允许当前用户、SYSTEM、Administrators 的生效允许 ACE；缺失／NULL／空 DACL、未知 ACE、叶目录拒绝 ACE、非可信授权及畸形 SID／ACL 全部拒绝。继承专用 ACE 不当作当前目录授权，私有叶目录也拒绝其向外部主体传播权限。TrustedInstaller 不获得私有叶目录访问。
- `create` 仅创建最后一级，父目录必须已经存在；创建调用同时传入受保护安全描述符，避免“先公开创建、再收紧权限”。已存在对象只验证，不改所有者或权限。
- `validate` 同时复核保留句柄与按原路径重新打开的64 位卷号／128 位文件 ID（不支持该查询时拒绝）、属性、所有者和 ACL。不声称完全消除 TOCTOU；当前账户和本机系统／管理员属于信任边界，ACL 变更不是原子事务。

这些 API 同步阻塞，只可放在专用线程／受监督 I/O 中使用。对象不暴露原始句柄、路径读写能力或安全描述符，也不派生 Debug／serde；错误仅包含固定码，不包含路径或 SID。目录准入不授予页面、Vault、凭据使用或执行权限。

## 平台 FFI

沿用 `overview.md` 已有规则：只有 `pulse-platform` 可使用平台 FFI。crate 默认 `deny(unsafe_code)`，只有私有 `windows::sys` 模块允许；每个不安全调用都有 SAFETY 说明。其余 crate 的 forbid 规则不变。

只复用锁文件已有 `windows-sys 0.61.2`，没有引入调查中的权限封装。Token 句柄由 OwnedHandle 回收；系统分配的描述符由 LocalFree 守卫持有。Token 缓冲按原生结构对齐、限制大小；描述符先验证自相对格式，再将原生指针转为有边界的切片；ACE／SID 解析使用安全 Rust。权限或身份变化不会触发自动修复。

## 验证门槛

正式入口为 `mise run check-platform`：当前宿主全目标严格 Clippy 和平台单测。Linux／macOS 只运行纯 ACL／路径策略测试；Windows 额外注册真实创建、重开、句柄阻止改名、宽权限原目录不修复、文件／缺失父目录及非法路径测试。Windows 专属测试不在其他平台伪装通过，不使用零测试命令作为原生证据。

Windows GNU 交叉检查可验证 FFI 签名和所有测试目标能编译，不能证明 NTFS／ACL 实际语义。CI 三平台已接入正式入口，但当前主机无 Windows／macOS 实机、仓库无 remote，远端运行结果仍待取得。在真实 Windows 测试及文件级持久化接口完成前，生产入口继续拒绝不支持的平台，不以该目录原语开放 Windows 产品流程。
