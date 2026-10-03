// Bounded keyboard/IME control input, not a paste policy or native-write receipt.
export class InputQueue {
  private readonly queue: {
    bytes: Uint8Array;
    resolve: () => void;
    reject: (e: Error) => void;
  }[] = [];
  private bytes = 0;
  private running = false;
  private closed = false;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private readonly abort = () => this.cancel();
  constructor(
    private readonly send: (bytes: Uint8Array) => Promise<void>,
    private readonly signal: AbortSignal,
  ) {
    signal.addEventListener("abort", this.abort, { once: true });
    if (signal.aborted) this.cancel();
  }
  get queuedBytes() {
    return this.bytes;
  }
  enqueue(text: string): Promise<void> {
    if (this.closed) return Promise.reject(new Error("input.closed"));
    if (
      !text ||
      text.length > 1024 ||
      /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(
        text,
      )
    )
      return Promise.reject(new Error("input.invalid"));
    const bytes = new TextEncoder().encode(text);
    if (bytes.length > 1024)
      return Promise.reject(new Error("input.too_large"));
    if (this.queue.length >= 32 || this.bytes + bytes.length > 8192) {
      this.fail(new Error("input.overloaded"));
      return Promise.reject(new Error("input.overloaded"));
    }
    const result = new Promise<void>((resolve, reject) =>
      this.queue.push({ bytes, resolve, reject }),
    );
    this.bytes += bytes.length;
    void this.drain();
    return result;
  }
  cancel(): void {
    this.fail(new Error("input.cancelled"));
  }
  private fail(error: Error) {
    if (this.closed) return;
    this.closed = true;
    clearTimeout(this.timer);
    this.signal.removeEventListener("abort", this.abort);
    for (const job of this.queue) job.reject(error);
    this.queue.length = 0;
    this.bytes = 0;
  }
  private async drain() {
    if (this.running || this.closed) return;
    this.running = true;
    try {
      while (this.queue.length && !this.closed) {
        const job = this.queue[0];
        if (!job) break;
        // An IPC timeout is uncertain admission: invalidate, NEVER retry this data.
        this.timer = setTimeout(
          () => this.fail(new Error("input.uncertain")),
          1000,
        );
        try {
          await this.send(job.bytes);
        } catch {
          this.fail(new Error("input.transport"));
          break;
        }
        clearTimeout(this.timer);
        if (this.closed) break;
        this.queue.shift();
        this.bytes -= job.bytes.length;
        job.resolve();
      }
    } finally {
      this.running = false;
    }
  }
}
