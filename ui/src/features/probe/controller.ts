import type { SessionEvidence } from "../../ipc/gen/commands";
import {
  automaticMode,
  finishReport,
  type Measurement,
  measure,
  nativeAvailable,
  verifyCancellation,
  verifyTypedEvents,
} from "../../ipc/probe";
import { verifySessionChannel } from "../../ipc/session-probe";

interface Snapshot {
  running: boolean;
  measurements: readonly Measurement[];
  status: string;
  error: string | null;
}
let snapshot: Snapshot = {
  running: false,
  measurements: [],
  status: "idle",
  error: null,
};
const listeners = new Set<() => void>();
function update(next: Partial<Snapshot>) {
  snapshot = { ...snapshot, ...next };
  for (const listener of listeners) listener();
}
export const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
export const getSnapshot = () => snapshot;
let initialized = false;

export async function start(): Promise<void> {
  if (snapshot.running || !nativeAvailable) return;
  update({ running: true, measurements: [], error: null, status: "starting" });
  const results: Measurement[] = [];
  let cancellation = false;
  let typedEvents = false;
  let session: SessionEvidence | null = null;
  let sessionRuns = 0;
  let failure: string | null = null;
  try {
    update({ status: "session-channel" });
    session = await verifySessionChannel();
    sessionRuns++;
    update({ status: "session-channel-recreated" });
    session = await verifySessionChannel();
    sessionRuns++;
    typedEvents = await verifyTypedEvents();
    for (const transport of ["channel", "websocket"] as const) {
      for (const scenario of [
        {
          label: "terminal",
          config: { chunkBytes: 65536, frames: 256, window: 8 },
          delay: 0,
        },
        {
          label: "desktop",
          config: { chunkBytes: 262144, frames: 256, window: 8 },
          delay: 0,
        },
        {
          label: "slow-consumer",
          config: { chunkBytes: 65536, frames: 32, window: 4 },
          delay: 75,
        },
      ]) {
        update({ status: `${transport}:${scenario.label}` });
        const result = await measure(
          transport,
          scenario.label,
          scenario.config,
          scenario.delay,
        );
        results.push(result);
        update({ measurements: [...results] });
      }
    }
    update({ status: "cancellation" });
    cancellation = await verifyCancellation();
    if (!cancellation) throw new Error("probe.cancellation_bound");
    // A successful new run also proves cancellation released the active slot.
    results.push(
      await measure("channel", "after-cancel", {
        chunkBytes: 8192,
        frames: 16,
        window: 4,
      }),
    );
    update({ measurements: [...results], status: "complete" });
  } catch (error) {
    failure = error instanceof Error ? error.message : String(error);
    update({ status: "failed", error: failure });
  } finally {
    update({ running: false });
    await finishReport({
      schemaVersion: 3,
      session,
      sessionRuns,
      typedEvents,
      completed:
        failure === null && cancellation && typedEvents && session !== null,
      cancellation,
      measurements: results,
      error: failure,
      environment: {
        userAgent: navigator.userAgent,
        hardwareConcurrency: navigator.hardwareConcurrency,
      },
      limitations: [
        "Session uses real native PTY and browser headless xterm callbacks, not DOM painting",
        "Delivery timing is clock-calibrated estimate; uncertainty is recorded",
        "Heartbeat measures JS timer responsiveness, not input-to-paint latency",
        "A single Linux/container run is not three-platform acceptance",
      ],
    }).catch((error: unknown) =>
      update({ error: String(error), status: "failed" }),
    );
  }
}

export async function initialize(): Promise<void> {
  if (
    initialized ||
    new URL(window.location.href).searchParams.has("session-peer")
  )
    return;
  initialized = true;
  if (nativeAvailable && (await automaticMode())) await start();
}
