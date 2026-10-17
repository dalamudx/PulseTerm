# CLI SSH 跳板原手势交接

`pulse ssh` 仍只接受保存的入口 ID 和用户明确选择的身份 ID，不接受原地址、密码、Agent 套接字、转发地址或远端命令。三份身份目录必须恰好匹配一项，身份类型不回退。

CLI 请求在最初的原窗口页面捕获 Vault epoch 和撤权条件。共享 SQL worker 只读取一次联合来源，纯解析器由该不可变来源解析保存的 Profile 路线、每跳身份、Vault 与指纹目录；拒绝临时目标、已派生 hop 来源和跨 Store 实例。不为初始批准后再读出另一条路线。原窗口的初始目标预览使用被冻结叶节点的 Profile 用户名、精确地址及端口。

初始目标批准不批准任何跳的身份使用。原 capture 交给现有 route Bank，新增守卫只能收紧页面、epoch、期限和取消条件。页面收到 backend token 后使用 `continueSshRoute`，不重新 begin；每跳继续在原表单显示实际 peer、身份名称、指纹和下一目标并明确批准。首次 pin 的精确推进、每次身份借用和来源 CAS 沿用现生产路线边界。直连路径保留从原点击算起的 30 秒期限，路线 capture 仍有原来的有界期限。

银行与三类直连认证共享启动槽。CLI 断连、等待失败、换页或窗口关闭撤销 token，但真实 driver／SDK／底层 stream 和 SQL 操作退出前不释放信用。Root 注册成功与原页呈现分别确认：原页消费 Bank Done 后，CLI 才调用已注册 typed SessionId 的选择桥，沿用 owned-session 检查、原页 admission 与五秒选择 ACK；普通 focus-tab 仍独立验证其 PreparedWindowAction，SSH 批准不转授该动作。CLI 返回前等待原页真实选择确认，逐跳对话框先退出再 attach。注册后的未确认或取消不撤销 Root 已接管的会话，也不重新认证。

验证：纯快照／实例拒绝、原期限仅收紧与真实 Vault 原 epoch 反例、严格 native 检查、36 项客户端／对话框继承 token／取消／晚结果检查通过。新 native 文件的 Astrolabe 诊断 Exact 无错误；TypeScript LSP 不作为通过证据。真实 CLI 三 peer Linux WebView 已通过初始目标批准、首跳取消零认证／零写、三跳实际 pin、两次所选 Agent 签名、叶终端 `GUI_SSH_ECHO:R`、刷新不重拨、Recent 独立重开和父 stream／两进程退出。CLI `session 1` 回执发生在原页 inputReady 之后。39 份成功、合并、清理和保留故障证据的 SHA 归档见 `target/probes/cli-route-acceptance.json`，默认 check/test/probe 合并回归通过，1050项Rust与1187项前端，7项忽略不计；唯一测试格式问题已修复，不计之前失败为通过。手工续跑必须传 `PULSE_CLI_ROUTE=1`，不能用未读取的环境键代替，并核对本轮日志时间避免混用旧证据。`mise run probe-workspace-cli-ssh-route` 已登记此环境。该验收限 Linux 本地真实软件 Agent／SSH，不计其他平台、硬件令牌、IME、外部网络互通或性能；全部构建、窗口与原生任务退出后 clean-dev 回收22.7GiB，临时目录清空且证据保留。
