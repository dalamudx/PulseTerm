// Explicit classified GBK fixture: actual backend actor, Channel and visible xterm.
import { Channel } from "@tauri-apps/api/core";
import {
  createDomRenderer,
  type DomRenderer,
} from "../features/probe/dom-renderer";
import { suppressQueryReplies } from "../features/terminal/query-policy";
import { SharedUnicodeProvider } from "../features/terminal/unicode-provider";
import { Utf8Stream } from "../features/terminal/utf8-stream";
import { AttachmentV2Consumer, type V2Boundary } from "./attachment-v2";
import { decodeFrame } from "./frames";
import { commands, type EncodingEvidence } from "./gen/commands";
import { probeAttachmentEnvelope } from "./probe-attachment-codec";
import { digest } from "./session-probe";
export async function verifyEncodingWindow(
  host: HTMLElement,
  signal: AbortSignal,
): Promise<EncodingEvidence> {
  const life = new AbortController();
  const stop = () => life.abort();
  signal.addEventListener("abort", stop, { once: true });
  if (signal.aborted) stop();
  const nonce = crypto.randomUUID();
  let renderer: DomRenderer | undefined;
  let disposeQueries: (() => void) | undefined;
  let consumer: AttachmentV2Consumer | undefined;
  let decoder = new Utf8Stream();
  let gen = 0n,
    ready = 0,
    acked = 0,
    marker = false,
    history = false;
  let final: Omit<V2Boundary, "columns" | "rows"> | undefined;
  let last: Promise<unknown> = Promise.resolve();
  let failed: unknown;
  const timer = setTimeout(() => {
    failed = new Error("encoding.frontend_deadline");
    life.abort();
    consumer?.cancel();
  }, 25000);
  const write = async (text: string, s: AbortSignal) => {
    if (!renderer || s.aborted || life.signal.aborted)
      throw new Error("encoding.cancelled");
    if (text)
      await new Promise<void>((done) => renderer?.terminal.write(text, done));
    if (s.aborted || life.signal.aborted) throw new Error("encoding.cancelled");
  };
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (data) => {
    try {
      if (life.signal.aborted) return;
      if (!(data instanceof ArrayBuffer))
        throw new Error("encoding.not_binary");
      const wire = new Uint8Array(data);
      const frame = decodeFrame(wire);
      const envelope = probeAttachmentEnvelope(frame.payload, 2);
      if (envelope.generation < gen) return;
      if (envelope.part === 1 && frame.sequence === 0) {
        if (envelope.generation <= gen)
          throw new Error("encoding.repeated_generation");
        consumer?.cancel();
        gen = envelope.generation;
        decoder = new Utf8Stream();
        consumer = new AttachmentV2Consumer(
          gen,
          {
            reset: async (b, s) => {
              if (b.sourceBytes !== 76n || b.utf8Bytes !== 75n)
                throw new Error("encoding.pending_capture_boundary");
              disposeQueries?.();
              renderer?.dispose();
              renderer = createDomRenderer(
                host,
                b,
                AbortSignal.any([s, life.signal]),
              );
              const p = new SharedUnicodeProvider();
              renderer.terminal.unicode.register(p);
              renderer.terminal.unicode.activeVersion = p.version;
              disposeQueries = suppressQueryReplies(renderer.terminal);
            },
            write: async (bytes, s) => write(decoder.feed(bytes), s),
            marker: async (b, s) => {
              if (!renderer || b.sourceBytes !== 78n || b.utf8Bytes !== 78n)
                throw new Error("encoding.marker_boundary");
              const t = renderer.terminal,
                buf = t.buffer.active;
              const before = [buf.cursorX, buf.cursorY, buf.baseY].join(":");
              await renderer.settle(AbortSignal.any([s, life.signal]));
              if ([buf.cursorX, buf.cursorY, buf.baseY].join(":") !== before)
                throw new Error("encoding.marker_changed_screen");
              marker = true;
            },
            finish: async (b, s) => {
              await write(decoder.finish(), s);
              await renderer?.settle(AbortSignal.any([s, life.signal]));
              final = b;
            },
          },
          async (g, sequence) => {
            if (life.signal.aborted) throw new Error("encoding.cancelled");
            await commands.probeSessionAck(nonce, g.toString(), sequence);
            acked++;
          },
        );
      }
      const active = consumer;
      if (!active || envelope.generation !== gen)
        throw new Error("encoding.no_current_generation");
      last = active
        .enqueue(wire)
        .then(async (receipt) => {
          if (!receipt || active !== consumer || life.signal.aborted) return;
          if (receipt.event.kind === "ready") {
            if (!renderer) throw new Error("encoding.no_renderer");
            await renderer.settle(life.signal);
            const b = renderer.terminal.buffer.active;
            if (
              b.baseY < 4 ||
              b.getLine(0)?.translateToString(true) !== "row" ||
              receipt.event.history.length === 0
            )
              throw new Error("encoding.history_missing");
            history = true;
            ready++;
            if (ready > 2) throw new Error("encoding.extra_replacement");
            await commands.probeEncodingInput(
              nonce,
              receipt.generation.toString(),
              ready === 1 ? "中文" : "确认",
            );
          }
        })
        .catch((error) => {
          failed = error;
          life.abort();
          consumer?.cancel();
        });
    } catch (error) {
      failed = error;
      life.abort();
      consumer?.cancel();
    }
  };
  let abort: (() => void) | undefined;
  try {
    if (life.signal.aborted) throw new Error("encoding.cancelled");
    const stopped = new Promise<never>((_resolve, reject) => {
      abort = () => reject(failed ?? new Error("encoding.cancelled"));
      life.signal.addEventListener("abort", abort, { once: true });
    });
    const result = await Promise.race([
      commands.probeEncoding(nonce, channel),
      stopped,
    ]);
    await last;
    if (failed) throw failed;
    if (
      !renderer ||
      !final ||
      ready !== 2 ||
      acked < 4 ||
      !marker ||
      !history ||
      result.generation !== gen.toString() ||
      final.sourceBytes.toString() !== result.sourceBytes ||
      final.utf8Bytes.toString() !== result.utf8Bytes ||
      final.revision.toString() !== result.revision ||
      result.sourceBytes !== "89" ||
      result.utf8Bytes !== "91" ||
      digest(renderer.terminal) !== result.digest ||
      !result.reverseInputChecked ||
      !result.sourceOnlyMarker ||
      result.replacements !== 1 ||
      result.peakCreditBytes > 64
    )
      throw new Error("encoding.final_projection");
    const text = Array.from({ length: renderer.terminal.rows }, (_, i) =>
      renderer?.terminal.buffer.active
        .getLine((renderer?.terminal.buffer.active.baseY ?? 0) + i)
        ?.translateToString(true),
    ).join("\n");
    if (!text.includes("中文") || !text.includes("GBK_OK"))
      throw new Error("encoding.visible_text");
    const evidence = renderer.evidence();
    return {
      ...result,
      domRowsChecked: evidence.rowsChecked,
      historyChecked: history,
      callbackAckChecked: true,
    };
  } finally {
    clearTimeout(timer);
    if (abort) life.signal.removeEventListener("abort", abort);
    signal.removeEventListener("abort", stop);
    life.abort();
    consumer?.cancel();
    disposeQueries?.();
    renderer?.dispose();
  }
}
