import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import {
  FrameVerifier,
  type ProbeConfig,
  percentile,
} from "../features/probe/verification";

export const nativeAvailable = isTauri();

interface BackendStats {
  frames: number;
  bytes: number;
  elapsedMs: number;
  ackRttP95Ms: number;
  peakFrames: number;
  peakBytes: number;
}
export interface Measurement {
  label: string;
  transport: "channel" | "websocket";
  config: ProbeConfig;
  bytesPerSecond: number;
  deliveryP95Ms: number;
  clockUncertaintyMs: number;
  heartbeatP95Ms: number;
  heartbeatMaxMs: number;
  backend: BackendStats;
  verified: boolean;
}

function stats(value: unknown): BackendStats {
  if (!value || typeof value !== "object") throw new Error("probe.stats");
  const record = value as Record<string, unknown>;
  for (const key of [
    "frames",
    "bytes",
    "elapsedMs",
    "ackRttP95Ms",
    "peakFrames",
    "peakBytes",
  ]) {
    if (
      typeof record[key] !== "number" ||
      !Number.isFinite(record[key]) ||
      record[key] < 0
    )
      throw new Error("probe.stats");
  }
  return record as unknown as BackendStats;
}

function runId(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (v) =>
    v.toString(16).padStart(2, "0"),
  ).join("");
}
const wait = (milliseconds: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, milliseconds));

async function synchronizeClock(): Promise<{
  offset: number;
  uncertainty: number;
}> {
  let best = { offset: 0, uncertainty: Number.POSITIVE_INFINITY };
  for (let sample = 0; sample < 7; sample++) {
    const start = performance.now();
    const remote = await invoke<number>("probe_clock");
    const end = performance.now();
    if ((end - start) / 2 < best.uncertainty)
      best = {
        offset: (start + end) / 2 - remote,
        uncertainty: (end - start) / 2,
      };
  }
  return best;
}

export async function measure(
  transport: Measurement["transport"],
  label: string,
  config: ProbeConfig,
  ackDelayMs = 0,
): Promise<Measurement> {
  const clock = await synchronizeClock();
  const id = runId();
  const verifier = new FrameVerifier(config, clock.offset);
  const delays: number[] = [];
  let previous = performance.now();
  const heartbeat = setInterval(() => {
    const now = performance.now();
    delays.push(Math.max(0, now - previous - 16));
    previous = now;
  }, 16);
  const batch = Math.max(1, Math.floor(config.window / 2));
  let ackChain = Promise.resolve();
  let failure: unknown;
  let socket: WebSocket | undefined;
  const receive = (data: unknown, ack: (sequence: number) => Promise<void>) => {
    try {
      if (!(data instanceof ArrayBuffer)) throw new Error("probe.not_binary");
      const sequence = verifier.consume(
        new Uint8Array(data),
        performance.now(),
      );
      if (verifier.frames % batch === 0 || verifier.frames === config.frames) {
        ackChain = ackChain
          .then(async () => {
            if (ackDelayMs > 0) await wait(ackDelayMs);
            await ack(sequence);
          })
          .catch((error: unknown) => {
            failure ??= error;
            void invoke("probe_cancel", { id }).catch(() => {});
          });
      }
    } catch (error) {
      failure ??= error;
      void invoke("probe_cancel", { id }).catch(() => {});
      socket?.close();
    }
  };
  const start = performance.now();
  try {
    let backend: BackendStats;
    if (transport === "channel") {
      const onData = new Channel<ArrayBuffer>();
      onData.onmessage = (data) =>
        receive(data, (sequence) =>
          invoke<void>("probe_ack", { id, sequence }),
        );
      backend = stats(
        await invoke<unknown>("probe_channel", { id, config, onData }),
      );
    } else {
      const ticket = await invoke<{ url: string; token: string }>(
        "probe_socket",
        { id, config },
      );
      backend = await new Promise<BackendStats>((resolve, reject) => {
        const connection = new WebSocket(ticket.url, [
          "pulseterm-probe",
          ticket.token,
        ]);
        socket = connection;
        connection.binaryType = "arraybuffer";
        connection.onmessage = (event: MessageEvent<unknown>) => {
          if (typeof event.data === "string") {
            try {
              resolve(stats(JSON.parse(event.data) as unknown));
            } catch (error) {
              reject(error);
            }
          } else
            receive(event.data, async (sequence) => {
              const bytes = new Uint8Array(4);
              new DataView(bytes.buffer).setUint32(0, sequence, true);
              connection.send(bytes);
            });
        };
        connection.onerror = () => reject(new Error("probe.socket_error"));
        connection.onclose = () => reject(new Error("probe.socket_closed"));
      });
    }
    await ackChain;
    if (failure) throw failure;
    verifier.finish();
    if (
      backend.bytes !== config.frames * config.chunkBytes ||
      backend.frames !== config.frames ||
      backend.peakFrames > config.window ||
      backend.peakBytes > config.window * config.chunkBytes
    )
      throw new Error("probe.backend_mismatch");
    return {
      label,
      transport,
      config,
      bytesPerSecond: backend.bytes / ((performance.now() - start) / 1000),
      deliveryP95Ms: percentile(verifier.deliveryMs, 95),
      clockUncertaintyMs: clock.uncertainty,
      heartbeatP95Ms: percentile(delays, 95),
      heartbeatMaxMs: Math.max(0, ...delays),
      backend,
      verified: true,
    };
  } finally {
    clearInterval(heartbeat);
    socket?.close();
  }
}

export async function verifyCancellation(): Promise<boolean> {
  const id = runId();
  const config = { chunkBytes: 65536, frames: 256, window: 4 };
  const channel = new Channel<ArrayBuffer>();
  let received = 0;
  let cancel: Promise<unknown> | undefined;
  channel.onmessage = () => {
    received++;
    if (received === 1) cancel = invoke("probe_cancel", { id });
  };
  try {
    await invoke("probe_channel", { id, config, onData: channel });
    throw new Error("probe.cancel_ignored");
  } catch (error) {
    if (typeof error !== "string" || !error.includes("cancelled")) throw error;
  }
  await cancel;
  return received > 0 && received <= config.window;
}

export const automaticMode = () => invoke<boolean>("probe_mode");
export const finishReport = (report: unknown) =>
  invoke<void>("probe_finish", { report });
