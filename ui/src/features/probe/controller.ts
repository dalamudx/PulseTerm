import { verifyEncodingWindow } from "../../ipc/encoding-window-probe";
import type {
  DomEvidence,
  EncodingEvidence,
  SessionEvidence,
} from "../../ipc/gen/commands";
import { commands } from "../../ipc/gen/commands";
import { installPeerBootstrap } from "../../ipc/peer-bootstrap";
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
import { PaneVisibility } from "../terminal/pane-visibility";
import { type PooledViewController, ViewHost } from "../terminal/view-pool";
import type { ViewSlotOwner } from "../terminal/view-slot-ref";

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
let terminalHost: HTMLElement | undefined;
let surfaceLife: AbortController | undefined;
function setTerminalHost(host: HTMLElement | null): void {
  if (terminalHost === host) return;
  surfaceLife?.abort();
  terminalHost = host ?? undefined;
  surfaceLife = host ? new AbortController() : undefined;
}

let probePool: ViewHost | undefined;
let probeFactories = 0;
const paneVisibility = new PaneVisibility();
let paneOwner: ViewSlotOwner | undefined;
export const registerProbeSlot = (owner: ViewSlotOwner) => {
  paneOwner = owner;
};
export function createProbeViewHost(
  doc: Document,
  signal: AbortSignal,
): ViewHost {
  const parking = doc.createElement("div");
  doc.body.appendChild(parking);
  const pool = new ViewHost(parking, signal, () =>
    update({ error: "probe.pool_cleanup_failed", status: "failed" }),
  );
  const remove = () => parking.remove();
  signal.addEventListener("abort", remove, { once: true });
  if (signal.aborted) remove();
  probePool = pool;
  return pool;
}
/** Finite adapter relays pool visibility to the active attachment controller.
 * Without a bound control plane, parking cancels the sample fail closed. */
export function probeViewFactory(
  element: HTMLElement,
  signal: AbortSignal,
): PooledViewController {
  probeFactories++;
  element.className = "terminal-host";
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    signal.removeEventListener("abort", dispose);
    if (terminalHost === element) setTerminalHost(null);
  };
  signal.addEventListener("abort", dispose, { once: true });
  return {
    setVisible: (visible) => {
      if (disposed || signal.aborted) throw new Error("probe.pool_closed");
      const handled = paneVisibility.observe(visible);
      if (visible) setTerminalHost(element);
      else if (!handled && terminalHost === element) setTerminalHost(null);
    },
    dispose,
  };
}

export async function start(): Promise<void> {
  if (snapshot.running || !nativeAvailable) return;
  update({ running: true, measurements: [], error: null, status: "starting" });
  const results: Measurement[] = [];
  let cancellation = false;
  let typedEvents = false;
  let session: SessionEvidence | null = null;
  let sessionRuns = 0;
  let domSession: SessionEvidence | null = null;
  let domEvidence: DomEvidence | null = null;
  let domRuns = 0;
  let encoding: EncodingEvidence | null = null;
  let encodingRuns = 0;
  let failure: string | null = null;
  try {
    update({ status: "session-channel" });
    session = await verifySessionChannel();
    sessionRuns++;
    update({ status: "session-channel-recreated" });
    session = await verifySessionChannel();
    sessionRuns++;
    const host = terminalHost,
      life = surfaceLife;
    if (!host || !life || !host.isConnected)
      throw new Error("probe.dom_host_missing");
    for (let i = 0; i < 2; i++) {
      update({ status: i === 0 ? "session-dom" : "session-dom-recreated" });
      domSession = await verifySessionChannel({
        host,
        signal: life.signal,
        pane: {
          visibility: paneVisibility,
          park: () => {
            if (!paneOwner?.park()) throw new Error("probe.pool_park_failed");
          },
          resume: () => {
            if (!paneOwner) throw new Error("probe.slot_missing");
            paneOwner.resume();
          },
        },
        complete: (e) => {
          domEvidence = e;
        },
      });
      domRuns++;
    }
    if (probeFactories !== 1 || probePool?.size !== 1)
      throw new Error("probe.view_pool_factory_count");
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
    if (await commands.probeEncodingMode()) {
      const host = terminalHost,
        life = surfaceLife;
      if (!host || !life || !host.isConnected)
        throw new Error("encoding.dom_host_missing");
      for (let i = 0; i < 2; i++) {
        update({ status: `encoding-dom-${i + 1}` });
        encoding = await verifyEncodingWindow(host, life.signal);
        encodingRuns++;
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
      schemaVersion: 4,
      encoding,
      encodingRuns,
      domSession,
      domEvidence,
      domRuns,
      session,
      sessionRuns,
      typedEvents,
      completed:
        failure === null &&
        cancellation &&
        typedEvents &&
        session !== null &&
        domSession !== null &&
        domEvidence !== null &&
        domRuns === 2,
      cancellation,
      measurements: results,
      error: failure,
      environment: {
        userAgent: navigator.userAgent,
        hardwareConcurrency: navigator.hardwareConcurrency,
      },
      limitations: [
        "Fixed PTY samples use headless and DOM xterm; frame fences and DOM rows are not compositor pixel latency",
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
  if (initialized) return;
  initialized = true;
  if (new URL(window.location.href).searchParams.has("session-peer")) {
    installPeerBootstrap(
      window,
      async (nonce: string, pageNonce: string, previousPageNonce?: string) => {
        const { verifyPeerView } = await import("../../ipc/peer-view-probe");
        await verifyPeerView(nonce, pageNonce, previousPageNonce);
      },
    );
    return;
  }
  if (nativeAvailable && (await automaticMode())) await start();
}
