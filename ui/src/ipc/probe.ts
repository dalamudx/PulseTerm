import { Channel, isTauri } from "@tauri-apps/api/core";
import {
  FrameVerifier,
  type ProbeConfig,
  percentile,
} from "../features/probe/verification";
import {
  type BackendStats,
  commands,
  events,
  type ProbeMeasurement,
  type ProbeReport,
} from "./gen/commands";

export const nativeAvailable = isTauri();

// Serde can serialize nonfinite floats as null; validate at the boundary before
// narrowing the generated types for UI arithmetic. No duplicate field lists.
type NonNullFields<T> = { [K in keyof T]: NonNullable<T[K]> };
type CheckedBackendStats = NonNullFields<BackendStats>;
export type Measurement = NonNullFields<ProbeMeasurement> & {
  backend: CheckedBackendStats;
};

function stats(value: unknown): CheckedBackendStats {
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
  return record as unknown as CheckedBackendStats;
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
    const remote = await commands.probeClock();
    const end = performance.now();
    if (typeof remote !== "number" || !Number.isFinite(remote) || remote < 0)
      throw new Error("probe.clock");
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
  const receive = (
    data: unknown,
    ack: (sequence: number) => Promise<unknown>,
  ) => {
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
            void commands.probeCancel(id).catch(() => {});
          });
      }
    } catch (error) {
      failure ??= error;
      void commands.probeCancel(id).catch(() => {});
      socket?.close();
    }
  };
  const start = performance.now();
  try {
    let backend: CheckedBackendStats;
    if (transport === "channel") {
      const onData = new Channel<ArrayBuffer>();
      onData.onmessage = (data) =>
        receive(data, (sequence) => commands.probeAck(id, sequence));
      backend = stats(await commands.probeChannel(id, config, onData));
    } else {
      const ticket = await commands.probeSocket(id, config);
      backend = await new Promise<CheckedBackendStats>((resolve, reject) => {
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
    if (received === 1) cancel = commands.probeCancel(id);
  };
  try {
    await commands.probeChannel(id, config, channel);
    throw new Error("probe.cancel_ignored");
  } catch (error) {
    if (typeof error !== "string" || !error.includes("cancelled")) throw error;
  }
  await cancel;
  return received > 0 && received <= config.window;
}

export async function verifyTypedEvents(): Promise<boolean> {
  const nonce = runId();
  let resolve!: () => void;
  const received = new Promise<void>((done) => {
    resolve = done;
  });
  const unlisten = await events.probeEcho.listen((event) => {
    if (event.payload.nonce === nonce && event.payload.protocolVersion === 1)
      resolve();
  });
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await commands.probeEvent(nonce);
    await Promise.race([
      received,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("probe.event_timeout")),
          2000,
        );
      }),
    ]);
    return true;
  } finally {
    clearTimeout(timer);
    unlisten();
  }
}

export const automaticMode = () => commands.probeMode();
export const finishReport = (report: ProbeReport) =>
  commands.probeFinish(report);
