# SSH 跳板联合来源

`pulse-store::ssh_route_source` 通过共享 SQL worker 读取及复核整条链；读取本身不拨号、不解锁、不返回明文，也不发行 target-use、实际 peer 或执行批准。GUI 生产接入与 Linux 验收见 [生产路由](ssh-production-route.md)；配置保存见 [跳板配置边界](ssh-jump-preferences.md)。

`load_ssh_route_source` 接受精确的最终 SSH Profile ID 与显式的身份种类／保存 ID。在一个 Deferred WAL 事务读取 Host／Endpoint／Profile 图，密码、私钥和 Agent 三类身份目录及墓碑，Vault 密文和原始 trust。最多八个跳板加一个目标，按最外跳到目标排列。每跳固定入口 ID、规范地址／端口、Profile 用户名、身份种类与 ID。中间跳必须有唯一有效默认身份；跨目录 ID 歧义、墓碑、缺失或用户名不符均拒绝。最终身份由用户明确选择，不使用列表首项或机器默认入口回退。

`validate_ssh_route_source` 在一个 Immediate 事务重读完整来源，比较整个图、三类身份目录的内容和修订、Vault 捕获及原 trust，包括未选择的行和隐藏墓碑；同修订替换同样拒绝。来源绑定 store 实例，重开不能复用旧来源。原页面谓词在排队、读取和发布边界核对 scope、原 epoch 与撤权；谓词 panic、worker 关闭和取消均拒绝发布，取消等待仍持有 I/O 信用直到 worker 实际退出。

`hop_source(index)` 只能派生当前精确 hop 的来源及角色。普通直连来源继续拒绝有跳板的入口；中间来源不能申请叶终端，转发目的地只能取被冻结的下一 hop。来源对象及内部密文没有 Debug／serde 投射。身份引用和 presence 标记只描述元数据，不授权签名、解密或执行。

实际 peer 确认、独立身份批准、精确首次 pin 推进和原点击期限由 [逐跳批准边界](ssh-route-hop-authorization.md) 及生产 driver 负责。来源复核是有界确认，不持有整个网络操作的 SQL 锁，也不承诺 SQL 与网络原子提交。

前置三项行为检查覆盖混合密码／私钥、显式叶身份、只读／重开、类型歧义、用户名与同修订变更、最终撤权、panic 和等待取消后的信用，两项隐私文档检查、Store Clippy 与无 SSH 信任 feature 构建通过。历史七份证据保存在 `target/probes/ssh-route-source-acceptance.json`；该批退出后清理 1.9 GiB。后续 Agent 三类来源检查通过精确 hop 顺序、类型／角色拒绝、引用删除与同修订替换。实际 Linux GUI 密码／私钥与 Agent 路线结果见生产文档及最新计划，历史前置证据不替代网络验收。
