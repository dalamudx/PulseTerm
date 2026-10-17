# 明确选择的 SSH Agent 身份

Agent 身份只保存名称、用户名、算法和完整 SHA256 公钥指纹，不保存私钥、套接字路径、环境变量或自动尝试列表。上层解析有界公钥并计算指纹；域模型的元数据不发行签名权限。

目录通过共享 SQLite worker 读取与 CAS 保存，旧数据库只读时不初始化 Agent 表。明确保存才在同一事务建立版本化目录，ID 为后端 UUIDv7，保留时间、HLC、修订和删除墓碑。完整记录比较拒绝同修订替换、外国实例及陈旧来源，最终撤权回滚。活动 SSH Profile 引用的身份不能删除；Profile CAS 同时比较密码、私钥与 Agent 三类目录。

原页面发现能力只使用后端 `SSH_AUTH_SOCK`；Unix 路径须为绝对 socket，文件 owner 和实际 peer uid 须匹配当前用户。请求及响应有界，Agent comments 不投射到页面。发现、元数据保存和实际 peer 签名分别授权。用户明确保存一把公钥后，只尝试保存的算法／SHA256 指纹，签名接口不向页面公开。Agent 拒签、取消、换页、epoch 失效或来源变化均失败，不回退密码、私钥或其他钥匙。

原页管理、共享启动槽、直接终端与跳板 driver 已接入。冻结路线包含三类身份，在每跳实际 fingerprint 批准后独立使用对应身份。Agent 中间用途只能转发到冻结下一目标，不能申请 PTY、shell 或任意地址；Agent 叶由终端 supervisor 注册及回收。根手势继承原页面／epoch／期限，保持到叶及父连接真正退出。

明确的保存直连 Agent + MFA 使用[独立联合意图](ssh-agent-interactive.md)，复用同一保存指纹、实际连接和原页／epoch 约束。普通 Agent 不因部分成功自动升级；验收当前状态以开发计划和验证记录为准。

有界恶意帧、超量／截断／非法公钥、撤权／拒签及所选 Ed25519 单次真实 SSH 认证检查通过；Store 完整来源／CAS／依赖删除、App 跨页／epoch／来源、原页 metadata 银行的撤权回滚与信用，以及三类型路线／中间用途反例通过。相关 Store／App／SSH／桌面严格检查与隐私文档反例通过。合并基础检查状态以最新计划为准。

Linux WebView 的直连验收通过两钥发现取消零写／零签名、保存第二把及名称更新各推进一次修订、删除取消零写、连接取消零签名、一次所选公钥签名且密码认证为零、终端固定行输入、最近连接、刷新不重认证、独立重开和正常 native 退出。组合路线通过密码外层、Agent 中间跳与 Agent 叶，所选公钥各签名一次，每跳实际描述与 SQL 地址／端口／指纹对应，刷新不重认证、独立重开和父资源退出。任务分别为 `probe-workspace-ssh-agent` 与 `probe-workspace-ssh-agent-route`，均登记 Linux CI；归档清单为 `target/probes/ssh-agent-acceptance.json`。

CLI Agent已完成Linux限定验收，复用原SavedGesture与startup bank：三目录唯一有效ID，原窗口显示Agent名称与精确目标，再明确批准；保持原click epoch／期限／撤销状态，最终使用实际peer。两项来源／epoch／碰撞／替换反例与workspace-cli/workspace-ssh全目标严格检查通过。真实CLI锁定／取消／导航／断连零签名，批准后仅所选第二把Ed25519一次签名、密码认证零；固定终端输入、刷新不重认证、最近连接独立重开和SDK流／Root排空正常，源及Agent目录不改。22项成功／失败证据SHA复核见 `target/probes/cli-agent-acceptance.json`。任务 `probe-workspace-cli-ssh-agent` 已登记Linux CI。

仅计 Linux GUI、本地软件 UNIX-domain Agent、Ed25519 和本地真实 SSH。CLI 跳板组合、跨平台 socket／管道、硬件令牌、其他算法、远端 Agent 转发与性能尚未验收。SQL 来源复核是有界确认，不持有整个签名网络操作的数据库锁，不承诺 SQL 与网络原子提交。基础 `probe-ssh-agent` 不替代生产页面验收，发行审计保持原策略。
