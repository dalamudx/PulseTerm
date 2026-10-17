# 终端 IME 的应用层按键边界

系统组合文字和最终编码仍由锁定的 xterm SDK 处理。PulseTerm 不从 `compositionupdate`／`compositionend` 读取文本，不补发组合结果，不把候选阶段字符当作命令或粘贴。

## 组合生命周期

`CompositionGuard` 在当前根上捕获 `compositionstart`／`compositionend`，并在失焦、页面退出和生命周期取消时清除本地状态及监听。每个工作区窗口、终端 Surface 和命令面板搜索框各自持有守卫；状态不跨窗共享。

事件的 `isComposing`、兼容 WebKit 的 `keyCode === 229` 以及 `key === "Process"` 都表示按键归输入法所有。即使浏览器在候选阶段漏标单次按键，活动生命周期仍保护应用动作。结束后不设置延时或吞掉下一次普通按键；结束边界上的 229 事件仍由兼容标记保护。

工作区在组合开始时退休待决快捷键前缀，组合期间跳过快捷键、方向焦点、弹窗 Escape 和强制刷新处理。守卫不调用 preventDefault／stopPropagation，让事件继续交给浏览器和 xterm。命令面板自身也跳过候选导航和确认键，Surface 的选区 Ctrl+C 分支让路给 SDK；普通 Ctrl+C 和已有复制逻辑保持原语义。

## 输入与权限

文字只通过已有 `onData` 订阅进入有界 InputQueue 和当前后端输入授权。组合事件本身不创建发送任务，也不提升输入权限。Surface／页面失效后，旧订阅不能发出迟到文本；后台恢复仍遵循既有捕获、ACK 和输入使能流程。

## 开发验证范围

七个直接相关 Vitest 文件共 79 项行为通过（分批运行，按文件去重），覆盖兼容标记、未标记候选键、失焦／页面退出、根隔离、监听清理、组合确认不执行命令、CJK／组合字符单次原样提交及旧视图拒绝。TypeScript、改动文件 Biome 和产品构建通过，保留既有 lint／打包警告。TypeScript Astrolabe 为 Unknown，不计通过；没有安装新工具、升级 SDK、运行全量回归或新增重型 CI 任务，现有 Vitest／类型／构建任务自动覆盖新增模块。

这些是应用层和模拟 SDK 输入验证。系统中日韩输入法、真实候选框跟随、缩放／跨屏 DPI、Linux WebKit 和其他平台的原生输入事件顺序仍须真实 WebView 验证，不计 TERM-04 整体验收完成。轻量开发证据保留在 `target/probes/ime-input-development/`。
