export interface ViewResource {
  dispose(): void;
}
type Owned = ViewResource | (() => void);
function dispose(value: Owned) {
  if (typeof value === "function") value();
  else value.dispose();
}
const MAX_GENERATION = (1n << 64n) - 1n;
/** One pane's disposable attachment surfaces, not session ownership or a parked
 * DOM pool. Generation is supplied by the ordered backend Begin, never invented. */
export class GenerationViewHost {
  private current: ViewScope | undefined;
  private last = 0n;
  private closed = false;
  private changing = false;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly signal: AbortSignal,
    private readonly failed: (error: Error) => void,
  ) {
    signal.addEventListener("abort", this.abort, { once: true });
    if (signal.aborted) this.dispose();
  }
  begin(generation: bigint): ViewScope {
    if (this.closed || this.signal.aborted) throw new Error("view.closed");
    if (
      typeof generation !== "bigint" ||
      generation <= this.last ||
      generation > MAX_GENERATION
    )
      throw new Error("view.generation");
    if (this.changing) throw new Error("view.transition");
    this.changing = true;
    try {
      if (this.current) this.retire(this.current.generation);
      if (this.closed) throw new Error("view.cleanup_failed");
      this.last = generation;
      const scope: ViewScope = new ViewScope(
        generation,
        (): boolean => this.current === scope && !this.closed,
        (g) => this.retire(g),
        () => this.cleanupFailed(),
      );
      this.current = scope;
      return scope;
    } finally {
      this.changing = false;
    }
  }
  retire(generation: bigint): boolean {
    const scope = this.current;
    if (!scope || scope.generation !== generation) return false;
    const changing = this.changing;
    this.changing = true;
    try {
      this.current = undefined;
      scope.release();
      return true;
    } finally {
      this.changing = changing;
    }
  }
  dispose(): void {
    if (this.closed) return;
    this.closed = true;
    this.signal.removeEventListener("abort", this.abort);
    const scope = this.current;
    this.current = undefined;
    scope?.release();
  }
  private cleanupFailed(): void {
    this.dispose();
    try {
      this.failed(new Error("view.cleanup_failed"));
    } catch {
      /* Failure reporting must not stop remaining resource cleanup. */
    }
  }
}
export class ViewScope {
  private readonly lifetime = new AbortController();
  readonly signal = this.lifetime.signal;
  private readonly owned = new Set<Owned>();
  private readonly disposed = new WeakSet<Owned>();
  private disposeOnce(value: Owned) {
    if (this.disposed.has(value)) return;
    this.disposed.add(value);
    dispose(value);
  }
  constructor(
    readonly generation: bigint,
    private readonly current: () => boolean,
    private readonly retire: (generation: bigint) => boolean,
    private readonly failed: () => void,
  ) {}
  isCurrent(): boolean {
    return !this.signal.aborted && this.current();
  }
  /** Idempotent ownership by object/function identity. Stale registration disposes
   * immediately and rejects; cleanup may never be transferred to a new scope. */
  own<T extends Owned>(value: T): T {
    if (!this.isCurrent()) {
      try {
        this.disposeOnce(value);
      } catch {
        throw new Error("view.late_cleanup_failed");
      }
      throw new Error("view.retired");
    }
    this.owned.add(value);
    return value;
  }
  async create<T extends ViewResource>(
    factory: (signal: AbortSignal) => Promise<T>,
  ): Promise<T> {
    if (!this.isCurrent()) throw new Error("view.retired");
    return new Promise<T>((resolve, reject) => {
      const abort = () => {
        this.signal.removeEventListener("abort", abort);
        reject(new Error("view.retired"));
      };
      this.signal.addEventListener("abort", abort, { once: true });
      // Caller cancellation settles immediately; the factory may still finish.
      // Observe its tail and dispose late values instead of leaking them.
      void Promise.resolve()
        .then(() => {
          if (!this.isCurrent()) throw new Error("view.retired");
          return factory(this.signal);
        })
        .then((value) => {
          this.signal.removeEventListener("abort", abort);
          try {
            resolve(this.own(value));
          } catch (error) {
            reject(error);
          }
        })
        .catch(() => {
          this.signal.removeEventListener("abort", abort);
          if (this.isCurrent()) this.retire(this.generation);
          reject(new Error("view.creation_failed"));
        });
    });
  }
  /** Called by the host; revoke callbacks before cleanup, then release in reverse
   * registration order. One throwing disposer must not leak the remaining ones. */
  release(): void {
    if (this.signal.aborted) return;
    this.lifetime.abort();
    const resources = [...this.owned].reverse();
    this.owned.clear();
    let failure = false;
    for (const resource of resources) {
      try {
        this.disposeOnce(resource);
      } catch {
        failure = true;
      }
    }
    if (failure) this.failed();
  }
}
