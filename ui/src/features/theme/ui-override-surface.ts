import { nonceDocument } from "../terminal/nonce-document";
import type { ViewResource, ViewScope } from "../terminal/view-host";
import {
  parseThemePaintData,
  rgbToHex,
  THEME_UI_PROPERTIES,
  type ThemePaintData,
} from "./paint-data";
import type { ThemePaintSurface } from "./view-paint-guard";

const ATTRIBUTE = "data-pulse-theme-view";
const owners = new WeakMap<HTMLElement, UiThemeOverrideTarget>();
let namespace = 0n;
/** Fixed UI overrides only. No terminal theme, Radix scale resolution, CSS expressions,
 * HTML, URLs or backend authority. Own this target and its guard with the same ViewScope.
 */
export class UiThemeOverrideTarget implements ViewResource {
  private readonly document: Document;
  private readonly id: string;
  private readonly styles = new Set<HTMLStyleElement>();
  private closed = false;
  constructor(
    private readonly view: ViewScope,
    private readonly target: HTMLElement,
  ) {
    if (!view.isCurrent() || !target.isConnected)
      throw new Error("theme.paint.target_retired");
    if (owners.has(target) || target.hasAttribute(ATTRIBUTE))
      throw new Error("theme.paint.target_owned");
    this.document = nonceDocument(target.ownerDocument);
    if (namespace === (1n << 64n) - 1n)
      throw new Error("theme.paint.namespace_exhausted");
    // CSS-only namespace, never a backend binding generation or theme authorization ID.
    this.id = `pt-theme-${++namespace}`;
    owners.set(target, this);
    try {
      target.setAttribute(ATTRIBUTE, this.id);
      view.own(this);
    } catch {
      this.dispose();
      throw new Error("theme.paint.target_retired");
    }
  }
  private current(): boolean {
    return (
      !this.closed &&
      this.view.isCurrent() &&
      owners.get(this.target) === this &&
      this.target.getAttribute(ATTRIBUTE) === this.id &&
      this.target.isConnected
    );
  }
  /** Inert allocation until guard calls apply; at most old + tentative stylesheet. */
  prepare(image: ThemePaintData): ThemePaintSurface {
    if (!this.current()) throw new Error("theme.paint.target_retired");
    if (this.styles.size >= 2) throw new Error("theme.paint.surface_busy");
    // Defensive runtime validation for callers outside ViewThemePaintGuard; never trust a TS cast.
    const data = parseThemePaintData(JSON.stringify(image));
    const values = data.overrides.map(
      ({ slot, color }) =>
        [THEME_UI_PROPERTIES[slot], rgbToHex(color)] as const,
    );
    const style = this.document.createElement("style");
    style.textContent = `[${ATTRIBUTE}="${this.id}"]{${values.map(([key, value]) => `${key}:${value};`).join("")}}`;
    this.styles.add(style);
    let disposed = false,
      applied = false;
    const dispose = () => {
      if (disposed) return;
      disposed = true;
      this.styles.delete(style);
      style.remove();
    };
    return {
      dispose,
      apply: () => {
        if (disposed || !this.current())
          throw new Error("theme.paint.surface_retired");
        if (applied) throw new Error("theme.paint.surface_reused");
        applied = true;
        try {
          this.document.head.append(style);
          if (disposed || !this.current())
            throw new Error("theme.paint.surface_retired");
          if (!style.sheet) throw new Error("theme.paint.styles_not_applied");
          const window = this.target.ownerDocument.defaultView;
          if (!window) throw new Error("theme.paint.styles_not_applied");
          const computed = window.getComputedStyle(this.target);
          for (const [key, value] of values) {
            if (computed.getPropertyValue(key).trim() !== value)
              throw new Error("theme.paint.styles_not_applied");
          }
        } catch {
          dispose();
          throw new Error("theme.paint.styles_not_applied");
        }
      },
    };
  }
  dispose(): void {
    if (this.closed) return;
    this.closed = true;
    // Revoke selector before cleanup; a throwing removal must not skip the other sheet.
    if (owners.get(this.target) === this) owners.delete(this.target);
    let failed = false;
    try {
      if (this.target.getAttribute(ATTRIBUTE) === this.id)
        this.target.removeAttribute(ATTRIBUTE);
    } catch {
      failed = true;
    }
    for (const style of this.styles) {
      try {
        style.remove();
      } catch {
        failed = true;
      }
    }
    this.styles.clear();
    if (failed) throw new Error("theme.paint.cleanup_failed");
  }
}
