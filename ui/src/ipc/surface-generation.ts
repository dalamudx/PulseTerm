// Native-authenticated generations, not authorization tokens. Only a requested
// resize/resume may replace a current surface. Stale frames must not signal activity.
export class SurfaceGeneration {
  private current: bigint | undefined;
  private pending = false;
  expectReplacement(current: bigint): void {
    if (this.current !== current || this.pending)
      throw new Error("surface.replacement_busy_or_stale");
    this.pending = true;
  }
  accept(
    generation: bigint,
    begin: boolean,
    sequence: number,
  ): "initial" | "same" | "stale" | "replacement" {
    if (generation <= 0n || generation > (1n << 64n) - 1n)
      throw new Error("surface.generation");
    if (this.current === undefined) {
      if (!begin || sequence !== 0) throw new Error("surface.no_begin");
      this.current = generation;
      return "initial";
    }
    if (generation < this.current) return "stale";
    if (generation === this.current) {
      if (begin) throw new Error("surface.duplicate_begin");
      return "same";
    }
    if (!begin || sequence !== 0 || !this.pending)
      throw new Error("surface.unrequested_generation");
    this.current = generation;
    this.pending = false;
    return "replacement";
  }
}
/** A cancelled old renderer may resolve/reject after replacement. Neither may
 * update idle state or detach the CURRENT binding. Check again after callback errors. */
export function forCurrentSurface<T>(
  operation: Promise<T>,
  current: () => boolean,
  value: (receipt: T) => Promise<void>,
  error: (cause: unknown) => void,
): Promise<void> {
  return operation
    .then((receipt) => (current() ? value(receipt) : undefined))
    .catch((cause) => {
      if (current()) error(cause);
    });
}
