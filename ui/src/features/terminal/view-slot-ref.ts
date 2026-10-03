import type { PooledViewController, ViewHost } from "./view-pool";
/** React19 cleanup belongs to this mount ticket, never to key alone. */
export function viewSlotRef(
  pool: ViewHost,
  key: string,
  factory: (element: HTMLElement, signal: AbortSignal) => PooledViewController,
) {
  return (slot: HTMLDivElement | null) => {
    if (!slot) return;
    const lease = pool.mount(key, slot, factory);
    return () => {
      lease.release();
    };
  };
}
