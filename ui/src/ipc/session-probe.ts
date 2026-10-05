// Developer-only finite PTY validation with headless and DOM xterm surfaces.
import { Channel } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { IBufferCell, Terminal as TerminalType } from "@xterm/headless";
import type { DomRenderer } from "../features/probe/dom-renderer";
import { IdleComparison } from "../features/terminal/idle-comparison";
import { InputQueue } from "../features/terminal/input-queue";
import {
  NativeVisibilityBuffer,
  NativeVisibilityFilter,
} from "../features/terminal/native-visibility";
import { suppressQueryReplies } from "../features/terminal/query-policy";
import { screenDigest } from "../features/terminal/screen-digest";
import { SurfaceVisibility } from "../features/terminal/surface-visibility";
import { SharedUnicodeProvider } from "../features/terminal/unicode-provider";
import { Utf8Stream } from "../features/terminal/utf8-stream";
import { armViewDeadline } from "../features/terminal/view-deadline";
import {
  GenerationViewHost,
  type ViewScope,
} from "../features/terminal/view-host";
import type { AttachBoundary } from "./attachment";
import { decodeFrame } from "./frames";
import {
  commands,
  events,
  type NativeVisibilitySample,
  type SessionEvidence,
} from "./gen/commands";
import {
  createProbeConsumer,
  type ProbeAttachmentVersion,
  type ProbeConsumer,
  probeAttachmentEnvelope,
} from "./probe-attachment-codec";
import { forCurrentSurface, SurfaceGeneration } from "./surface-generation";
import { TransferViewProbe } from "./transfer-view-probe";

type ProbeTerminal = Pick<
  TerminalType,
  | "buffer"
  | "cols"
  | "rows"
  | "write"
  | "unicode"
  | "parser"
  | "onData"
  | "dispose"
