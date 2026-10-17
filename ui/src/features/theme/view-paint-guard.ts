import type { ViewResource, ViewScope } from "../terminal/view-host";
import { parseThemePaintData, type ThemePaintData } from "./paint-data";
/** Trusted renderer resource: prepare has no visible effects; apply is synchronous,
 * dispose is idempotent and removes even a partially-applied surface. No theme code runs.
 */
export interface ThemePaintSurface extends ViewResource {
  apply(): void;
}
type Pending = { epoch: object; abort: AbortController };
/** Local lifecycle only, NOT backend authorization. Reader must use an authorized
 * page/generation/source receipt, and forward source invalidation to invalidate().
 * Generation comes exclusively from the existing ordered-Begin ViewScope.
 */
export class ViewThemePaintGuard implements ViewResource {
  private epoch: object = {};
  private publishedEpoch: object | undefined;
  private pending: Pending | undefined;
  private closed = false;
  private cleanupBroken = false;
  private lastGood: ThemePaintData | undefined;
  private readonly surfaces = new Set<ThemePaintSurface>();
  private readonly disposed = new WeakSet<ThemePaintSurface>();
  constructor(private readonly view: ViewScope) {
    view.own(this);
  }
  /** Revoke old requests even on A→B→A. Keep last good pixels marked stale, not current.
   * Aborting the reader does not free its local slot until its promise actually exits.
   */
  invalidate(): void {
    if (this.closed) return;
    this.epoch = {};
    this.pending?.abort.abort();
  }
  /** Authority retirement: clear pixels without closing the healthy view or freeing a live read.
   * Unlike invalidate (stale-last-good policy), this removes every old drawing resource.
   */
  revoke(): void {
    if (this.closed) return;
    this.invalidate();
    this.publishedEpoch = undefined;
    for (const surface of [...this.surfaces]) {
      try {
        this.release(surface);
      } catch {
        /* Attempt all resources before failing closed. */
      }
    }
    if (this.cleanupBroken) this.cleanupFailure();
  }
  snapshot(): Readonly<{ data: ThemePaintData | undefined; current: boolean }> {
    return Object.freeze({
      data: this.lastGood,
      current:
        !this.closed &&
        this.view.isCurrent() &&
        this.publishedEpoch === this.epoch,
    });
  }
  private current(job: Pending): boolean {
    return (
      !this.closed &&
      this.view.isCurrent() &&
      job.epoch === this.epoch &&
      !job.abort.signal.aborted
    );
  }
  private release(surface: ThemePaintSurface): void {
    if (this.disposed.has(surface)) return;
    this.disposed.add(surface);
    this.surfaces.delete(surface);
    try {
      surface.dispose();
    } catch {
      this.cleanupBroken = true;
      throw new Error("theme.paint.cleanup_failed");
    }
  }
  private cleanupFailure(): never {
    this.view.release();
    throw new Error("theme.paint.cleanup_failed");
  }
  async refresh(
    reader: (signal: AbortSignal) => Promise<unknown>,
    prepare: (data: ThemePaintData) => ThemePaintSurface,
  ): Promise<"applied" | "stale"> {
    if (this.closed || !this.view.isCurrent())
      throw new Error("theme.paint.view_retired");
    if (this.pending) throw new Error("theme.paint.busy");
    const job: Pending = { epoch: this.epoch, abort: new AbortController() };
    this.pending = job;
    const abort = () => job.abort.abort();
    this.view.signal.addEventListener("abort", abort, { once: true });
    let next: ThemePaintSurface | undefined;
    let adopted = false;
    try {
      let encoded: unknown;
      try {
        encoded = await reader(job.abort.signal);
      } catch {
        if (!this.current(job)) return "stale";
        throw new Error("theme.paint.read_failed");
      }
      if (!this.current(job)) return "stale";
      const data = parseThemePaintData(encoded);
      try {
        next = prepare(data);
      } catch {
        if (!this.current(job)) return "stale";
        throw new Error("theme.paint.prepare_failed");
      }
      if (this.surfaces.has(next) || this.disposed.has(next))
        throw new Error("theme.paint.resource_reused");
      this.surfaces.add(next);
      adopted = true;
      if (!this.current(job)) {
        this.release(next);
        return "stale";
      }
      try {
        next.apply();
      } catch {
        this.release(next);
        if (!this.current(job)) return "stale";
        throw new Error("theme.paint.apply_failed");
      }
      if (!this.current(job)) {
        this.release(next);
        return "stale";
      }
      // Only publish after applying and removing the old resource; cleanup failure closes
      // this view, never the session or a healthy replacement generation.
      for (const old of this.surfaces) {
        if (old !== next) this.release(old);
      }
      if (!this.current(job)) {
        this.release(next);
        return "stale";
      }
      this.lastGood = data;
      this.publishedEpoch = job.epoch;
      return "applied";
    } catch (error) {
      if (this.cleanupBroken) return this.cleanupFailure();
      if (adopted && next && this.surfaces.has(next)) {
        try {
          this.release(next);
        } catch {
          return this.cleanupFailure();
        }
      }
      throw error;
    } finally {
      this.view.signal.removeEventListener("abort", abort);
      if (this.pending === job) this.pending = undefined;
    }
  }
  dispose(): void {
    if (!this.closed) {
      this.closed = true;
      this.epoch = {};
      this.pending?.abort.abort();
      this.lastGood = undefined;
      this.publishedEpoch = undefined;
    }
    for (const surface of [...this.surfaces]) {
      try {
        this.release(surface);
      } catch {
        /* Continue every remaining cleanup. */
      }
    }
    if (this.cleanupBroken) throw new Error("theme.paint.cleanup_failed");
  }
}
