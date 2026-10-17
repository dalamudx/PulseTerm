export const terminalEnglish = {
  确认终端粘贴: "Review terminal paste",
  "{0} 行，{1} UTF-8 字节。换行可能执行命令；Tab 和隐形字符可能改变含义。":
    "{0} lines, {1} UTF-8 bytes. Newlines can execute commands; tabs and invisible characters can change their meaning.",
  "粘贴内容预览（隐形字符转义显示）":
    "Paste preview (invisible characters escaped)",
  粘贴一次: "Paste once",
  "关闭会话 {0}？": "Close session {0}?",
  "会话中可能仍有程序运行。关闭会终止会话连接并回收本地原生资源，不是仅隐藏标签。":
    "Programs may still be running. This terminates the session connection and releases its local native resources; it does not just hide a tab.",
  保留会话: "Keep session",
  检查终端文件路径: "Review terminal file path",
  检查终端链接: "Review terminal link",
  "路径来自终端输出，可能位于远程主机。包围路径的引号不计入复制内容；变量和反斜线按字面保留。请核对后复制，此操作不会读取本机文件或调用编辑器。":
    "This path comes from terminal output and may refer to a remote host. Surrounding quotes are excluded when copying; variables and backslashes remain literal. Review before copying. This does not read local files or launch an editor.",
  "链接来自终端输出。IP:端口按 HTTP 网页地址解释，端口不代表服务一定支持 HTTP。请核对完整目标，再复制地址或交给系统默认浏览器处理。":
    "This link comes from terminal output. IP:port is interpreted as an HTTP address; the port does not imply HTTP support. Review the full target before copying it or opening it in the default browser.",
  实际文件路径: "Actual file path",
  实际目标地址: "Actual target address",
  "Windows 盘符路径": "Windows drive path",
  "Unix 绝对路径": "Unix absolute path",
  " · 行 {0}": " · line {0}",
  " · 列 {0}": " · column {0}",
  "主机：{0} · 协议：{1}": "Host: {0} · protocol: {1}",
  "此目标不可用：仅接受不含凭据的 HTTP／HTTPS 地址。":
    "This target is unavailable: only HTTP/HTTPS addresses without credentials are accepted.",
  复制路径: "Copy path",
  复制地址: "Copy address",
  在系统浏览器打开: "Open in browser",
  "行列位置单独显示，复制内容仅含路径。尚不支持外部编辑器打开。":
    "Line and column are shown separately; only the path is copied. Opening an external editor is not supported yet.",
  "此视图暂不支持系统打开，可复制地址。":
    "Opening the system browser is unavailable in this view. You can copy the address.",
  "路径已复制。": "Path copied.",
  "地址已复制。": "Address copied.",
  "复制未完成，可选中地址后使用系统复制。":
    "Copy did not complete. Select the address and use the system copy command.",
  "正在请求系统浏览器…": "Requesting the system browser…",
  "已交给系统默认浏览器处理。": "Handed to the default browser.",
  "当前平台尚不支持系统打开，可复制地址。":
    "Opening the system browser is not supported on this platform. You can copy the address.",
  "未能交给系统浏览器，可重试或复制地址。":
    "Could not open the system browser. Retry or copy the address.",
  "终端响铃 · 清除": "Terminal bell · dismiss",
  清除终端响铃提示: "Dismiss terminal bell notice",
  回到最新输出: "Return to latest output",
  "当前会话已变化，请关闭后重新打开历史。":
    "The session has changed. Close and reopen history.",
  "终端内容已变化，当前页已保留。请点“最新历史”重新读取。":
    "Terminal content has changed. The current page is retained. Select “Latest history” to read again.",
  "终端正在处理其他操作，请稍后重试。":
    "The terminal is busy. Try again later.",
  "无法读取历史，请确认会话仍可查看后重新读取。":
    "Could not read history. Confirm the session is still available and read again.",
  终端历史与查找: "Terminal history and search",
  "查找历史与主屏或当前备用屏，支持跨软换行，不跨硬换行。整词按 Unicode 单词边界，连续中文不自动分词。整词和正则每次定位一条完整逻辑行，不支持回溯引用或环视。查找不会向终端发送内容。":
    "Search history and the primary or current alternate screen across soft wraps, not hard line breaks. Whole-word matching uses Unicode word boundaries; continuous Chinese is not segmented automatically. Whole-word and regular expression search locate one complete logical line at a time; backreferences and lookaround are unsupported. Searching does not send terminal input.",
  查找范围: "Search scope",
  历史与主屏: "History and primary screen",
  备用屏: "Alternate screen",
  查找文字: "Search text",
  查找历史与主屏: "Search history and primary screen",
  查找备用屏: "Search alternate screen",
  区分大小写: "Match case",
  正则表达式: "Regular expression",
  整词匹配: "Whole word",
  "按 Unicode 单词边界匹配；连续中文不自动分词。":
    "Match Unicode word boundaries; continuous Chinese is not segmented automatically.",
  上一个: "Previous",
  下一个: "Next",
  "较早的匹配（Shift+Enter）": "Earlier match (Shift+Enter)",
  "较新的匹配（Enter）": "Later match (Enter)",
  当前页历史文本: "History text on this page",
  更早: "Older",
  较新: "Newer",
  最新历史: "Latest history",
  "正在读取历史…": "Reading history…",
  "尚无保留历史；输入文字仍可查找主屏。":
    "No retained history yet. Enter text to search the primary screen.",
  "查找片段：第 {0}–{1} 行（含主屏）；点“最新历史”返回翻页。":
    "Search context: rows {0}–{1}, including the primary screen. Select “Latest history” to return to paging.",
  "第 {0}–{1} 行，共保留 {2} 行{3}": "Rows {0}–{1} of {2} retained rows{3}",
  "；更早内容已按容量上限清理":
    "; older content was removed at the capacity limit",
  "请输入不含控制字符的查找文字，最多 512 字节。":
    "Enter search text without control characters, up to 512 bytes.",
  "正在查找{0}…": "Searching {0}…",
  "备用屏第 {0} 行": "alternate screen row {0}",
  "历史第 {0} 行": "history row {0}",
  "主屏第 {0} 行": "primary screen row {0}",
  "匹配{0}{1}。{2}": "Match at {0}{1}. {2}",
  "至{0}": " through {0}",
  "片段超过一页，仅显示末尾部分。":
    "The match exceeds one page; only the final section is shown.",
  "已到{0}内容，没有更多匹配。": "Reached the {0} content. No more matches.",
  最早: "earliest",
  最新: "latest",
  "{0}中没有匹配。": "No match in {0}.",
  "正在查找{0}… {1} / {2} 行。": "Searching {0}… {1} / {2} rows.",
  "终端内容或会话已变化，当前页已保留。请重新查找。":
    "Terminal content or the session has changed. The current page is retained. Search again.",
  "正则表达式无效或超过编译限制。请修改表达式，当前页已保留。":
    "The regular expression is invalid or exceeds compilation limits. Edit it; the current page is retained.",
  "内容超过单次查找预算，当前页已保留。可关闭正则和整词匹配后重试，或缩小范围。":
    "The content exceeds the search budget. The current page is retained. Disable regular expression and whole-word matching or narrow the scope before trying again.",
  "备用屏未启用或保留内容不可用，当前页已保留。请选择历史与主屏，或启用备用屏后重新查找。":
    "The alternate screen is inactive or retained content is unavailable. The current page is retained. Select history and primary screen, or activate the alternate screen and search again.",
  "查找未完成，当前页已保留。请稍后重新查找。":
    "Search did not complete. The current page is retained. Search again later.",
  "允许 CLI 新建窗口？": "Allow CLI to create a window?",
  "创建一个空工作区。已有终端保留在原窗口；新窗口启动终端时会另行确认。":
    "Create an empty workspace. Existing terminals stay in their original window; starting a terminal in the new window requires separate confirmation.",
  空工作区: "Empty workspace",
  新窗口: "New window",
  新建窗口: "Create window",
  "为冷分屏创建首个终端？": "Create the first terminal for a new split?",
  "允许 CLI 创建分屏终端？": "Allow CLI to create a split terminal?",
  "允许 CLI 创建本地终端？": "Allow CLI to create a local terminal?",
  "先创建并呈现一个本地终端，再单独确认第二个分屏终端。取消第二步会保留首个会话；取消此步不会启动。":
    "Create and show the first local terminal, then confirm the second split terminal separately. Cancelling the second step retains the first session; cancelling this step starts nothing.",
  "CLI 请求启动已保存或后端发现的本地 Profile。确认后创建一个保留会话；取消或未确认不会启动。已有会话不受影响。":
    "CLI requests a saved or backend-discovered local Profile. Confirm to create a retained session; cancelling or leaving it unconfirmed starts nothing. Existing sessions are unaffected.",
  创建首个终端: "Create first terminal",
  创建分屏: "Create split",
  创建终端: "Create terminal",
  "允许 CLI 连接保存的 SSH 主机？": "Allow CLI to connect to a saved SSH host?",
  "仅使用请求时已确认的 Vault 密钥和保存目标。仍须通过该连接的指纹验证；取消不会认证或创建会话。":
    "Use only the Vault key and saved target confirmed when requested. The actual connection fingerprint must still be verified; cancelling does not authenticate or create a session.",
  保存的主机与身份: "Saved host and identity",
  "{0} · {1}:{2}\n{3} · {4}\n主机 {5}\n身份 {6}":
    "{0} · {1}:{2}\n{3} · {4}\nHost {5}\nIdentity {6}",
  "查看 CLI 请求的 SSH 目标？": "Review the SSH target requested by CLI?",
  "查看保存的主机和身份后，仍需确认连接目标及实际指纹。":
    "After reviewing the saved host and identity, you still need to confirm the connection target and actual fingerprint.",
  "先解锁保险库，关闭保险库窗口后再次点击查看目标。解锁不会连接主机。":
    "Unlock the Vault first, then close it and review the target again. Unlocking does not connect to the host.",
  "保存的 SSH 入口：{0}": "Saved SSH entry: {0}",
  "主机 {0}\n身份 {1}": "Host {0}\nIdentity {1}",
  请求的保存记录: "Requested saved records",
  查看目标: "Review target",
  树视图: "Tree view",
  列表视图: "List view",
  分组: "Groups",
  列表: "List",
  最近: "Recent",
  最近连接: "Recent connections",
  搜索主机或标签: "Search hosts or tags",
  搜索主机树: "Search host tree",
  仅收藏主机: "Favorite hosts only",
  "主机目录读取失败，请重新加载工作区。":
    "Host directory unavailable. Reload the workspace.",
  "最近连接读取失败，请重新加载工作区。":
    "Recent connections unavailable. Reload the workspace.",
  "正在读取最近连接…": "Reading recent connections…",
  没有匹配的最近连接: "No matching recent connections",
  "尚无成功的 SSH 连接": "No successful SSH connections yet",
  没有匹配的主机: "No matching hosts",
  "选择主机 {0} · {1}": "Select host {0} · {1}",
  "最近连接：{0}": "Last connected: {0}",
  未分组: "Ungrouped",
  "会话 {0}": "Session {0}",
  "{0} · 会话 {1}": "{0} · Session {1}",
  "无法读取命令目录 · 原有按钮仍使用各自的权限检查":
    "Command directory unavailable · Existing buttons retain their permission checks",
  "键位来源不可用 · 未替换为默认键位。":
    "Keybinding source unavailable · Default bindings have not been substituted.",
  "无法打开快捷键设置 · 请重试": "Cannot open keyboard shortcuts · Try again",
} as const;
