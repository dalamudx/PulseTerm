export const connectionEnglish = {
  "{0} · {1}:{2}\n{3} · {4}\n主机 {5}\n身份 {6}\n私钥 + MFA（两种因素均须成功）":
    "{0} · {1}:{2}\n{3} · {4}\nHost {5}\nIdentity {6}\nPrivate key + MFA (both factors must succeed)",
  "主机 {0}\n身份 {1}\n认证方式：私钥 + MFA（两种因素均须成功）":
    "Host {0}\nIdentity {1}\nAuthentication: Private key + MFA (both factors must succeed)",
  "{0} · {1} · 私钥 + MFA（直连）": "{0} · {1} · Private key + MFA (direct)",
  保存的密码: "Saved password",
  保存的私钥: "Saved private key",
  "私钥 + MFA": "Private key + MFA",
  "{0} · {1} · 私钥 + MFA": "{0} · {1} · Private key + MFA",
  "{0} · {1} · Agent + MFA": "{0} · {1} · Agent + MFA",
  认证方式: "Authentication method",
  "交互认证 / MFA": "Interactive / MFA",
  "SSH 交互认证": "SSH interactive authentication",
  "{0} · {1}:{2}\n{3} · {4}\n主机 {5}\n身份 {6}\n交互认证 / MFA（不读取保存密码）":
    "{0} · {1}:{2}\n{3} · {4}\nHost {5}\nIdentity {6}\nInteractive / MFA (no saved password read)",
  "主机 {0}\n身份 {1}\n认证方式：交互认证 / MFA":
    "Host {0}\nIdentity {1}\nAuthentication: Interactive / MFA",
  "交互认证 / MFA（不读取保存密码）":
    "Interactive / MFA (no saved password read)",
  "{0} · {1} · 交互认证 / MFA": "{0} · {1} · Interactive authentication / MFA",
  服务器认证问题: "Server authentication questions",
  "以下内容由服务器提供。仅在核对目标后回答；取消将关闭待认证连接。":
    "The following content comes from the server. Answer only after verifying the target; cancelling closes the pending connection.",
  提交本轮回答: "Submit answers for this round",
  "回答超出长度限制，请缩短后再提交。":
    "An answer exceeds the length limit. Shorten it before submitting.",
  "交互认证暂不支持临时目标或跳板路径，请选择直接连接的保存主机。":
    "Interactive authentication currently requires a saved host with a direct connection. Temporary targets and jump routes are not supported.",
  保存的主机: "Saved host",
  目标: "Target",
  密钥算法: "Key algorithm",
  "SHA-256 指纹": "SHA-256 fingerprint",
  "正在核对保存的主机指纹 · 不发送认证":
    "Checking the saved host fingerprint · No authentication sent",
  "指纹核对未确认 · 不自动重试，关闭后重新核对":
    "Fingerprint check unconfirmed · Close and check again; no automatic retry",
  "首次见到此密钥。请通过独立可信渠道核对，保存仅添加应用指纹记录，不启动连接。":
    "This key is new. Verify it through an independent trusted channel. Saving only adds an application fingerprint record and does not start a connection.",
  "主机密钥已变更 · 本入口禁止替换。请通过可信渠道核对现有记录。":
    "Host key changed · Replacement is blocked here. Verify the existing record through a trusted channel.",
  "已匹配应用保存的指纹 · 本次仅核对，没有认证或连接。":
    "Matches the fingerprint saved in this application · Checked only; no authentication or connection.",
  "正在保存此指纹 · 不自动连接":
    "Saving this fingerprint · No automatic connection",
  "已保存此指纹 · 没有启动 SSH 连接":
    "Fingerprint saved · No SSH connection started",
  "保存未确认或来源已变化 · 不自动重试，关闭后重新核对实际记录":
    "Save unconfirmed or source changed · Close and check the actual record again; no automatic retry",
  "SSH 主机指纹": "SSH host fingerprint",
  "仅核对应用保存的目标与公开密钥，不读取系统 known_hosts，不发送凭据，也不启动 SSH 会话。":
    "Check the application's saved target and public key only. System known_hosts is not read, credentials are not sent, and no SSH session starts.",
  保存指纹: "Save fingerprint",
  "配置源已变化，请重新预览。旧值保留供核对，不能保存。":
    "Configuration source changed. Preview again. Previous values remain for comparison and cannot be saved.",
  "源状态未确认，请重新预览后保存。旧值保留供核对。":
    "Source status unconfirmed. Preview again before saving. Previous values remain for comparison.",
  "名称已修改，请重新预览。": "Name changed. Preview again.",
  "输入 Host 别名以读取配置。": "Enter a Host alias to read its configuration.",
  "IdentityFile 提示": "IdentityFile hints",
  "{0} 项（不读取密钥）": "{0} entries (keys are not read)",
  "请输入 Host 别名，不包含空格或通配符。":
    "Enter a Host alias without spaces or wildcards.",
  "正在读取配置…": "Reading configuration…",
  "已读取。可以核对源文件是否仍与本次预览一致。":
    "Configuration read. You can check whether its source files still match this preview.",
  "配置不可用或读取未确认，请检查文件后重新预览。不会使用空配置。":
    "Configuration unavailable or read unconfirmed. Check the files and preview again. Empty configuration will not be used.",
  "正在核对源文件…": "Checking source files…",
  "本次预览的源文件未变。没有保存主机或建立连接。":
    "This preview's source files are unchanged. No host was saved or connection started.",
  "配置已变化或核对未确认，请重新预览。没有保存主机或建立连接。":
    "Configuration changed or check unconfirmed. Preview again. No host was saved or connection started.",
  "正在准备保存…": "Preparing to save…",
  "已准备主机“{0}”。保存后可在 SSH 主机中查看，不会建立连接。":
    "Host “{0}” is ready. After saving, find it in SSH hosts. No connection will be started.",
  "准备未确认，请重新预览。没有保存主机。":
    "Preparation unconfirmed. Preview again. No host was saved.",
  "已保存主机“{0}”。可在 SSH 主机中查看。":
    "Host “{0}” saved. Find it in SSH hosts.",
  "保存未确认或来源已变化，请先查看主机目录，再重新预览。不会自动重试。":
    "Save unconfirmed or source changed. Check the host list, then preview again. No automatic retry.",
  "读取本机 SSH 配置，核对连接参数后可保存为主机。IdentityFile 仅提供数量提示，凭据需另外选择。":
    "Read local SSH configuration and check its connection parameters before saving a host. IdentityFile provides counts only; select credentials separately.",
  "Host 别名": "Host alias",
  保存的主机名称: "Saved host name",
  准备保存: "Prepare to save",
  保存主机: "Save host",
  核对源文件: "Check source files",
  预览: "Preview",
  "正在读取保存的主机…": "Reading saved hosts…",
  "输入地址后核对目标，再保存或使用已有主机。":
    "Enter an address and check the target, then save or use an existing host.",
  "目录读取失败，请关闭后重新打开。":
    "Host list could not be read. Close and reopen.",
  "多个已保存主机使用此地址，请从主机列表选择。":
    "Several saved hosts use this address. Select one from the host list.",
  "用户名：{0}\n主机：{1}\n端口：{2}\n名称：{3}":
    "Username: {0}\nHost: {1}\nPort: {2}\nName: {3}",
  "将使用已保存主机，随后选择身份并核对指纹。":
    "Use the saved host, then select an identity and verify the fingerprint.",
  "将保存新主机，随后选择身份并核对指纹。":
    "Save a new host, then select an identity and verify the fingerprint.",
  使用此主机: "Use this host",
  保存并继续: "Save and continue",
  "请输入 user@host:port；IPv6 使用方括号。不支持密码、路径或其他协议。":
    "Enter user@host:port; use brackets for IPv6. Passwords, paths, and other protocols are not supported.",
  "正在核对临时目标…": "Checking temporary target…",
  "临时目标未确认，请关闭后重新核对地址。未开始连接。":
    "Temporary target unconfirmed. Close and check the address again. No connection started.",
  "正在确认保存的主机…": "Confirming saved host…",
  "保存未确认或目录已变化，请关闭后重新读取。已有保存可能已完成，不会重试或连接。":
    "Save unconfirmed or host list changed. Close and read again. Saving may have completed; no retry or connection will start.",
  "快速连接 SSH": "Quick SSH connection",
  "核对地址后可临时连接，连上后选择保存。也可先保存或使用已有主机，再选择身份并确认连接。":
    "Check the address for a temporary connection, then choose whether to save it after connecting. You can also save first or use an existing host, then select an identity and confirm the connection.",
  连接地址: "Connection address",
  快速连接地址: "Quick connection address",
  "名称（可选）": "Name (optional)",
  快速连接名称: "Quick connection name",
  取消快速连接: "Cancel quick connection",
  核对地址: "Check address",
  临时连接: "Connect temporarily",
  保存此连接: "Save this connection",
  临时连接保存名称: "Name for saved temporary connection",
  暂不保存: "Not now",
  保存此主机: "Save this host",
  "连接已建立。保存后可从主机列表重新连接。":
    "Connection established. Save this host to reconnect from the host list.",
  "请输入有效的主机名称。": "Enter a valid host name.",
  "保存结果未确认，请关闭后检查主机列表。不会重复保存，已有连接继续运行。":
    "Save result unconfirmed. Close and check the host list. Saving will not repeat; the existing connection keeps running.",
} as const;
