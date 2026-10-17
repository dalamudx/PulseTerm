import type { ViewScope } from "../terminal/view-host";
import { THEME_UI_PROPERTIES } from "../theme/paint-data";
import { UiThemeOverrideTarget } from "../theme/ui-override-surface";
import { ViewThemePaintGuard } from "../theme/view-paint-guard";
/** Actual WebView/ordered-Begin scope, but fixed synthetic UI colors only: NOT a
 * backend-source receipt, terminal palette update, pixel/performance or full GUI-theme test.
 */
export async function verifyUiThemeOverlay(
  view: ViewScope,
  host: HTMLElement,
): Promise<void> {
  const document = host.ownerDocument,
    window = document.defaultView;
  if (!window) throw new Error("theme.overlay.document");
  const sibling = document.createElement("div");
  host.parentElement?.append(sibling);
  const original = new Map(
    Object.values(THEME_UI_PROPERTIES).map((key) => [
      key,
      window.getComputedStyle(host).getPropertyValue(key).trim(),
    ]),
  );
  const siblingBefore = new Map(
    Object.values(THEME_UI_PROPERTIES).map((key) => [
      key,
      window.getComputedStyle(sibling).getPropertyValue(key).trim(),
    ]),
  );
  const target = new UiThemeOverrideTarget(view, host),
    guard = new ViewThemePaintGuard(view);
  const id = host.getAttribute("data-pulse-theme-view"),
    selector = `[data-pulse-theme-view="${id}"]`;
  const ownSheets = () =>
    [...document.querySelectorAll("style")].filter((s) =>
      s.textContent?.startsWith(selector),
    );
  const encoded = (beta = false) =>
    JSON.stringify({
      version: 1,
      appearance: "dark",
      neutral: "slate",
      accent: "cyan",
      terminal: {
        foreground: [241, 245, 249],
        background: [15, 23, 42],
        ansi: Array.from({ length: 16 }, () => [0, 0, 0]),
      },
      overrides: Object.keys(THEME_UI_PROPERTIES).map((slot) => ({
        slot,
        color:
          slot === "fg" ||
          slot.startsWith("fg") ||
          slot.endsWith("Fg") ||
          slot === "accentContrast"
            ? [241, 245, 249]
            : beta
              ? [71, 85, 105]
              : [15, 23, 42],
      })),
      lowForegroundContrast: false,
    });
  try {
    if (
      (await guard.refresh(
        async () => encoded(),
        (data) => target.prepare(data),
      )) !== "applied"
    )
      throw new Error("theme.overlay.initial");
    if (ownSheets().length !== 1 || !guard.snapshot().current)
      throw new Error("theme.overlay.ownership");
    let resolve: ((v: unknown) => void) | undefined;
    const pending = new Promise<unknown>((ok) => {
      resolve = ok;
    });
    let aborted: AbortSignal | undefined;
    const late = guard.refresh(
      (signal) => {
        aborted = signal;
        return pending;
      },
      (data) => target.prepare(data),
    );
    guard.invalidate();
    guard.invalidate();
    let busy = false;
    try {
      await guard.refresh(
        async () => encoded(true),
        (data) => target.prepare(data),
      );
    } catch (error) {
      busy = error instanceof Error && error.message === "theme.paint.busy";
    }
    if (!busy || !aborted?.aborted || !resolve)
      throw new Error("theme.overlay.cancel");
    resolve(encoded(true));
    if (
      (await late) !== "stale" ||
      ownSheets().length !== 1 ||
      guard.snapshot().current
    )
      throw new Error("theme.overlay.late");
    if (
      (await guard.refresh(
        async () => encoded(true),
        (data) => target.prepare(data),
      )) !== "applied"
    )
      throw new Error("theme.overlay.replace");
    if (
      ownSheets().length !== 1 ||
      window.getComputedStyle(host).getPropertyValue("--bg-base").trim() !==
        "#475569"
    )
      throw new Error("theme.overlay.readback");
    for (const [key, value] of siblingBefore) {
      if (
        window.getComputedStyle(sibling).getPropertyValue(key).trim() !== value
      )
        throw new Error("theme.overlay.sibling");
    }
    const good = guard.snapshot().data;
    guard.invalidate();
    let invalid = false;
    try {
      await guard.refresh(
        async () => "PRIVATE malformed",
        (data) => target.prepare(data),
      );
    } catch (error) {
      invalid =
        error instanceof Error && error.message === "theme.paint.invalid";
    }
    if (
      !invalid ||
      guard.snapshot().data !== good ||
      guard.snapshot().current ||
      ownSheets().length !== 1
    )
      throw new Error("theme.overlay.invalid");
  } finally {
    guard.dispose();
    target.dispose();
    sibling.remove();
  }
  if (
    !view.isCurrent() ||
    host.hasAttribute("data-pulse-theme-view") ||
    ownSheets().length !== 0
  )
    throw new Error("theme.overlay.cleanup");
  for (const [key, value] of original) {
    if (window.getComputedStyle(host).getPropertyValue(key).trim() !== value)
      throw new Error("theme.overlay.restore");
  }
}
