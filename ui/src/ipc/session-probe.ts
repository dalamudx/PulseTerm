// Developer-only real-WebView validation: @xterm/headless is not a DOM renderer.
import { Channel } from "@tauri-apps/api/core";
import type { IBufferCell, Terminal as TerminalType } from "@xterm/headless";
import headless from "@xterm/headless";
import { IdleComparison } from "../features/terminal/idle-comparison";
import { suppressQueryReplies } from "../features/terminal/query-policy";
import { screenDigest } from "../features/terminal/screen-digest";
import { SharedUnicodeProvider } from "../features/terminal/unicode-provider";
import { Utf8Stream } from "../features/terminal/utf8-stream";
import type { AttachBoundary } from "./attachment";
import { AttachmentConsumer } from "./attachment-consumer";
import { decodeFrame } from "./frames";
import { commands, type SessionEvidence } from "./gen/commands";

const { Terminal } = headless;
function color(cell: IBufferCell, foreground: boolean): string {
  if (foreground ? cell.isFgDefault() : cell.isBgDefault()) return "default";
  const value = foreground ? cell.getFgColor() : cell.getBgColor();
  return (foreground ? cell.isFgRGB() : cell.isBgRGB())
    ? `rgb:${(value >>> 16) & 255},${(value >>> 8) & 255},${value & 255}`
    : `idx:${value}`;
}
function digest(terminal: TerminalType): string {
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
export async function verifySessionChannel(): Promise<SessionEvidence> {
  const nonce = crypto.randomUUID();
  let terminal: TerminalType | undefined;
  let disposePolicy: (() => void) | undefined;
  let decoder = new Utf8Stream();
  let replies = 0;
  let final: { revision: bigint; parsedBytes: bigint } | undefined;
  let consumer: AttachmentConsumer | undefined;
  let failed: unknown;
  let latest: Promise<unknown> = Promise.resolve();
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
  };
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
  channel.onmessage = (data) => {
    try {
      if (!(data instanceof ArrayBuffer)) throw new Error("session.not_binary");
      const wire = new Uint8Array(data);
      const frame = decodeFrame(wire);
      if (frame.kind === 8 && frame.payload[1] === 5) {
        outputEpoch++;
        idle?.activity();
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
        consumer = new AttachmentConsumer(
          generation,
          {
            reset: async (boundary) => {
              if (terminal) throw new Error("session.repeated_reset");
              terminal = new Terminal({
                cols: boundary.columns,
                rows: boundary.rows,
                scrollback: 128,
                allowProposedApi: true,
              });
              const provider = new SharedUnicodeProvider();
              terminal.unicode.register(provider);
              terminal.unicode.activeVersion = provider.version;
              disposePolicy = suppressQueryReplies(terminal);
              terminal.onData(() => replies++);
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
              }
            },
            checkpoint: async (_revision, _parsed, signal) =>
              write("\x1b[?2026l", signal),
            finish: async (revision, parsedBytes, signal) => {
              await write(decoder.finish(), signal);
              final = { revision, parsedBytes };
            },
          },
          async (gen, sequence) => {
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
      }
      latest = consumer
        .enqueue(wire)
        .then(async (receipt) => {
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
            const boundary = receipt.event.boundary;
            if (!terminal || generationSeen === undefined)
              throw new Error("session.idle_not_ready");
            const surface = terminal;
            const generation = generationSeen.toString();
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
                    if (result !== "queued")
                      throw new Error(`session.repair_${result}`);
                    repairAdmitted = true;
                  })
                  .catch((error) => {
                    failed = error;
                    consumer?.cancel();
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
                  failed = error;
                  consumer?.cancel();
                });
              },
            });
            await commands.probeSessionGo(nonce);
          }
        })
        .catch((error) => {
          failed = error;
          consumer?.cancel();
        });
    } catch (error) {
      failed = error;
      consumer?.cancel();
    }
  };
  try {
    const result = await commands.probeSession(nonce, channel);
    await latest;
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
      !result.windowRevoked ||
      !result.sessionSurvived ||
      result.nativeExit !== 0 ||
      result.peakCreditBytes > 8212 ||
      !delayed ||
      !idleChecked ||
      !markerChecked ||
      !repairChecked
    )
      throw new Error("session.integrity_failed");
    result.idleChecked = idleChecked;
    result.markerChecked = markerChecked;
    result.repairChecked = repairChecked;
    previous = { nonce, generation: result.generation };
    return result;
  } finally {
    idle?.dispose();
    consumer?.cancel();
    disposePolicy?.();
    terminal?.dispose();
    channel.onmessage = () => {};
  }
}
