import { useMemo } from "react";
import type { PooledViewController, ViewHost } from "./view-pool";
import { viewSlotRef } from "./view-slot-ref";
export function ViewSlot({
  pool,
  viewKey,
  factory,
  className,
}: {
  pool: ViewHost;
  viewKey: string;
  factory: (element: HTMLElement, signal: AbortSignal) => PooledViewController;
  className?: string;
}) {
  const attach = useMemo(
    () => viewSlotRef(pool, viewKey, factory),
    [pool, viewKey, factory],
  );
  return <div className={className} ref={attach} />;
}
