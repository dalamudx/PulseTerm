# 冷 CLI 保存 SSH 意图

`pulse ssh <保存的入口 ID> --identity <保存的身份 ID>` 在没有现存端点时可启动独立 GUI。启动参数只携带两个非空保存 ID，不携带地址、密码、钥匙、Agent socket、自动解锁参数或远端命令。冷分屏通过独立的双确认流程创建初始目标，见 [冷 CLI 分屏](cli-cold-split.md)，当前真实窗口验收尚未完成；冷 SSH 的意图确认不授予分屏或任意冷命令权限。

冷启动与暖请求的授权时刻不同。暖请求继续在最初点击捕获 Vault epoch。冷请求先发行不可复制、不可 Debug 的 `ColdSavedSessionIntent`：只保留 owner 弱引用、原页面、两个精确 ID、原取消条件和最多 30 秒的截止；不捕获 epoch，不授予 SQL、CPU、网络或凭据权限。原启动期限只能收紧。

原窗口显示查看目标邀请。保险库锁定时，用户可选择打开保险库；保险库关闭后仍需再次明确点击查看目标。解锁本身不确认意图。批准银行在处理这个点击的原生调用中同步消费意图，并核对 owner 实例、同一原页面、取消和截止，捕获当次已解锁 epoch。不能等到异步等待者恢复后再捕获一次新 epoch。未解锁、换页、取消、过期或回执等待已退出时拒绝确认。

确认只允许读取原保存来源。联合 SQL 快照解析精确入口和三份身份目录的唯一身份，随后仍需要原窗口的精确目标批准以及现有实际 peer 指纹／身份使用链。私钥、密码、Agent 和跳板不互相回退。再次锁定或重新解锁会使已经确认的旧 epoch 失效。冷意图批准不授权连接，也不转授普通 focus-tab 动作。

显式 `--interactive` 只选择保存账号的交互认证方法，不携带任何回答。冷邀请和精确目标预览均显示 MFA；原意图确认仍只捕获当次原页 epoch，方法转换不续期、不读取保存密码。问题、回答及取消回执见 [CLI SSH 交互认证](cli-interactive-ssh.md)；跳板交互方式继续明确拒绝。

GUI 先释放 metadata 邀请的 pending 槽，后端才能发行精确目标邀请；Vault 等待关闭的监听在关闭、取消和处置时清除。CLI 成功回执仍等待 Root 注册和原页 owned-session 选择 ACK。取消等待不提前释放正在运行的 SQL、SDK 或 native 信用；已交给 Root 的会话不因 CLI 等待结束而被重拨。

Linux 密码冷启动的真实窗口链已验证：由三个独立 CLI 请求启动 GUI，锁定取消和解锁后取消均零认证／零来源写入，解锁后仍停在明确确认；最终精确目标批准后认证一次。刷新前后固定原文均由 peer 回显，生产 `Ctrl+Shift+R` 刷新未重拨，三个 GUI、所有端点和 peer stream 正常退出。两项真实 Vault 意图反例、28 项相关前端、5 项原生批准银行和 10 项启动边界检查已通过。默认 check/test/probe 通过：1050 项 Rust、1191 项前端，7 项忽略不计；审计退出 5，许可证与维护策略仍阻断。26 份不可变 SHA 证据见 `target/probes/cold-ssh-acceptance.json`；所有构建／窗口／native 退出后 clean-dev 回收18.4GiB，本批临时目录清空。

真实探针使用既有 fixture 的专用私有目录及所有权哨兵；截图失败不能跳过子进程和目录清理。CLI回执等待覆盖后端既有期限，不延长授权期限。冷Agent的Linux本地软件二钥实窗确认锁定／解锁后取消零签名零写，独立点击及精确目标批准后只预存第二公钥签名一次、零密码。冷三跳确认首跳取消零认证、每跳实际地址／身份／本轮服务器指纹与SQL匹配；外跳password一次，中／叶所选Agent各签一次，SQL只新增两条跳板pin，已知叶pin保持不变。最终共用harness的三种冷模式和默认check均通过，生产刷新前后原文回显、不重拨，所有GUI／端点／叶与父连接／转发流退出；1050项Rust、1191项前端与基础probe回归参考复核归档，7项忽略不计。Agent24份、三跳33份SHA见 `target/probes/cold-agent-acceptance.json`、`target/probes/cold-agent-route-acceptance.json`；全退出后clean-dev回收10.6GiB，本批临时目录清空。fixture Clippy all通过但保留unused告警，Astrolabe引用Unknown未计通过。多窗口、硬件、外网、跨平台、系统IME和性能尚未验收；冷split的Linux六模式已验，见 [冷CLI分屏](cli-cold-split.md)；发行审计仍阻断。
