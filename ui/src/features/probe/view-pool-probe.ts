import { ViewHost } from "../terminal/view-pool";
/** EOF-only DOM/controller ownership test. It does NOT claim backend pause on park. */
export function verifyViewPool(
  host: HTMLElement,
  root: HTMLElement,
  signal: AbortSignal,
): void {
  if (signal.aborted || !host.isConnected || !host.contains(root))
    throw new Error("pool.probe_unavailable");
  const doc = host.ownerDocument,
    parking = doc.createElement("div"),
    slot = doc.createElement("div");
  slot.className = "terminal-host";
  host.parentElement?.appendChild(slot);
  doc.body.appendChild(parking);
  let now = 0,
    created = 0,
    disposed = 0,
    oldDropped = 0,
    recentDropped = 0,
    failure: Error | undefined;
  const counts = () => ({ disposed, recentDropped });
  const shown: boolean[] = [];
  const life = new AbortController();
  const pool = new ViewHost(
    parking,
    life.signal,
    (_key, error) => {
      failure = error;
    },
    { maxViews: 3, maxParked: 1, ttlMs: 100 },
    () => now,
  );
  const factory = (element: HTMLElement, controllerSignal: AbortSignal) => {
    created++;
    element.appendChild(root);
    return {
      setVisible: (visible: boolean) => {
        if (controllerSignal.aborted) throw new Error("pool.controller_closed");
        shown.push(visible);
      },
      dispose: () => {
        disposed++;
        if (root.parentElement === element && host.isConnected)
          host.appendChild(root);
      },
    };
  };
  try {
    const one = pool.mount("terminal", host, factory),
      two = pool.mount("terminal", slot, factory);
    if (
      one.release() ||
      !two.isCurrent() ||
      two.element !== one.element ||
      !slot.contains(root)
    )
      throw new Error("pool.stale_mount");
    if (
      !two.release() ||
      two.element.parentElement !== parking ||
      !parking.hidden ||
      !parking.inert ||
      disposed !== 0
    )
      throw new Error("pool.park_failed");
    const three = pool.mount("terminal", host, factory);
    if (
      three.element !== one.element ||
      !host.contains(root) ||
      created !== 1 ||
      JSON.stringify(shown) !== "[true,false,true]"
    )
      throw new Error("pool.reuse_failed");
    pool
      .mount("old", slot, () => ({
        setVisible: () => {},
        dispose: () => {
          oldDropped++;
        },
      }))
      .release();
    pool
      .mount("recent", slot, () => ({
        setVisible: () => {},
        dispose: () => {
          recentDropped++;
        },
      }))
      .release();
    if (oldDropped !== 1 || recentDropped !== 0 || !three.isCurrent())
      throw new Error("pool.lru_failed");
    now = 100;
    pool.sweep();
    if (counts().recentDropped !== 1 || !three.isCurrent() || pool.size !== 1)
      throw new Error("pool.ttl_failed");
    life.abort();
    if (
      counts().disposed !== 1 ||
      three.isCurrent() ||
      !host.contains(root) ||
      failure
    )
      throw new Error("pool.dispose_failed");
  } finally {
    pool.dispose();
    parking.remove();
    slot.remove();
  }
}
