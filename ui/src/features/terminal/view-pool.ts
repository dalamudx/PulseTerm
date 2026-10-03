export interface PooledViewController {
  /** Must revoke local input/render work synchronously on false. Backend pause,
   * resume and generation handoff remain the controller's serialized protocol. */
  setVisible(visible: boolean): void;
  dispose(): void | Promise<void>;
}
export interface ViewLease {
  readonly element: HTMLElement;
  /** Mount lifetime, not controller/session lifetime or a backend capability. */
  readonly signal: AbortSignal;
  isCurrent(): boolean;
  release(): boolean;
}
export interface ViewPoolPolicy {
  maxViews: number;
  maxParked: number;
  ttlMs: number;
}
interface Entry {
  key: string;
  element: HTMLElement;
  life: AbortController;
  controller?: PooledViewController;
  mount?: AbortController;
  slot?: HTMLElement;
  pending: number;
  parkedAt?: number;
  order: bigint;
  disposing: boolean;
}
/** Stable pane containers outside React; controller identity survives parking.
 * Window-local keys and mount tickets are NOT backend owner/generation tokens. */
export class ViewHost {
  private entries = new Map<string, Entry>();
  private retiring = new Set<Entry>();
  private timer: ReturnType<typeof setTimeout> | undefined;
  private closed = false;
  private changing = false;
  private order = 0n;
  private readonly policy: ViewPoolPolicy;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly parking: HTMLElement,
    private readonly signal: AbortSignal,
    private readonly failed: (key: string, error: Error) => void,
    policy: Partial<ViewPoolPolicy> = {},
    private readonly now: () => number = () => performance.now(),
  ) {
    this.policy = { maxViews: 32, maxParked: 6, ttlMs: 300000, ...policy };
    const p = this.policy;
    if (
      !Number.isInteger(p.maxViews) ||
      p.maxViews < 1 ||
      p.maxViews > 64 ||
      !Number.isInteger(p.maxParked) ||
      p.maxParked < 0 ||
      p.maxParked > p.maxViews ||
      !Number.isFinite(p.ttlMs) ||
      p.ttlMs < 1 ||
      p.ttlMs > 2147483647
    )
      throw new Error("pool.policy");
    parking.hidden = true;
    parking.inert = true;
    signal.addEventListener("abort", this.abort, { once: true });
    if (signal.aborted) this.dispose();
  }
  get size() {
    return this.entries.size;
  }
  get pendingDisposals() {
    return this.retiring.size;
  }
  mount(
    key: string,
    slot: HTMLElement,
    factory: (
      element: HTMLElement,
      signal: AbortSignal,
    ) => PooledViewController,
  ): ViewLease {
    if (
      typeof key !== "string" ||
      !key ||
      key.length > 128 ||
      !slot.isConnected ||
      slot.ownerDocument !== this.parking.ownerDocument ||
      this.parking.contains(slot)
    )
      throw new Error("pool.slot");
    return this.mutate(() => {
      this.sweepInternal();
      let entry = this.entries.get(key);
      if (
        [...this.entries.values()].some(
          (e) => e !== entry && e.mount && e.slot === slot,
        )
      )
        throw new Error("pool.slot_occupied");
      if (entry?.element.contains(slot)) throw new Error("pool.cyclic_slot");
      if (!entry) {
        if (this.entries.size + this.retiring.size >= this.policy.maxViews)
          throw new Error("pool.capacity");
        const element = slot.ownerDocument.createElement("div");
        entry = {
          key,
          element,
          life: new AbortController(),
          order: 0n,
          disposing: false,
          pending: 0,
        };
        this.entries.set(key, entry);
        try {
          slot.appendChild(element);
          entry.controller = factory(element, entry.life.signal);
          if (this.closed || entry.life.signal.aborted)
            throw new Error("pool.closed");
        } catch {
          this.drop(entry);
          this.arm();
          throw new Error("pool.creation_failed");
        }
      }
      const current = entry;
      const old = current.mount;
      current.mount = undefined;
      old?.abort();
      const ticket = new AbortController();
      current.mount = ticket;
      current.slot = slot;
      const wasParked = current.parkedAt !== undefined;
      current.parkedAt = undefined;
      try {
        slot.appendChild(current.element);
        if (!old || wasParked) this.notify(current, true);
      } catch {
        this.drop(current);
        this.arm();
        throw new Error("pool.mount_failed");
      }
      if (
        this.closed ||
        this.entries.get(key) !== current ||
        ticket.signal.aborted
      )
        throw new Error("pool.closed");
      this.arm();
      const isCurrent = () =>
        !this.closed &&
        this.entries.get(key) === current &&
        current.mount === ticket &&
        !ticket.signal.aborted;
      return {
        element: current.element,
        signal: ticket.signal,
        isCurrent,
        release: () => {
          if (!isCurrent()) return false;
          return this.mutate(() => {
            current.mount = undefined;
            current.slot = undefined;
            ticket.abort();
            current.parkedAt = this.now();
            current.order = ++this.order;
            try {
              this.parking.appendChild(current.element);
              this.notify(current, false);
            } catch {
              this.drop(current);
              this.arm();
              throw new Error("pool.park_failed");
            }
            this.sweepInternal();
            this.arm();
            return true;
          });
        },
      };
    });
  }
  sweep(): void {
    if (this.closed) return;
    this.mutate(() => {
      this.sweepInternal();
      this.arm();
    });
  }
  dispose(): void {
    if (this.closed) return;
    this.closed = true;
    this.signal.removeEventListener("abort", this.abort);
    clearTimeout(this.timer);
    this.timer = undefined;
    for (const entry of [...this.entries.values()]) this.drop(entry);
  }
  private mutate<T>(work: () => T): T {
    if (this.closed || this.signal.aborted) throw new Error("pool.closed");
    if (this.changing) throw new Error("pool.transition");
    this.changing = true;
    try {
      return work();
    } finally {
      this.changing = false;
    }
  }
  private sweepInternal() {
    const now = this.now();
    const parked = [...this.entries.values()]
      .filter((e) => !e.mount && e.parkedAt !== undefined)
      .sort((a, b) => (a.order < b.order ? -1 : 1));
    for (const entry of parked)
      if (now - (entry.parkedAt ?? now) >= this.policy.ttlMs) this.drop(entry);
    const remaining = parked.filter((e) => this.entries.get(e.key) === e);
    for (const entry of remaining.slice(
      0,
      Math.max(0, remaining.length - this.policy.maxParked),
    ))
      this.drop(entry);
  }
  private arm() {
    clearTimeout(this.timer);
    this.timer = undefined;
    if (this.closed) return;
    let next = Infinity;
    for (const entry of this.entries.values())
      if (entry.parkedAt !== undefined)
        next = Math.min(next, entry.parkedAt + this.policy.ttlMs);
    if (Number.isFinite(next))
      this.timer = setTimeout(
        () => this.sweep(),
        Math.max(1, next - this.now()),
      );
  }
  private notify(entry: Entry, visible: boolean) {
    const result: unknown = entry.controller?.setVisible(visible);
    if (result && typeof result === "object" && "then" in result) {
      this.observeTail(entry, result, "visibility");
      throw new Error("pool.visibility_async");
    }
  }

  private observeTail(
    entry: Entry,
    result: unknown,
    kind: "visibility" | "dispose",
  ) {
    entry.pending++;
    this.retiring.add(entry);
    const deadline = setTimeout(
      () => this.report(entry, `pool.${kind}_uncertain`),
      1500,
    );
    void Promise.resolve(result)
      .catch(() => this.report(entry, `pool.${kind}_failed`))
      .finally(() => {
        clearTimeout(deadline);
        entry.pending--;
        if (entry.pending === 0) this.retiring.delete(entry);
      });
  }

  private report(entry: Entry, code: string) {
    try {
      this.failed(entry.key, new Error(code));
    } catch {
      /* Diagnostics must not stop other cleanup. */
    }
  }
  private drop(entry: Entry) {
    if (this.entries.get(entry.key) === entry) this.entries.delete(entry.key);
    const mount = entry.mount;
    entry.mount = undefined;
    entry.slot = undefined;
    mount?.abort();
    entry.life.abort();
    try {
      entry.element.remove();
    } catch {
      this.report(entry, "pool.remove_failed");
    }
    if (!entry.controller || entry.disposing) return;
    entry.disposing = true;
    try {
      const result = entry.controller.dispose();
      if (result) this.observeTail(entry, result, "dispose");
    } catch {
      this.report(entry, "pool.dispose_failed");
    }
  }
}
