// Keep decoder-only UTF-8 fragments out of xterm's print path. This decoder is
// per session/view stream, never shared, and must be consumed in byte order.
export class Utf8Stream {
  private readonly decoder = new TextDecoder("utf-8", { ignoreBOM: true });
  private ended = false;
  feed(bytes: Uint8Array): string {
    if (this.ended) throw new Error("UTF-8 stream already ended");
    if (bytes.byteLength > 64 * 1024)
      throw new Error("Terminal chunk exceeds 64 KiB");
    return this.decoder.decode(bytes, { stream: true });
  }
  finish(): string {
    if (this.ended) return "";
    this.ended = true;
    return this.decoder.decode();
  }
}
