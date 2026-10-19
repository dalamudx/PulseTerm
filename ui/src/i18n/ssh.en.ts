export const sshEnglish = {
  "正在读取主机…": "Reading hosts…",
  "仅保存连接目标，不批准主机指纹或建立连接":
    "Saves connection targets only; host key trust and connections require separate approval.",
  尚未保存主机: "No saved hosts yet.",
  "主机列表不可用 · 未使用空列表替代数据库内容":
    "Host list unavailable. The stored catalogue has not been replaced with an empty list.",
  "编辑 SSH 入口": "Edit SSH entry",
  "选择 SSH 主机或添加主机": "Choose an SSH host or add a host",
  "添加 SSH 入口": "Add SSH entry",
  启用多入口管理: "Enable multiple SSH entries",
  显示名称: "Display name",
  主机地址: "Host address",
  端口: "Port",
  用户名: "Username",
  归属机器: "Machine",
  选择入口归属机器: "Choose the machine for this entry",
  新建机器: "New machine",
  添加: "Add",
  核对指纹: "Verify fingerprint",
  "请检查名称、地址、端口和用户名；不允许控制字符或隐形方向字符":
    "Check the name, address, port and username. Control and invisible direction characters are not allowed.",
  确认删除主机: "Confirm host deletion",
  机器名称: "Machine name",
  保存机器名称: "Save machine name",
  请检查机器名称: "Check the machine name.",
  "正在保存主机…": "Saving host…",
  "已保存 · 本地终端和主机指纹信任未改变":
    "Saved. Local terminals and host key trust are unchanged.",
  "保存未确认或版本冲突 · 不自动重试，关闭后重新打开以读取实际结果":
    "Save unconfirmed or version conflict. Close and reopen to read the actual result; no automatic retry.",
  "读取 SSH 配置…": "Reading SSH profiles…",
  "请先在主机管理中启用多入口管理。":
    "Enable multiple SSH entries in host management first.",
  "选择配置，修改名称、登录用户名和默认身份。":
    "Choose a profile to edit its name, login username and default identity.",
  "还没有 SSH 入口，请先添加主机。": "No SSH entries yet. Add a host first.",
  "配置读取未确认，请关闭后重新打开。":
    "Profile read unconfirmed. Close and reopen to try again.",
  选择SSH配置: "Choose an SSH profile",
  编辑SSH配置: "Edit SSH profile",
  配置名称: "Profile name",
  配置登录用户名: "Profile login username",
  配置默认身份: "Profile default identity",
  每次连接时选择: "Choose for each connection",
  "Agent 身份": "Agent identities",
  私钥身份: "Private key identities",
  密码身份: "Password identities",
  "（未设置凭据）": " (credentials not saved)",
  "原默认身份已删除，请重新选择":
    "The previous default identity was deleted. Choose another identity.",
  默认身份: "Default identity",
  "默认身份用于下次连接时预选，连接前仍可更改。":
    "The default identity is preselected for the next connection. You can change it before connecting.",
  配置SSH跳板: "Configure SSH jump host",
  直接连接: "Direct connection",
  "SSH 跳板": "SSH jump host",
  "可引用其他 SSH 配置，最多八跳。连接时需逐一确认每跳目标、身份和实际指纹。":
    "Reference another SSH profile, up to eight hops. Confirm the target, identity and actual fingerprint for each hop when connecting.",
  保存SSH配置: "Save SSH profile",
  "请核对配置名称、登录用户名、默认身份和跳板配置。":
    "Check the profile name, login username, default identity and jump profile.",
  "保存 SSH 配置…": "Saving SSH profile…",
  "SSH 配置已保存。": "SSH profile saved.",
  "保存未确认，配置或身份可能已变更。请关闭后重新打开检查。":
    "Save unconfirmed. The profile or identity may have changed. Close and reopen to check.",
  "当前主机：{0} · 当前身份：{1}": "Current host: {0} · Current identity: {1}",
  尚未选择: "Not selected",
  "默认身份不可用或用户名不匹配，请重新选择。":
    "The default identity is unavailable or its username does not match. Choose an identity.",
  "请选择连接身份。": "Choose an identity to connect.",
  确认临时SSH目标: "Confirm temporary SSH target",
  选择保存的SSH主机: "Choose a saved SSH host",
  "正在读取保存的主机与身份…": "Reading saved hosts and identities…",
  "所选主机已不在保存目录中，请关闭后重新读取。":
    "The selected host is no longer in the saved catalogue. Close and reopen to reload.",
  " · 未保存密码": " · No saved password",
  "{0} · {1} · 私钥{2}": "{0} · {1} · Private key{2}",
  " · 未保存凭据": " · Credentials not saved",
  核对所选主机指纹: "Check selected host fingerprint",
  "连接前请解锁保险库；连接将使用所选身份和已批准的主机指纹。":
    "Unlock the Vault before connecting. The connection uses the selected identity and approved host key fingerprint.",
  "请先添加主机和保存的 SSH 身份。":
    "Add a host and a saved SSH identity first.",
  "可以选择保存的主机；请先保存 SSH 身份。":
    "You can select a saved host. Save an SSH identity before connecting.",
  "无法读取保存的选择，请关闭后重试。":
    "Cannot read saved selections. Close and reopen to try again.",
  "正在读取 CLI 已批准路线的实际指纹…":
    "Reading the actual fingerprint for the route approved in the CLI…",
  "第 {0}/{1} 跳：{2}，{3}@{4}:{5}。身份 {6}。{7} {8}。{9}":
    "Hop {0}/{1}: {2}, {3}@{4}:{5}. Identity {6}. {7} {8}. {9}",
  "首次指纹，请核对后信任并连接。":
    "First-use fingerprint. Verify it before trusting and connecting.",
  "实际指纹匹配保存记录，确认使用该身份。":
    "The actual fingerprint matches the saved record. Confirm use of this identity.",
  信任并连接此跳: "Trust and connect this hop",
  确认连接此跳: "Confirm this hop",
  "正在验证主机并连接保存的身份…":
    "Verifying the host and connecting with the saved identity…",
  "主机指纹未批准或不匹配，请关闭后检查指纹。":
    "The host fingerprint is unapproved or does not match. Close and check the fingerprint.",
  "连接结果未确认，请关闭后刷新会话列表；不要重复连接。":
    "Connection result unconfirmed. Close and refresh the session list before attempting another connection.",
  "连接未成功，请检查已解锁的保险库、身份凭据和主机；未自动重试。":
    "Connection failed. Check the unlocked Vault, identity credentials and host. No automatic retry was attempted.",
  搜索与批量管理: "Search and bulk management",
  "搜索{0}": "Search {0}",
  选择当前搜索结果: "Select current search results",
  没有匹配项: "No matches",
  "选择 {0} · {1}": "Select {0} · {1}",
  "编辑 {0} · {1}": "Edit {0} · {1}",
  "删除所选（{0}）": "Delete selected ({0})",
  "删除 {0} 个{1}？{2}": "Delete {0} {1}? {2}",
  确认删除所选: "Confirm deletion",
  取消批量删除: "Cancel bulk deletion",
  "现有会话会继续运行。": "Existing sessions will continue running.",
  现有会话继续运行: "Existing sessions will continue running.",
  "名称和引用会被删除，已保存的凭据保留。现有会话会继续运行。":
    "Names and references will be deleted. Saved credentials are retained, and existing sessions will continue running.",
  "名称和引用会被删除，已保存的密文保留。现有会话会继续运行。":
    "Names and references will be deleted. Saved encrypted data is retained, and existing sessions will continue running.",
  "管理连接目标。不保存凭据，不修改主机指纹信任，也不启动 SSH 连接。":
    "Manage connection targets. This does not save credentials, change host key trust or start an SSH connection.",
  "为 SSH 入口保存独立的配置名称、登录用户名和默认身份。":
    "Save a separate profile name, login username and default identity for each SSH entry.",
  "预览 ssh_config": "Preview ssh_config",
  "连接 SSH 主机": "Connect to SSH host",
  主机: "Host",
  身份: "Identity",
  选择保存的SSH身份: "Choose a saved SSH identity",
  删除: "Delete",
  编辑: "Edit",
  取消: "Cancel",
  完成SSH配置: "Finish editing SSH profile",
} as const;
