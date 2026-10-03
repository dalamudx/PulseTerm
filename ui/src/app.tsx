import { useSyncExternalStore } from "react";
import {
  getSnapshot,
  setTerminalHost,
  start,
  subscribe,
} from "./features/probe/controller";
import { nativeAvailable } from "./ipc/probe";

const copy = {
  title: "IPC 数据通道验证",
  intro: "真实 WebView · 合成数据 · 非正式产品界面",
  run: "开始测量",
  running: "测量中…",
  browser:
    "请通过 mise run desktop 启动原生 WebView。浏览器预览不能测量 Tauri Channel。",
  empty: "尚无测量结果。",
  status: "状态",
  scenario: "样例",
  transport: "通道",
  rate: "吞吐 MB/s",
  latency: "送达 p95 ms*",
  peak: "在途峰值 KiB",
  heartbeat: "JS 定时器最大延迟 ms",
  verified: "数据校验",
  yes: "完整",
  note: "* 送达时间为时钟校准后的估算。时钟误差、ACK 往返和完整报告保存到 target/probes/channel.json。结果不代表终端解析、桌面渲染或三平台验收。",
};

export function App() {
  const state = useSyncExternalStore(subscribe, getSnapshot);
  return (
    <main>
      <header>
        <h1>{copy.title}</h1>
        <p>{copy.intro}</p>
      </header>
      <div className="toolbar">
        <button
          type="button"
          disabled={state.running || !nativeAvailable}
          onClick={() => {
            void start();
          }}
        >
          {state.running ? copy.running : copy.run}
        </button>
        <p aria-live="polite">
          {copy.status}：{state.status}
        </p>
      </div>
      {!nativeAvailable ? <p role="status">{copy.browser}</p> : null}
      {state.error ? (
        <p className="error" role="alert">
          {state.error}
        </p>
      ) : null}
      <section
        aria-labelledby="terminal-probe-label"
        className="terminal-probe"
      >
        <h2 id="terminal-probe-label">DOM 终端验证</h2>
        <p>
          固定 PTY 样例 · 初始 40 × 12 · 自动适配、输入与恢复验证，无用户 shell
        </p>
        <div className="terminal-host" ref={setTerminalHost} />
      </section>
      <div className="table-scroll">
        <table>
          <thead>
            <tr>
              <th>{copy.scenario}</th>
              <th>{copy.transport}</th>
              <th>{copy.rate}</th>
              <th>{copy.latency}</th>
              <th>{copy.peak}</th>
              <th>{copy.heartbeat}</th>
              <th>{copy.verified}</th>
            </tr>
          </thead>
          <tbody>
            {state.measurements.length === 0 ? (
              <tr>
                <td colSpan={7}>{copy.empty}</td>
              </tr>
            ) : (
              state.measurements.map((result) => (
                <tr key={`${result.transport}:${result.label}`}>
                  <td>{result.label}</td>
                  <td>{result.transport}</td>
                  <td>{(result.bytesPerSecond / 1e6).toFixed(2)}</td>
                  <td>{result.deliveryP95Ms.toFixed(2)}</td>
                  <td>{(result.backend.peakBytes / 1024).toFixed(0)}</td>
                  <td>{result.heartbeatMaxMs.toFixed(2)}</td>
                  <td>{copy.yes}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
      <p className="note">{copy.note}</p>
    </main>
  );
}
