# Windows SSH 产品接线

## 显式构建与当前状态

Windows 的 `workspace-ssh` 复用现有共享产品链，接通 Host／Identity／SSH Profile／组织元数据、Vault 密码与私钥写入、所选 Agent 身份、指纹观察与明确批准、保存／临时目标、密码／私钥／Agent、联合 MFA、跳板及本地 Root 会话注册。只扩大平台接线，不新增页面可传入的地址／程序／管道或通用签名接口。实际地址来自后端保存记录或独立批准的临时目标。

普通 `workspace` 仍不启用网络适配，默认 feature 仍为空；现有普通用户本地验证包的构建范围不变。Windows 原生 MSVC 工作流已有含 `workspace-cli,workspace-ssh` 的完整目标编译入口，但最终本地链工作区程序仍以 `workspace` 构建。显式 workspace-cli 的暖控制传输、固定同伴冷拉起及单实例已接线；Windows 二实例 argv 转发仍拒绝；启用 feature 名称不表示原生 CLI 使用链已验收。

Windows 含 SSH（workspace-cli,workspace-ssh）与不含 SSH（workspace）的全部桌面／测试目标 GNU 交叉检查均通过，既有平台警告保留，不当作 Windows 严格 lint 或原生执行。Linux 实际执行 20 项专项：信任票据4项、会话／MFA8项、Agent元数据2项、路线来源3项和Vault监督3项，均零失败／零忽略；包括真实peer的MFA、取消、错pin反例，以及原页面、SQL／KDF信用和退出等待。Linux正式组合全部目标严格Clippy、基础check及格式／diff检查通过。本批开发验证完成，原始日志、源码快照与清理记录见 `target/probes/windows-ssh-product/verification-v1.json`；Windows原生使用链未验收。

## 保持的授权与生命周期

页面来源、WindowToken、当前 nonce 和原页面代际先校验；保存凭据引用、Vault epoch 和来源快照继续绑定用户原点击。指纹观察不授权认证，必须使用实际连接的 peer 证据后才借出凭据。密码、私钥、Agent 和联合 MFA 仍为不同的明确意图，失败不切换方法或遍历其他密钥。软件 RSA 的既有阻断保持。

前端只获得有界元数据和终端投影，不获得密码、私钥、Agent 路径或签名能力。密码／私钥保存沿用生产私库与相同监督层，保存不自动启动连接。Windows SQLite 使用已有私有目录／主文件准入，目录及文件句柄随工作线程保留；本批不改变权限策略或修复已有 ACL。

连接与跳板沿用原 startup capacity、协议会话 supervisor 和 NativeIoCompletion。换页只撤销原页权限及挂载，不自动销毁根会话；取消等待方不提前释放物理 I/O 容量。窗口关闭继续等待具体请求、父连接、终端与原生资源排空，不能把 Actor 关闭或 future 取消当作完成。

## 平台边界与原生验收

- Windows Agent 只支持固定本机 OpenSSH 命名管道，见[Agent 身份](ssh-agent-identities.md)。自有管道夹具目前只完成交叉编译；用户 OpenSSH 服务、ACL、令牌与实际签名尚无 Windows 原生证据。
- SSH 配置读取／Include／确认与导入在显式 workspace-ssh 中接入 Windows，只读取后端选定的私有目录。核心共用限额、按需发现和快照确认，Windows 桌面适配复用私有目录／文件租约；默认仍为用户 .ssh，页面只提供别名。4 项同源原生夹具和完整桌面目标已交叉检查，尚未原生执行；详见[配置来源边界](ssh-config-import.md)。不读取 ProgramData 系统配置，不自动加入系统 known_hosts；普通 workspace 与本地验证包不开放 SSH。
- Linux 真实网络、页面或共享授权回归不外推 Windows；交叉编译不证明 Windows WebView2、SQLite、TCP／Agent 或退出行为通过。远程 MSVC 工作流尚未执行，暂无新的可分发包。

后续原生普通用户批次需覆盖：首次创建／解锁 Vault、保存身份、首次与变化指纹、密码／私钥／Agent 的明确方法、联合 MFA、代表性跳板、输入／resize／刷新保活，以及取消／关闭期间的实际连接和线程退出。环境与服务缺失必须记录为未覆盖／失败，不用认证回退或宽松信任补齐。完整CLI、性能、发行许可与签名门禁保持独立。
