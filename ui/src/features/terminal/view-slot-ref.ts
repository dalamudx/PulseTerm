import type { PooledViewController, ViewHost, ViewLease } from "./view-pool";

interface Mount {
  slot: HTMLDivElement;
  lease: ViewLease;
  parked: boolean;
}
/** React slot ownership survives imperative park/resume, while backend and pool
 * generations remain separate. Cleanup follows this slot's latest fenced lease. */
export class ViewSlotOwner {
  private current: Mount | undefined;
  constructor(
    private readonly pool: ViewHost,
    private readonly key: string,
    private readonly factory: (
      element: HTMLElement,
      signal: AbortSignal,
    ) => PooledViewController,
    private readonly mounted?: (owner: ViewSlotOwner) => void,
  ) {}
  readonly ref = (slot: HTMLDivElement | null) => {
    if (!slot) return;
    const mount: Mount = {
      slot,
      lease: this.pool.mount(this.key, slot, this.factory),
      parked: false,
    };
    this.current = mount;
    try {
      this.mounted?.(this);
    } catch {
      if (this.current === mount) this.current = undefined;
      mount.lease.release();
      throw new Error("pool.slot_observer_failed");
    }
    return () => {
      if (this.current !== mount) return;
      this.current = undefined;
      mount.lease.release();
    };
  };
  isMounted(): boolean {
    return this.current !== undefined;
  }
  park(): boolean {
    const mount = this.current;
    if (!mount || mount.parked) return false;
    if (!mount.lease.release()) return false;
    mount.parked = true;
    return true;
  }
  resume(): ViewLease {
    const mount = this.current;
    if (!mount?.parked) throw new Error("pool.slot_not_parked");
    const next = mount.lease.resume(mount.slot);
    mount.lease = next;
    mount.parked = false;
    return next;
  }
}
export function viewSlotRef(
  pool: ViewHost,
  key: string,
  factory: (element: HTMLElement, signal: AbortSignal) => PooledViewController,
) {
  return new ViewSlotOwner(pool, key, factory).ref;
}
