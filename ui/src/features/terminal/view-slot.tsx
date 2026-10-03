import { useMemo } from "react";
import type { PooledViewController, ViewHost } from "./view-pool";
import { ViewSlotOwner } from "./view-slot-ref";
export function ViewSlot({
  pool,
  viewKey,
  factory,
  className,
  onMount,
}: {
  pool: ViewHost;
  viewKey: string;
  factory: (element: HTMLElement, signal: AbortSignal) => PooledViewController;
  className?: string;
  onMount?: (owner: ViewSlotOwner) => void;
}) {
  const owner = useMemo(
    () => new ViewSlotOwner(pool, viewKey, factory, onMount),
    [pool, viewKey, factory, onMount],
  );
  return <div className={className} ref={owner.ref} />;
}
