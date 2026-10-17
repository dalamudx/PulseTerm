# 输入时回到底部

`[terminal].scroll_on_input` 是终端显示偏好，默认 `true`。设置页提供“输入时回到底部”复选框，支持保存明确的 `false` 和恢复继承。保存沿用原页 token、文件快照冲突检查和受监督设置 I/O。

前端通过设置投影 v7 接收布尔值与来源。v7 缺少该字段、类型不是布尔或多出未知字段时拒绝；v1～v6 保持原有封闭形状，缺省使用 `true`。恢复继承移除单个键，不写其他终端字段。

单视图和分屏从同一显示偏好构造 Surface；Profile 字号等显示覆盖不会丢失该偏好。Surface 在创建 xterm 时设置公开的 `scrollOnUserInput` 选项，省略仍按 `true`。它控制已获准输入时是否返回当前输出位置，不发送额外字节，也不修改输入审批、输出 ACK、解析器或后台视图调度。

保存影响以后新挂载的视图，例如新建、切换标签后重新打开或刷新页面；已挂载视图保留原偏好。它不提供在线调整，也不将新输出的自动跟随、历史保留和输入滚动合并为一个设置。输出回底通过[独立偏好](terminal-output-scroll.md)配置。

## 直接验证

存储检查覆盖默认开启、明确关闭、来源继承、恢复以及非法值；原生检查覆盖文件保存、重读、恢复和旧页撤销。前端检查覆盖 v7 封闭解码、旧版兼容、表单保存/恢复、Profile 显示合并、SDK 选项及保持初始输入禁用。
存储与原生共2项、受影响前端共30项直接行为通过；TypeScript、格式、settings-schema、Store／桌面库严格Clippy和产品构建通过，Store按需诊断Exact零问题。未运行完整回归，不将选项映射验证计为真实WebView或PTY/SSH滚动验收。

沿用现有 Rust、Vitest、TypeScript、前端构建和 settings-schema CI；不新增回归矩阵。实际 WebView、PTY/SSH 交互、跨平台与滚动性能另行验收。开发收据保存在 `target/probes/input-scroll-development/`。
