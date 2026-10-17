# 终端行高与字距

终端设置沿用当前设置表单、原页面票据与逐字段保存接口。默认字号仍为 14px，行高为 100%，附加字距为 0px。

## 存储与来源

`terminal.line_height_percent` 是 100–300 的整数百分比，`terminal.letter_spacing` 是 0–8 的整数 CSS 像素值。负数、小数、非有限数字和超范围值拒绝。两项使用稀疏可选字段，缺失表示继承；明确保存 0px 字距仍是用户覆盖。恢复某一项只删除该项覆盖，保留字号、其他设置与文件内容。现有 scoped settings 的 CAS、完整来源验证和最终授权回调继续适用。

默认文件与生成的 `assets/schemas/settings.schema.json` 使用相同边界。旧配置缺失新字段时继承默认值，读取不创建用户配置。

## 页面和绘制

设置绘制投影 v2 显式包含两项数值及其来源；前端严格检查字段、版本和范围。v1 仍按原字段集解码，使用 100%／0px 默认；v1 混入新字段和不完整 v2 都拒绝。

绘制偏好保留独立的行高和字距，Profile 的已有字号／字体／光标覆盖不会丢弃这些偏好。xterm 接收 `lineHeight = line_height_percent / 100` 与 `letterSpacing`。设置保存不产生新会话；刷新由原有布局、resize 和捕获确认流程重建视图，输入仍需现有页面／generation／Commit 授权。

## 验证边界

三个存储用例覆盖独立来源、明确零值、非法值／schema一致性，以及标量保存与恢复保留其他内容。协议和表单检查覆盖 v1／v2 严格解码、逐字段提交、旧行事件撤销、绘制冻结与 Profile 合并。

`mise run probe-workspace-settings` 使用私有临时配置与固定 shell，在真实 Linux WebView 中分别改变行高和字距，对照内核 `stty size` 与原生 resize ACK，检查刷新同一 PTY／PID、独立进程读取不写文件、分别恢复继承、保留字号覆盖及两进程原生退出。此探针不代表系统 IME、其他平台、字体连字、CJK 后备字体或性能验收。

最新完成状态和证据以 `docs/development/plan.md` 为准。
