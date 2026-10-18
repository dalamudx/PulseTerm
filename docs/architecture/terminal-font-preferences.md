# 终端字体预设与后备字体

默认仍为系统等宽字体、14px、行高100%、字距0px。设置表单提供系统等宽、内置 JetBrains Mono 与自定义名称。选择预设只改变待编辑值；原有“保存”按钮才发起逐字段原页 CAS。查看许可证没有写入或启动副作用。

## 本地字体与来源

JetBrains Mono 2.304 的原始 Regular WOFF2 和 SIL OFL 1.1 文本放在 `ui/src/assets/fonts/jetbrains-mono/`，`source.json` 记录固定上游 URL 与 SHA-256。字体未修改、未做子集；没有下载时加载字体或访问外部服务。页面内“许可”显示完整文本。这里只交付 Regular 字体资源，不声称打包了独立粗体、斜体或 CJK 字体。

## 保存与绘制

`terminal.font_fallback` 是与主字体相同的有界单个 family 名称，缺失时继承系统等宽。它有独立来源、保存和恢复操作，不能提交 CSS 列表、字体 URL 或任意样式。前端构造受限的主字体、后备字体、通用 monospace 序列。

设置绘制投影 v3 包含后备字体和连字偏好的来源。v1/v2 继续按各自精确字段集解码，补明确默认值；旧版本夹带新字段、v3字段缺失或类型不符都拒绝。字体偏好不授予终端输入、resize 或会话创建权限；现有原页、generation、布局与 Commit 边界继续适用。

内置字体被主字体或后备字体选中时，Surface 在加载 SDK／测量前等待当前文档 FontFaceSet 的本地 face，检查实际 face 可用，最多等待1500ms。页面撤销和超时拒绝继续创建，迟到结果不能发布旧页 Surface。此就绪检查是资源准备，不是性能验收。

## 按需 WebGL 运算符连字（Linux 限定验收通过）

默认仍使用 DOM 渲染器且连字关闭。用户明确保存开启时，Surface 按需加载固定版本官方 `@xterm/addon-webgl`，绑定同一个 xterm 实例与共享 Unicode scalar provider。合并仅覆盖有限的常见 ASCII 运算符，每次回调最多64段，返回 UTF-16 索引给 SDK，由 SDK 保持实际终端列宽与样式边界。主／后备字体仍由当前文档本地 FontFaceSet 准备，不引入字体网络读取。

开启时 GPU 不可用、画布超过4194304像素、字形图集超过64MiB或绘制失败都明确拒绝，不静默回退并声称连字完成。每次后端帧仍先解析及验证原可见终端 digest，再等待当前全行 public render、有效图集和画布、无 GL 错误及1000ms内完成的 GPU fence 后才发布绘制回执／ACK。该收据证明 GPU 绘制命令完成，不是桌面合成器或屏幕呈现确认；旧 DOM 路径保持原证据检查。

原生 `webglcontextlost` 立即失效当前 generation、取消等待和原 Pane 连接／布局观察，输入必须经原有新 Bootstrap／Commit 才恢复。迟到加载和旧上下文事件不能撤销替换后的 Surface；renderer、joiner、事件和 fence 由 Surface 生命周期回收。默认 DOM 的静态样式仍禁用自动 liga/calt。

Linux真实WebView已验证webgl2，当前文档同字体整串／逐字Canvas有142个差异像素，完整运算符截图明确合并。根因是旧xterm静态liga/calt禁用被图集继承；仅启用的Surface在图集暖身前添加局部OpenType覆盖，处置时移除。默认DOM规则不变。精确ASCII输入、后端中文／emoji／组合字符混合列宽、同PTY刷新、第三进程零写重开及所有原生退出通过。1182项全前端、修复后41项相关检查；上下文丢失／旧事件拒绝和GPU等待期间不发ACK由行为反例验证，不据此声称真实GPU故障或性能通过。13项不可变SHA证据见 `target/probes/terminal-ligatures-acceptance.json`。验证用公开画布观测仅在独立 `validation-probe-webgl` 构建中注入；默认生产构建不包含。清理待复用二进制的CLI验收链完成。

## 验证边界

`mise run probe-workspace-font` 是 Linux 真 WebView 验收：预设选择和许可证零写、明确保存内置字体、缺失主字体使用内置后备、固定 ASCII／中文／emoji 输出，以及同 PTY／PID 和退出。它复用设置网格、重开和恢复链，不能替代跨平台、IME、全面 CJK 覆盖、系统字体枚举或字体安装验证。Linux实窗及第三进程以67×20网格只读重开最终字体配置已通过，三进程及PTY最终退出。1173项全前端、84项设置Rust（1项忽略不计）、格式与CLI／SSH严格原生检查通过；Rust设置诊断Exact无诊断，TypeScript LSP Unknown不计通过。23项不可变证据见 `target/probes/terminal-font-acceptance.json`；clean-dev回收10.6GiB，临时目录已清除。


## Linux 受控 1 倍／2 倍缩放与字距修复

`mise run probe-workspace-dpi` 依次以 `GDK_SCALE=1` 和 `2`、`GDK_DPI_SCALE=1` 复用正式字体矩阵。生产窗口的逻辑尺寸保持960×640；X11实测分别为960×640和1920×1280。字体v2收据记录四组由内核stty与后端resize交叉核对的网格：14px为92×29、18px为67×22；150%行高为67×15／67×14，加2px字距后为56×15／56×14。各组列数严格相同，默认及18px行数严格相同；本矩阵的150%行高允许至多一行差异，因为锁定xterm在设备像素层对字符高度向上取整、对行高乘积向下取整，换算回CSS后可能改变边界行数。这个限定不扩展为任意字体或缩放比例的保证。

锁定的DOM及WebGL renderer均把 `letterSpacing` 加在设备像素层。设置继续使用CSS像素，Surface按所属document的 `devicePixelRatio` 换算后传入xterm；非零字距监听所属窗口的resolution媒体查询与resize，像素比变化后重建查询，视图撤销／释放时清理监听。默认0字距不额外注册监听，输入授权、ACK屏障及14px产品默认值保持原合同。修复前2倍矩阵的2px字距错误地得到61列，修复后恢复56列；监听、过期事件和Surface实际接线有回归，真实多屏移动仍待验。

字体探针等待前端“已保存”状态后再关闭／刷新，并把AT-SPI逻辑屏幕边界换算为X11设备坐标，真实点击后要求下拉菜单展开。两组预设明确保存、许可查看、后备字体、刷新／独立重开、恢复继承及六条PTY退出均通过。最终收据、截图、日志与修复前失败保留在 `target/probes/workspace-dpi/`。正式mise入口和Linux CI复用组合任务，失败不发布通过收据，子任务超时撤销自有进程组；仅复制本轮生成的图像／日志。

验收限于Linux Xvfb软件渲染及固定GTK缩放；物理多屏移动、真实DPI／系统IME、硬件GPU、连字GPU路径和其他平台继续开放。
