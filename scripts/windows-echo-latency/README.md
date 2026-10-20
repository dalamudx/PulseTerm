# PulseTerm 回显延迟测量包（Windows，真实 GPU）

这个测量包回答一个问题：**在真实 GPU 上，WebView 加 xterm.js 的按键回显比原生终端慢多少？** 它不运行 PulseTerm，也不经过 PTY 和后端。

| 目标 | 说明 |
|---|---|
| `wt` | Windows Terminal（原生 DirectX 渲染）运行 `echo-console.exe`，经 ConPTY 回显 |
| `edge-dom` | Edge 应用窗口中的 xterm.js 6，DOM 渲染器，页面内本地回显 |
| `edge-webgl` | 同上，WebGL 渲染器 |

Edge 页面不经过 PTY，`wt` 经过 ConPTY，所以对比**偏向 Edge**。即使这样 Edge 仍明显更慢，结论更可靠。Edge 与 PulseTerm 在 Windows 上使用的 WebView2 同为 Chromium 内核，但窗口合成方式不完全相同。

## 测量原理

`echo-latency.exe` 用 `SendInput` 向前台窗口发送按键 `1`。三个目标收到后都会画一个纯红色块。程序用 DXGI 桌面复制读取窗口客户区，红色像素增加的那一帧即为回显帧，延迟取该帧的 QPC 呈现时间减去发键时间。每个目标先预热 5 次，再采样 100 次；整套流程跑 2 轮。

## 运行前

1. 显示器设为 **144Hz**，**关闭 HDR**。G-SYNC 保持你平时的设置即可，结果中会记录刷新率。
2. 输入法切换到**英文**（任务栏显示“英”或 ENG）。中文模式可能拦截按键，导致测量超时。
3. 关闭录屏、远程桌面、截图工具等占用桌面复制的软件。必须在本机直接操作，**不要通过远程桌面运行**。
4. 解压前右键 zip →“属性”→ 勾选“解除锁定”。也可以在解压后执行 `Get-ChildItem -Recurse | Unblock-File`。

## 运行

在解压目录打开 PowerShell：

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

脚本会依次打开 Windows Terminal 和两个 Edge 窗口，每个窗口自动测约 10–15 秒，总共约 2–3 分钟。**测量期间不要碰键盘和鼠标**；窗口失去焦点或被移动时，测量会报错退出，重新运行即可。

运行结束后会显示汇总表，并生成 `results-时间.zip`。请把这个 zip 发回。

## 结果内容

每个目标的 JSON 包含逐次延迟、p50、p95、刷新率、显卡、Windows 版本、窗口 DPI 和键盘布局。`summary.json` 合并两轮数据，列出各目标与 Windows Terminal 的 p50、p95 差值。测量只读取目标窗口区域的像素颜色计数，不保存截图。

Edge 使用独立的临时配置目录（`%TEMP%\pulseterm-echo-edge`），不读取你的浏览数据，结束后会自动删除。

## 第三方组件

`xterm/` 目录包含 xterm.js 6.0.0 与 @xterm/addon-webgl 0.19.0，采用 MIT 许可，许可文本随附于同一目录。
