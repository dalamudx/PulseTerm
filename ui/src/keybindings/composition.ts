/** Browser IME ownership. Legacy WebKit can report keyCode 229 without isComposing. */
export interface CompositionKey {
  readonly key: string;
  readonly isComposing?: boolean;
  readonly keyCode?: number;
}
export function isCompositionKey(event: CompositionKey): boolean {
  return (
    event.isComposing === true ||
    event.keyCode === 229 ||
    event.key === "Process"
  );
}

/** Tracks composition only; never cancels events, reads text or emits terminal input. */
export class CompositionGuard {
  private active = false;
  private disposed = false;
  private readonly start = () => {
    if (this.disposed) return;
    this.active = true;
    this.started();
  };
  private readonly end = () => {
    this.active = false;
  };
  private readonly abort = () => this.dispose();
  constructor(
    private readonly target: EventTarget,
    private readonly signal: AbortSignal,
    private readonly started: () => void = () => {},
  ) {
    if (signal.aborted) {
      this.disposed = true;
      return;
    }
    target.addEventListener("compositionstart", this.start, true);
    target.addEventListener("compositionend", this.end, true);
    target.addEventListener("blur", this.end, true);
    target.addEventListener("pagehide", this.end, true);
    signal.addEventListener("abort", this.abort, { once: true });
  }
  handles(event: CompositionKey): boolean {
    return !this.disposed && (this.active || isCompositionKey(event));
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.active = false;
    this.target.removeEventListener("compositionstart", this.start, true);
    this.target.removeEventListener("compositionend", this.end, true);
    this.target.removeEventListener("blur", this.end, true);
    this.target.removeEventListener("pagehide", this.end, true);
    this.signal.removeEventListener("abort", this.abort);
  }
}
