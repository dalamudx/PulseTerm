/** Controlled target snapshot: read-only fixed colors, draw/readback, then complete cleanup. */
import { THEME_UI_PROPERTIES } from "../features/theme/paint-data";
import { commands } from "./gen/commands";
import type { TransferViewProbe } from "./transfer-view-probe";

type ThemeHook = (
  run: string,
  generation: string,
  claim: string,
) => Promise<void>;
export type TargetThemeWindow = Pick<
  Window,
  "addEventListener" | "removeEventListener"
> & { __PULSE_TARGET_THEME__?: ThemeHook };
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
export function installTargetThemeBridge(
  window: TargetThemeWindow,
  run: string,
  host: HTMLElement,
  signal: AbortSignal,
  view: Pick<
    TransferViewProbe,
    "paintTheme" | "revokeTheme" | "dispose" | "verifyCapturedPalette"
  >,
  releaseHost?: () => void,
): () => void {
  let closed = signal.aborted,
    busy = false;
  let released = false;
  const release = () => {
    if (!released) {
      released = true;
      releaseHost?.();
    }
  };
  const values = () =>
    Object.values(THEME_UI_PROPERTIES).map((property) =>
      getComputedStyle(host).getPropertyValue(property).trim(),
    );
  const paint: ThemeHook = async (nonce, generation, claim) => {
    if (
      closed ||
      signal.aborted ||
      busy ||
      nonce !== run ||
      !UUID.test(claim) ||
      !/^[1-9][0-9]{0,19}$/.test(generation)
    )
      throw new Error("theme.target.forbidden");
    busy = true;
    try {
      const baseline = values();
      await view.verifyCapturedPalette(generation);
      if (closed || signal.aborted) throw new Error("theme.target.retired");
      await view.paintTheme(generation, async (reading) => {
        if (closed || reading.aborted || signal.aborted)
          throw new Error("theme.target.retired");
        const r = await commands.probeTargetThemeRead(run, generation, claim);
        if (
          r.generation !== generation ||
          r.ticket !== claim ||
          closed ||
          reading.aborted ||
          signal.aborted
        )
          throw new Error("theme.target.receipt");
        await commands.probeTargetThemeConfirm(run, generation, claim);
        if (closed || reading.aborted || signal.aborted)
          throw new Error("theme.target.retired");
        return r.paintJson;
      });
      if (closed || signal.aborted) throw new Error("theme.target.retired");
      await commands.probeTargetThemeWaitRevoked(run, generation, claim);
      if (closed || signal.aborted) throw new Error("theme.target.retired");
      if (!view.revokeTheme(generation))
        throw new Error("theme.target.revoke_failed");
      // Notification must clear the actual live surface, not just dispose the whole view.
      if (values().some((value, i) => value !== baseline[i]))
        throw new Error("theme.target.cleanup");
      view.dispose();
      if (values().some((value, i) => value !== baseline[i]))
        throw new Error("theme.target.cleanup");
      release();
      await commands.probeTargetThemeDrawn(run, generation, claim);
      if (closed || signal.aborted) throw new Error("theme.target.retired");
    } catch (error) {
      const stage =
        new Map([
          ["terminal.palette.override_mismatch", 1],
          ["terminal.palette.reset_mismatch", 2],
          ["terminal.palette.ris_mismatch", 3],
          ["terminal.palette.view_retired", 4],
          ["terminal.palette.query_leak", 5],
        ]).get(error instanceof Error ? error.message : "") ?? 0;
      if (!closed && !signal.aborted) {
        try {
          await commands.probeTargetThemeFailed(run, generation, claim, stage);
        } catch {
          /* Already retired: cannot wake a replacement's challenge. */
        }
      }
      throw error;
    } finally {
      try {
        view.dispose();
      } finally {
        try {
          release();
        } finally {
          busy = false;
        }
      }
    }
  };
  const stop = () => {
    closed = true;
    if (window.__PULSE_TARGET_THEME__ === paint)
      delete window.__PULSE_TARGET_THEME__;
    try {
      view.dispose();
    } finally {
      try {
        release();
      } finally {
        signal.removeEventListener("abort", stop);
        window.removeEventListener("pagehide", stop);
      }
    }
  };
  if (!closed) {
    window.__PULSE_TARGET_THEME__ = paint;
    signal.addEventListener("abort", stop, { once: true });
    window.addEventListener("pagehide", stop, { once: true });
  }
  return stop;
}