>;
function color(cell: IBufferCell, foreground: boolean): string {
  if (foreground ? cell.isFgDefault() : cell.isBgDefault()) return "default";
  const value = foreground ? cell.getFgColor() : cell.getBgColor();
  return (foreground ? cell.isFgRGB() : cell.isBgRGB())
    ? `rgb:${(value >>> 16) & 255},${(value >>> 8) & 255},${value & 255}`
    : `idx:${value}`;
}
export function digest(terminal: ProbeTerminal): string {
  const buffer = terminal.buffer.active;
  return screenDigest({
    cells: Array.from({ length: terminal.rows }, (_, row) =>
      Array.from({ length: terminal.cols }, (_, col) => {
        const cell = buffer.getLine(buffer.baseY + row)?.getCell(col);
        if (!cell) throw new Error("session.cell_missing");
        return {
          text: cell.getWidth() === 0 ? "" : cell.getChars() || " ",
          width: cell.getWidth(),
          foreground: color(cell, true),
          background: color(cell, false),
          bold: cell.isBold() !== 0,
          italic: cell.isItalic() !== 0,
          underline: cell.isUnderline() !== 0,
          inverse: cell.isInverse() !== 0,
        };
      }),
    ),
    cursor: [Math.min(buffer.cursorX, terminal.cols - 1), buffer.cursorY],
    alternate: buffer.type === "alternate",
  });
}
let previous: { nonce: string; generation: string } | undefined;
async function requireRejected(action: Promise<unknown>): Promise<void> {
  let denied = false;
  try {
    await action;
  } catch {
    denied = true;
  }
  if (!denied) throw new Error("session.stale_admitted");
}
export async function verifySessionChannel(dom?: {
  host: HTMLElement;
  signal: AbortSignal;
  complete: (e: import("./gen/commands").DomEvidence) => void;
  pane: {
    visibility: import("../features/terminal/pane-visibility").PaneVisibility;
    park: () => void;
    resume: () => void;
  };
}): Promise<SessionEvidence> {
  const selected = await commands.probeSessionProtocol();
  if (selected !== 1 && selected !== 2)
    throw new Error("session.unsupported_attachment_version");
  const protocol: ProbeAttachmentVersion = selected;
  const nonce = crypto.randomUUID();
  let terminal: ProbeTerminal | undefined;
  let renderer: DomRenderer | undefined;
  const lifetime = new AbortController();
  const cancel = () => {
    lifetime.abort();
    consumer?.cancel();
    if (generationSeen !== undefined)
      void commands
        .probeSessionDetach(nonce, generationSeen.toString())
        .catch(() => {});
  };
  dom?.signal.addEventListener("abort", cancel, { once: true });
  if (dom?.signal.aborted) lifetime.abort();
  let viewScope: ViewScope | undefined;
  let priorRenderer: DomRenderer | undefined;
  let viewRebuilds = 0;
  let decoder = new Utf8Stream();
  let replies = 0;
  let inputQueue: InputQueue | undefined;
  let inputStarted = false,
    inputArmed = false,
    inputChecked = false;
  let inputText = "";
  const inputJobs: Promise<void>[] = [];
  let final: { revision: bigint; parsedBytes: bigint } | undefined;
  let consumer: ProbeConsumer | undefined;
  let failed: unknown;
  const views = new GenerationViewHost(lifetime.signal, (error) => {
    failed ??= error;
    cancel();
  });
  let latest: Promise<unknown> = Promise.resolve();
  let deadline: ReturnType<typeof setTimeout> | undefined;
  const write = async (text: string, signal: AbortSignal) => {
    if (signal.aborted) throw new Error("session.cancelled");
    if (!terminal) throw new Error("session.no_renderer");
    if (text.length === 0) return;
    await new Promise<void>((resolve, reject) => {
      const abort = () => reject(new Error("session.cancelled"));
      signal.addEventListener("abort", abort, { once: true });
      terminal?.write(text, () => {
        signal.removeEventListener("abort", abort);
        if (signal.aborted) reject(new Error("session.cancelled"));
        else resolve();
      });
    });
  };
  const bytes = async (data: Uint8Array, signal: AbortSignal) => {
    for (let at = 0; at < data.length; at += 4096)
      await write(decoder.feed(data.subarray(at, at + 4096)), signal);
    await renderer?.settle(AbortSignal.any([signal, lifetime.signal]));
  };
  const screenContains = (text: string) => {
    const b = terminal?.buffer.active;
    return b
      ? Array.from(
          { length: terminal?.rows ?? 0 },
          (_, i) => b.getLine(b.baseY + i)?.translateToString(true) ?? "",
        ).some((line) => line.includes(text))
      : false;
  };
  const transferChannel = new Channel<ArrayBuffer>();
  const transferView = new TransferViewProbe({
    version: protocol,
    host: dom?.host,
    signal: lifetime.signal,
    ack: async (generation, sequence) => {
      await commands.probeTransferAck(nonce, generation, sequence);
    },
    confirm: async (generation, boundary, value) => {
      const rendered = {
        columns: boundary.columns,
        rows: boundary.rows,
        revision: boundary.revision.toString(),
        parsedBytes: boundary.parsedBytes.toString(),
        digest: value,
      };
      await requireRejected(
        commands.probeTransferAck(crypto.randomUUID(), generation, 0),
      );
      await requireRejected(
        commands.probeTransferConfirm(
          nonce,
          (BigInt(generation) + 1n).toString(),
          rendered,
        ),
      );
      await requireRejected(commands.probeSessionAck(nonce, generation, 0));
      await commands.probeTransferConfirm(nonce, generation, rendered);
      await requireRejected(
        commands.probeTransferConfirm(nonce, generation, rendered),
      );
    },
    digest,
    failed: (error) => {
      failed ??= error;
      cancel();
    },
  });
  transferChannel.onmessage = (data) => transferView.receive(data);

  const channel = new Channel<ArrayBuffer>();
  let delayed = false;
  let idleChecked = false;
  let markerChecked = false;
  let repairChecked = false,
    damaged = false,
    repairAttempted = false,
    repairAdmitted = false;
  let historyBeforeRepair: string[] = [];
  const historyText = () => {
    const b = terminal?.buffer.normal;
    return b
      ? Array.from(
          { length: b.baseY },
          (_, i) => b.getLine(i)?.translateToString(false) ?? "",
        )
      : [];
  };
  const paused = () => {
    const b = terminal?.buffer.active;
    return b
      ? Array.from(
          { length: terminal?.rows ?? 0 },
          (_, i) => b.getLine(b.baseY + i)?.translateToString(true) ?? "",
        ).some((s) => s.includes("MARKER_READY"))
      : false;
  };
  let markerSeen = false;
  let outputEpoch = 0;
  let geometry: Pick<AttachBoundary, "columns" | "rows"> | undefined;
  let idle: IdleComparison | undefined;
  let generationSeen: bigint | undefined;
  let resizeCount = 0,
    resizeChecked = false;
  let phase = "bootstrap";
  let poolStarted = false,
    poolChecked = false,
    poolResumePending = false;
  let poolBoundary = 0n;
  let stopPane: (() => void) | undefined;
  let pauseStarted = false,
    visibilityChecked = false,
    surfaceHidden = false,
    resumePending = false;
  const generations = new SurfaceGeneration();
  let rebinds = 0,
    wireCount = 0;
  let hiddenBoundary = 0n;
  let visibility: SurfaceVisibility | undefined;
  let nativeCycle: Promise<string> | undefined;
  let nativeOnlyPaused = false,
    hiddenWireCount = 0;
  let stopNative: (() => void) | undefined;
  let nativeHidden = false,
    nativeRestored = false;
  let nativeTimer: ReturnType<typeof setTimeout> | undefined;
  let expectedSize: { columns: number; rows: number } | undefined;
  let oldGeneration: bigint | undefined;
  channel.onmessage = (data) => {
    try {
      if (!(data instanceof ArrayBuffer)) throw new Error("session.not_binary");
      const wire = new Uint8Array(data);
      const frame = decodeFrame(wire);
      wireCount++;
      if (surfaceHidden) return;
      if (frame.kind !== 8) throw new Error("session.frame_kind");
      const header = probeAttachmentEnvelope(frame.payload, protocol);
      const route = generations.accept(
        header.generation,
        header.part === 1,
        frame.sequence,
      );
      if (route === "stale") return;
      if (frame.kind === 8 && frame.payload[1] === 5) {
        outputEpoch++;
        idle?.activity();
      }
      if (
        consumer &&
        frame.kind === 8 &&
        frame.sequence === 0 &&
        frame.payload[1] === 1
      ) {
        const next = new DataView(
          frame.payload.buffer,
          frame.payload.byteOffset,
          frame.payload.byteLength,
        ).getBigUint64(4, true);
        if (
          !dom ||
          !expectedSize ||
          generationSeen === undefined ||
          next <= generationSeen ||
          rebinds >= 5
        )
          throw new Error("session.unexpected_generation");
        oldGeneration = generationSeen;
        rebinds++;
        idle?.dispose();
        priorRenderer = renderer;
        stopPane?.();
        views.retire(generationSeen);
        consumer = undefined;
        renderer = undefined;
        terminal = undefined;

        decoder = new Utf8Stream();
        geometry = undefined;
        outputEpoch++;
      }
      const admissionEpoch = outputEpoch;
      if (!consumer) {
        if (
          frame.kind !== 8 ||
          frame.sequence !== 0 ||
          frame.payload.length < 20
        )
          throw new Error("session.no_begin");
        const generation = new DataView(
          frame.payload.buffer,
          frame.payload.byteOffset,
          frame.payload.byteLength,
        ).getBigUint64(4, true);
        generationSeen = generation;
        const view = views.begin(generation);
        viewScope = view;
        consumer = createProbeConsumer(
          protocol,
          generation,
          {
            reset: async (boundary, signal) => {
              phase = "reset";
              if (terminal) throw new Error("session.repeated_reset");
              if (dom) {
                const created = await view.create(async (viewSignal) => {
                  const { createDomRenderer } = await import(
                    "../features/probe/dom-renderer"
                  );
                  return createDomRenderer(
                    dom.host,
                    boundary,
                    AbortSignal.any([signal, lifetime.signal, viewSignal]),
                  );
                });
                if (!view.isCurrent()) throw new Error("session.view_retired");
                renderer = created;
                terminal = renderer.terminal;
                if (priorRenderer) {
                  const root = dom.host.querySelector(".xterm");
                  priorRenderer.dispose();
                  if (
                    !root ||
                    dom.host.querySelector(".xterm") !== root ||
                    !view.isCurrent()
                  )
                    throw new Error("session.old_disposal_touched_new_view");
                  viewRebuilds++;
                  priorRenderer = undefined;
                }
              } else {
                const created = await view.create(async () => {
                  const { default: headless } = await import("@xterm/headless");
                  return new headless.Terminal({
                    cols: boundary.columns,
                    rows: boundary.rows,
                    scrollback: 128,
                    allowProposedApi: true,
                  });
                });
                if (!view.isCurrent()) throw new Error("session.view_retired");
                terminal = created;
              }
              const provider = new SharedUnicodeProvider();
              terminal.unicode.register(provider);
              terminal.unicode.activeVersion = provider.version;
              view.own(suppressQueryReplies(terminal));
              view.own(
                terminal.onData((text) => {
                  if (inputArmed && inputQueue) {
                    inputText += text;
                    const job = inputQueue.enqueue(text);
                    inputJobs.push(job);
                    void job.catch((error) => {
                      if (!view.isCurrent()) return;
                      failed ??= error;
                      cancel();
                    });
                  } else replies++;
                }),
              );
              decoder = new Utf8Stream();
              geometry = { columns: boundary.columns, rows: boundary.rows };
            },
            write: bytes,
            repair: async (_boundary, data, signal) => {
              if (!damaged) throw new Error("session.unsolicited_repair");
              decoder = new Utf8Stream();
              await bytes(data, signal);
              if (
                JSON.stringify(historyBeforeRepair) !==
                JSON.stringify(historyText())
              )
                throw new Error("session.repair_history");
            },
            marker: async (_revision, _parsed, signal) => {
              if (signal.aborted) throw new Error("session.cancelled");
              if (!damaged && paused()) {
                historyBeforeRepair = historyText();
                damaged = true;
                await write("\x18\x1b[1;1H!", signal);
                await renderer?.settle(
                  AbortSignal.any([signal, lifetime.signal]),
                );
              }
            },
            checkpoint: async (_revision, _parsed, signal) => {
              await write("\x1b[?2026l", signal);
              await renderer?.settle(
                AbortSignal.any([signal, lifetime.signal]),
              );
            },
            finish: async (revision, parsedBytes, signal) => {
              await write(decoder.finish(), signal);
              await renderer?.settle(
                AbortSignal.any([signal, lifetime.signal]),
              );
              final = { revision, parsedBytes };
            },
          },
          async (gen, sequence) => {
            if (lifetime.signal.aborted) throw new Error("session.cancelled");
            if (sequence === 0) {
              await requireRejected(
                commands.probeSessionAck(
                  nonce,
                  (gen + 1n).toString(),
                  sequence,
                ),
              );
              if (previous) {
                await requireRejected(
                  commands.probeSessionAck(
                    previous.nonce,
                    previous.generation,
                    sequence,
                  ),
                );
                await requireRejected(
                  commands.probeSessionAck(
                    nonce,
                    previous.generation,
                    sequence,
                  ),
                );
              }
            }

            // Deliberately withhold a live-frame ACK once; the only credit slot must stay bound.
            if (sequence > 2 && !delayed) {
              delayed = true;
              await new Promise<void>((resolve) => setTimeout(resolve, 250));
            }
            await commands.probeSessionAck(nonce, gen.toString(), sequence);
          },
        );
        const boundConsumer = consumer;
        view.own(() => boundConsumer.cancel());
      }
      const activeConsumer = consumer;
      const activeView = viewScope;
      if (!activeView) throw new Error("session.view_scope_missing");
      latest = forCurrentSurface(
        activeConsumer.enqueue(wire),
        () => activeConsumer === consumer && activeView.isCurrent(),
        async (receipt) => {
          if (
            frame.kind === 8 &&
            frame.payload.length >= 12 &&
            new DataView(
              frame.payload.buffer,
              frame.payload.byteOffset,
            ).getBigUint64(4, true) !== generationSeen
          )
            return;
          if (
            receipt?.event.kind === "data" &&
            repairChecked &&
            !inputStarted &&
            screenContains("INPUT_READY")
          ) {
            phase = "input";
            inputStarted = true;
            if (generationSeen === undefined)
              throw new Error("session.input_generation");
            const gen = generationSeen.toString();
            await requireRejected(
              commands.probeSessionInput(
                nonce,
                (generationSeen + 1n).toString(),
                "typed",
              ),
            );
            if (previous)
              await requireRejected(
                commands.probeSessionInput(nonce, previous.generation, "typed"),
              );
            await requireRejected(commands.probeSessionInput(nonce, gen, "\n"));
            inputQueue = new InputQueue(
              async (bytes) => {
                await commands.probeSessionInput(
                  nonce,
                  gen,
                  new TextDecoder("utf-8", { fatal: true }).decode(bytes),
                );
              },
              AbortSignal.any([lifetime.signal, activeView.signal]),
            );
            const queue = inputQueue;
            activeView.own(() => queue.cancel());
            inputArmed = true;
            try {
              if (renderer) {
                await renderer.keyboardSample(lifetime.signal);
                await Promise.all(inputJobs);
                if (inputText !== "typed中文🌍\r")
                  throw new Error("session.keyboard_path");
              } else {
                await inputQueue.enqueue("typed中文🌍\r");
              }
              inputChecked = true;
            } finally {
              inputArmed = false;
            }
          }

          if (
            receipt?.event.kind === "marker" &&
            admissionEpoch === outputEpoch &&
            geometry
          ) {
            markerSeen = true;
            idle?.observe({
              ...geometry,
              revision: receipt.event.revision,
              parsedBytes: receipt.event.parsedBytes,
            });
          }
          if (
            receipt?.event.kind === "repair" &&
            admissionEpoch === outputEpoch
          )
            idle?.observe(receipt.event.boundary);
          if (receipt?.event.kind === "ready") {
            phase = "ready";
            const boundary = receipt.event.boundary;
            if (!terminal || generationSeen === undefined)
              throw new Error("session.idle_not_ready");
            const surface = terminal;
            const generation = generationSeen.toString();
            const guardFit = () =>
              armViewDeadline(
                activeView,
                6000,
                `session.fit_no_transition_${resizeCount}`,
                (error) => {
                  failed ??= error;
                  cancel();
                },
              );
            if (dom && renderer && resizeCount === 0) {
              if (!screenContains("RESIZE_SEED"))
                throw new Error("session.resize_seed_missing");
              resizeChecked = false;
              await requireRejected(
                commands.probeSessionResize(
                  nonce,
                  (generationSeen + 1n).toString(),
                  60,
                  12,
                ),
              );
              phase = "fit";
              guardFit();
              renderer.watchFit(
                async (size) => {
                  expectedSize = size;
                  resizeCount++;
                  generations.expectReplacement(generationSeen as bigint);
                  await commands.probeSessionResize(
                    nonce,
                    generation,
                    size.columns,
                    size.rows,
                  );
                },
                (error) => {
                  failed ??= error;
                  cancel();
                },
              );
              return;
            }
            if (dom) {
              if (
                !expectedSize ||
                oldGeneration === undefined ||
                boundary.columns !== expectedSize.columns ||
                boundary.rows !== expectedSize.rows ||
                !screenContains("RESIZE_SEED")
              )
                throw new Error("session.resize_capture_mismatch");
              resizeChecked = resizeCount === 3;
              await requireRejected(
                commands.probeSessionAck(nonce, oldGeneration.toString(), 0),
              );
              await requireRejected(
                commands.probeSessionInput(
                  nonce,
                  oldGeneration.toString(),
                  "typed",
                ),
              );
              await requireRejected(
                commands.probeSessionResize(
                  nonce,
                  oldGeneration.toString(),
                  40,
                  12,
                ),
              );
              await requireRejected(
                commands.probeSessionDetach(nonce, oldGeneration.toString()),
              );
            }

            if (dom && renderer && resizeCount < 3) {
              phase = "fit";
              guardFit();
              renderer.watchFit(
                async (size) => {
                  expectedSize = size;
                  resizeCount++;
                  generations.expectReplacement(generationSeen as bigint);
                  await commands.probeSessionResize(
                    nonce,
                    generation,
                    size.columns,
                    size.rows,
                  );
                },
                (error) => {
                  failed ??= error;
                  cancel();
                },
              );
              if (resizeCount === 1) {
                for (const [width, height] of [
                  [464, 192],
                  [480, 184],
                  [496, 176],
                ]) {
                  dom.host.style.width = `${width}px`;
                  dom.host.style.height = `${height}px`;
                }
              } else {
                dom.host.style.width = "";
                dom.host.style.height = "";
              }
              return;
            }
            if (dom && poolResumePending) {
              if (
                boundary.parsedBytes < poolBoundary ||
                !screenContains("HIDDEN_SEED") ||
                !dom.pane.visibility.read()
              )
                throw new Error("session.pool_restore_missing");
              poolResumePending = false;
              poolChecked = true;
            }
            if (dom && renderer && !poolStarted) {
              phase = "pool_park";
              poolStarted = true;
              let before = wireCount;
              visibility = new SurfaceVisibility(
                {
                  retire: () => {
                    surfaceHidden = true;
                    before = wireCount;
                    inputArmed = false;
                    views.retire(BigInt(generation));
                  },
                  pause: async () => {
                    await commands.probeSessionVisibility(
                      nonce,
                      generation,
                      false,
                    );
                  },
                  resume: async () => {
                    if (lifetime.signal.aborted)
                      throw new Error("session.cancelled");
                    surfaceHidden = false;
                    poolResumePending = true;
                    generations.expectReplacement(BigInt(generation));
                    await commands.probeSessionVisibility(
                      nonce,
                      generation,
                      true,
                    );
                  },
                  failed: (error) => {
                    failed ??= error;
                    cancel();
                  },
                },
                lifetime.signal,
              );
              stopPane = dom.pane.visibility.bind(
                (shown) => visibility?.observe("container", shown),
                lifetime.signal,
              );
              dom.pane.park();
              await visibility.paused;
              if (dom.pane.visibility.read() || !dom.host.closest("[hidden]"))
                throw new Error("session.pool_not_parked");
              await requireRejected(
                commands.probeSessionInput(nonce, generation, "typed"),
              );
              await requireRejected(
                commands.probeSessionAck(nonce, generation, 0),
              );
              poolBoundary = BigInt(
                await commands.probeSessionHiddenOutput(nonce, generation),
              );
              await new Promise<void>((resolve) => setTimeout(resolve, 100));
              if (poolBoundary <= boundary.parsedBytes || wireCount !== before)
                throw new Error("session.pool_hidden_delivery");
              dom.pane.resume();
              await visibility.resumed;
              return;
            }

            if (dom && renderer && !pauseStarted) {
              phase = "visibility";
              pauseStarted = true;
              await requireRejected(
                commands.probeSessionVisibility(
                  nonce,
                  (generationSeen + 1n).toString(),
                  false,
                ),
              );
              visibility = new SurfaceVisibility(
                {
                  retire: () => {
                    nativeOnlyPaused = nativeHidden && !dom.host.hidden;
                    hiddenWireCount = wireCount;
                    surfaceHidden = true;
                    inputArmed = false;
                    inputQueue?.cancel();
                    idle?.dispose();
                    views.retire(BigInt(generation));
                  },
                  pause: async () => {
                    await commands.probeSessionVisibility(
                      nonce,
                      generation,
                      false,
                    );
                  },
                  resume: async () => {
                    if (!nativeCycle)
                      throw new Error("session.missing_native_cycle");
                    hiddenBoundary = BigInt(await nativeCycle);
                    if (lifetime.signal.aborted)
                      throw new Error("session.cancelled");
                    if (
                      hiddenBoundary <= boundary.parsedBytes ||
                      wireCount !== hiddenWireCount ||
                      !nativeOnlyPaused
                    )
                      throw new Error("session.native_only_pause_missing");
                    surfaceHidden = false;
                    resumePending = true;
                    generations.expectReplacement(BigInt(generation));
                    await commands.probeSessionVisibility(
                      nonce,
                      generation,
                      true,
                    );
                  },
                  failed: (error) => {
                    failed ??= error;
                    cancel();
                  },
                },
                lifetime.signal,
              );
              stopPane = dom.pane.visibility.bind(
                (shown) => visibility?.observe("container", shown),
                lifetime.signal,
              );
              let nativeFilter: NativeVisibilityFilter | undefined;
              const pendingNative = new NativeVisibilityBuffer();
              let nativeDone!: () => void;
              const nativeObserved = new Promise<void>((resolve) => {
                nativeDone = resolve;
              });
              const applyNative = (sample: NativeVisibilitySample) => {
                if (lifetime.signal.aborted) return;
                if (!nativeFilter) {
                  pendingNative.add(sample);
                  return;
                }
                const accepted = nativeFilter.accept(sample);
                if (!accepted) return;
                if (!accepted.valid) {
                  failed ??= new Error("session.native_query_failed");
                  cancel();
                  return;
                }
                const shown = accepted.visible && !accepted.minimized;
                if (!shown) nativeHidden = true;
                else if (nativeHidden) {
                  nativeRestored = true;
                  nativeDone();
                }
                visibility?.observe("native", shown);
              };
              stopNative = await events
                .nativeVisibilitySample(getCurrentWebviewWindow())
                .listen((event) => {
                  try {
                    applyNative(event.payload);
                  } catch (error) {
                    failed ??= error;
                    cancel();
                  }
                });
              if (lifetime.signal.aborted) throw new Error("session.cancelled");
              const nativeSnapshot =
                await commands.probeSessionNativeSnapshot(nonce);
              nativeFilter = new NativeVisibilityFilter(nativeSnapshot);
              visibility.observe(
                "native",
                nativeSnapshot.visible && !nativeSnapshot.minimized,
              );
              const pendingNativeSample = pendingNative.take(
                nativeSnapshot.incarnation,
              );
              if (pendingNativeSample) applyNative(pendingNativeSample);

              // Isolate native as the ONLY hidden source: no host.hidden update,
              // no page observer override, and no manual pause/resume invocation.
              await requireRejected(
                commands.probeSessionNativeCycle(
                  nonce,
                  (generationSeen + 1n).toString(),
                ),
              );
              nativeCycle = commands.probeSessionNativeCycle(nonce, generation);
              void nativeCycle.catch((error) => {
                failed ??= error;
                cancel();
              });
              await visibility.paused;
              await requireRejected(
                commands.probeSessionInput(nonce, generation, "typed"),
              );
              await requireRejected(
                commands.probeSessionAck(nonce, generation, 0),
              );
              await requireRejected(
                commands.probeSessionNativeCycle(nonce, generation),
              );
              await Promise.race([
                nativeObserved,
                new Promise<never>((_, reject) => {
                  nativeTimer = setTimeout(
                    () => reject(new Error("session.native_event_timeout")),
                    2000,
                  );
                }),
              ]);
              clearTimeout(nativeTimer);

              await visibility.resumed;

              return;
            }
            if (dom && resumePending) {
              if (
                boundary.parsedBytes < hiddenBoundary ||
                !screenContains("HIDDEN_SEED") ||
                !screenContains("RESIZE_SEED")
              )
                throw new Error("session.hidden_restore_missing");
              visibilityChecked = true;
              resumePending = false;
              await requireRejected(
                commands.probeSessionVisibility(
                  nonce,
                  oldGeneration?.toString() ?? "0",
                  true,
                ),
              );
            }

            const rendered = (b: AttachBoundary) => ({
              columns: b.columns,
              rows: b.rows,
              revision: b.revision.toString(),
              parsedBytes: b.parsedBytes.toString(),
              digest: digest(surface),
            });
            await requireRejected(
              commands.probeSessionCompare(
                nonce,
                (generationSeen + 1n).toString(),
                rendered(boundary),
              ),
            );
            if (previous)
              await requireRejected(
                commands.probeSessionCompare(
                  nonce,
                  previous.generation,
                  rendered(boundary),
                ),
              );
            const wrong = rendered(boundary);
            wrong.digest =
              wrong.digest.slice(0, -1) +
              (wrong.digest.endsWith("0") ? "1" : "0");
            if (
              (await commands.probeSessionCompare(nonce, generation, wrong)) !==
              "mismatch"
            )
              throw new Error("session.mismatch_missed");
            await requireRejected(
              commands.probeSessionCompare(
                nonce,
                generation,
                rendered(boundary),
              ),
            );
            let checked!: () => void;
            const done = new Promise<void>((resolve) => {
              checked = resolve;
            });
            phase = "idle";
            idle = new IdleComparison({
              compare: async (b, signal) => {
                if (signal.aborted) throw new Error("session.idle_aborted");
                const result = await commands.probeSessionCompare(
                  nonce,
                  generation,
                  rendered(b),
                );
                if (signal.aborted) throw new Error("session.idle_aborted");
                return result;
              },
              mismatch: () => {
                failed = new Error("session.idle_mismatch");
                checked();
              },
              matched: () => {
                idleChecked = true;
                checked();
              },
            });
            activeView.own(idle);
            // The finite helper is waiting for go, so no subsequent Data can
            // invalidate this Commit boundary while we test the idle scheduler.
            idle.observe(boundary);
            let timer: ReturnType<typeof setTimeout> | undefined;
            try {
              await Promise.race([
                done,
                new Promise<never>((_, reject) => {
                  timer = setTimeout(
                    () => reject(new Error("session.idle_timeout")),
                    2500,
                  );
                }),
              ]);
            } finally {
              if (timer !== undefined) clearTimeout(timer);
              idle.dispose();
            }
            if (failed || !idleChecked)
              throw failed ?? new Error("session.idle_unchecked");
            await requireRejected(
              commands.probeSessionMarker(
                nonce,
                (generationSeen + 1n).toString(),
              ),
            );
            phase = "idle";
            idle = new IdleComparison({
              requestMarker: async (signal) => {
                if (signal.aborted) throw new Error("session.marker_aborted");
                await commands.probeSessionMarker(nonce, generation);
                if (signal.aborted) throw new Error("session.marker_aborted");
              },
              compare: async (b, signal) => {
                if (signal.aborted) throw new Error("session.idle_aborted");
                const result = await commands.probeSessionCompare(
                  nonce,
                  generation,
                  rendered(b),
                );
                if (signal.aborted) throw new Error("session.idle_aborted");
                return result;
              },
              mismatch: (b) => {
                if (repairAttempted) {
                  failed = new Error("session.repair_diverged");
                  consumer?.cancel();
                  void commands.probeSessionResume(nonce).catch(() => {});
                  return;
                }
                repairAttempted = true;
                void commands
                  .probeSessionRepair(nonce, generation, rendered(b))
                  .then((result) => {
                    if (!activeView.isCurrent()) return;
                    if (result !== "queued")
                      throw new Error(`session.repair_${result}`);
                    repairAdmitted = true;
                  })
                  .catch((error) => {
                    if (!activeView.isCurrent()) return;
                    failed ??= error;
                    activeConsumer.cancel();
                    void commands.probeSessionResume(nonce).catch(() => {});
                  });
              },
              matched: () => {
                if (!markerSeen || markerChecked || !damaged || !repairAdmitted)
                  return;
                const buffer = surface.buffer.active;
                const paused = Array.from(
                  { length: surface.rows },
                  (_, row) =>
                    buffer
                      .getLine(buffer.baseY + row)
                      ?.translateToString(true) ?? "",
                ).some((line) => line.includes("MARKER_READY"));
                if (!paused) return;
                markerChecked = true;
                repairChecked = true;
                void commands.probeSessionResume(nonce).catch((error) => {
                  if (!activeView.isCurrent()) return;
                  failed ??= error;
                  activeConsumer.cancel();
                });
              },
            });
            activeView.own(idle);
            await commands.probeSessionGo(nonce);
          }
        },
        (error) => {
          failed ??= error;
          cancel();
        },
      );
    } catch (error) {
      failed ??= error;
      cancel();
    }
  };
  let stopAbort: (() => void) | undefined;
  try {
    if (lifetime.signal.aborted) throw new Error("session.cancelled");
    const stopped = new Promise<never>((_resolve, reject) => {
      const abort = () => reject(failed ?? new Error("session.cancelled"));
      lifetime.signal.addEventListener("abort", abort, { once: true });
      stopAbort = () => lifetime.signal.removeEventListener("abort", abort);
    });
    const native = commands
      .probeSession(nonce, channel, transferChannel)
      .catch((error: unknown) => {
        throw failed ?? error;
      });
    const result = await Promise.race([
      native,
      stopped,
      new Promise<never>((_, reject) => {
        deadline = setTimeout(() => {
          const error =
            failed ??
            new Error(
              `session.frontend_deadline_${phase}_r${resizeCount}_b${rebinds}_${lifetime.signal.aborted ? "aborted" : "active"}`,
            );
          failed = error;
          cancel();
          reject(error);
        }, 70000);
      }),
    ]);
    await latest;
    await transferView.finish();
    if (failed) throw failed;
    if (
      !terminal ||
      !final ||
      replies !== 0 ||
      generationSeen?.toString() !== result.generation ||
      (previous !== undefined &&
        BigInt(result.generation) <= BigInt(previous.generation)) ||
      digest(terminal) !== result.digest ||
      final.revision.toString() !== result.revision ||
      final.parsedBytes.toString() !== result.parsedBytes ||
      !result.peerDenied ||
      !result.peerTransferChecked ||
      !result.peerDisplayChecked ||
      !result.peerReloadChecked ||
      !result.peerReloadDisplayChecked ||
      !result.peerShellHintsChecked ||
      !result.peerReloadLiveChecked ||
      !result.peerNavigationRetired ||
      !result.peerInitialDisplayChecked ||
      !result.peerLiveDisplayChecked ||
      !result.peerBootstrapAdvanceChecked ||
      !result.peerPendingAckRetired ||
      !result.windowRevoked ||
      !result.sessionSurvived ||
      result.nativeExit !== 0 ||
      result.attachmentVersion !== protocol ||
      result.peakCreditBytes > 8192 + (protocol === 2 ? 28 : 20) ||
      !delayed ||
      !idleChecked ||
      !markerChecked ||
      !repairChecked ||
      !inputChecked ||
      !screenContains("INPUT_OK typed中文🌍") ||
      (dom !== undefined &&
        (!resizeChecked ||
          result.resizeCount !== 3 ||
          result.pauseCount !== 2 ||
          !result.nativeWindowChecked ||
          !result.nativeOnlyChecked ||
          !nativeOnlyPaused ||
          !visibilityChecked ||
          rebinds !== 5 ||
          !poolChecked))
    )
      throw new Error("session.integrity_failed");
    result.idleChecked = idleChecked;
    result.markerChecked = markerChecked;
    result.repairChecked = repairChecked;
    result.inputChecked = inputChecked;
    if (dom) {
      if (!renderer) throw new Error("session.no_dom_renderer");
      renderer.markFitChecked();
      renderer.markVisibilityChecked();
      if (!nativeHidden || !nativeRestored)
        throw new Error("session.native_signal_missing");
      renderer.markNativeSignalChecked();
      renderer.markNativePauseChecked();
      if (viewRebuilds !== 5 || !viewScope?.isCurrent())
        throw new Error("session.view_host_missing");
      renderer.markViewHostChecked();
      const root = dom.host.querySelector<HTMLElement>(".xterm");
      if (!root) throw new Error("session.pool_root_missing");
      const { verifyViewPool } = await import(
        "../features/probe/view-pool-probe"
      );
      verifyViewPool(dom.host, root, lifetime.signal);
      renderer.markViewPoolChecked();
      renderer.markActiveParkChecked();
      dom.complete(renderer.evidence());
    }
    previous = { nonce, generation: result.generation };
    return result;
  } finally {
    stopPane?.();
    stopAbort?.();
    inputArmed = false;
    inputQueue?.cancel();

    stopNative?.();
    clearTimeout(nativeTimer);
    visibility?.dispose();
    clearTimeout(deadline);
    idle?.dispose();
    cancel();
    views.dispose();
    lifetime.abort();
    dom?.signal.removeEventListener("abort", cancel);

    transferView.dispose();
    transferChannel.onmessage = () => {};
    channel.onmessage = () => {};
    if (dom) {
      dom.host.hidden = false;
      dom.host.style.width = "";
      dom.host.style.height = "";
    }
  }
}
