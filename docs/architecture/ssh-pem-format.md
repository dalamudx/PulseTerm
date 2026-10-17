# SSH 私钥格式与算法

私钥导入及保存凭据认证共用 `ssh_private_key_decode::decode_bounded` 和同一个 CPU permit。支持 Ed25519 与 ECDSA NIST P-256、P-384、P-521 的 OpenSSH 格式、PKCS#8 `PRIVATE KEY` 与 `ENCRYPTED PRIVATE KEY` PEM。旧 Ed25519 配置和凭据继续可用。

输入继续受 ingress 字节预算与 4096 字节口令上限约束。PEM 只允许一个完整匹配标签的块；拒绝前置非空文本、多块、错标签和独立 PKCS#1／SEC1 PEM。明文块不接受附带口令；加密块要求非空口令。解码、解密和规范化后的密钥材料使用 SecretDocument 或零化容器。算法检查同时应用于 OpenSSH 外层和解密后的密钥；PKCS#8 在算法专用构造前检查 OID、命名曲线及密钥载荷大小。

加密 PKCS#8 在解密前检查 PBES2 / PBKDF2：迭代次数 1～600000，盐 8～64 字节，显式派生长度只能为 32；PRF 仅 HMAC-SHA256／SHA512，加密仅 AES-256-CBC／GCM。未知 KDF、旧式 PRF、异常参数或超预算先拒绝；错误口令及非法内容返回无内容的 Denied。原 OpenSSH bcrypt 上限仍为 64 轮。

软件 RSA 导入及认证保持拒绝。启用锁定库的 RSA 功能会引入受 `RUSTSEC-2023-0071` 影响的 `rsa 0.10.0-rc.18`，审计报告暂无修复版本，因此未启用该特性，也未增加审计豁免。外部 Agent 的已有公钥协议与此软件私钥能力分开验收。DSA、非 NIST 曲线、PKCS#1／SEC1、PPK 和其他 KDF／加密方案尚未交付。

新增算法复用既有独立接收、解析、加密保存及实际 peer 使用授权，不授予保存、解密其他凭据或连接权限，也不增加 GUI 可见原始私钥响应。加密保存保留原始格式，使用时临时解码，实际连接仍通过同一个有界解码入口，并在签名前核验原页、Vault epoch、元数据源及实际 peer。

`mise run test-ssh-authentication` 包含生成式格式检查：OpenSSH／明文和加密 PEM 的同一公钥身份、真实签名、三种 NIST 曲线、正确／错误口令及不支持算法拒绝。PBKDF2 参数、多块／错标签／超长输入等原有检查继续保留。

`mise run probe-ssh-private-key-algorithms` 使用隔离 SQLite 与真实 loopback SSH，对三个 NIST 算法分别保存加密 OpenSSH 和加密 PKCS#8 原始输入，共六组；验证密文文件重开、显式解锁、源变更拒绝、协议会话收养、刷新恢复、EOF／主动关闭和原生资源排空。数据库重开不升级旧 reader 的凭据：夹具关闭原 Owner 后创建新的 Owner，并重新执行显式解锁。探针结果按曲线与来源格式归档；不将 headless 结果当作原生 WebView、外部服务器或跨平台验收。
