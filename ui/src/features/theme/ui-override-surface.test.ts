import { expect, it, vi } from "vitest";
import { GenerationViewHost } from "../terminal/view-host";
import {
  parseThemePaintData,
  rgbToHex,
  THEME_UI_PROPERTIES,
  type ThemePaintData,
} from "./paint-data";
import { resolveUiTheme } from "./radix-theme";
import { UiThemeOverrideTarget } from "./ui-override-surface";
import { ViewThemePaintGuard } from "./view-paint-guard";

it("resolves full fixed stock UI on the same nonce surface and clears all 28 slots", async () => {
  const f = fixture(),
    element = f.element(),
    target = new UiThemeOverrideTarget(f.scope, element),
    guard = new ViewThemePaintGuard(f.scope);
  const source = parseThemePaintData(encoded()),
    generated = resolveUiTheme(source, { contrast: "reject" });
  expect(
    await guard.refresh(
      async () => encoded(),
      (data) =>
        target.prepare(resolveUiTheme(data, { contrast: "reject" }).paint),
    ),
  ).toBe("applied");
  expect(f.sheets).toHaveLength(1);
  for (const v of generated.paint.overrides)
    expect(f.computed(element, THEME_UI_PROPERTIES[v.slot])).toBe(
      rgbToHex(v.color),
    );
  expect(guard.snapshot().data?.terminal).toEqual(source.terminal);
  guard.revoke();
  expect(f.sheets).toHaveLength(0);
  f.host.dispose();
});
it("low-contrast and unsupported seed fail before allocation, preserving only stale last-good", async () => {
  const f = fixture(),
    element = f.element(),
    target = new UiThemeOverrideTarget(f.scope, element),
    guard = new ViewThemePaintGuard(f.scope);
  const prepare = (data: ThemePaintData) =>
    target.prepare(resolveUiTheme(data, { contrast: "reject" }).paint);
  await guard.refresh(async () => encoded(), prepare);
  const original = f.sheets[0];
  guard.invalidate();
  const source = JSON.parse(encoded());
  source.overrides = [
    { slot: "fg", color: [255, 255, 255] },
    { slot: "bgBase", color: [255, 255, 255] },
  ];
  await expect(
    guard.refresh(async () => JSON.stringify(source), prepare),
  ).rejects.toThrow("theme.paint.prepare_failed");
  source.overrides = [];
  source.accent = "#123456";
  await expect(
    guard.refresh(async () => JSON.stringify(source), prepare),
  ).rejects.toThrow("theme.paint.prepare_failed");
  expect(f.sheets).toEqual([original]);
  expect(guard.snapshot().current).toBe(false);
  guard.revoke();
  expect(f.sheets).toHaveLength(0);
  f.host.dispose();
});
/** Only a DOM ownership/nonce/cascade stub; NOT browser CSS or CSP acceptance. */
function fixture() {
  const sheets: Style[] = [];
  let nonce = "fixture-page-nonce",
    allow = true;
  class Style {
    nonce = "";
    textContent = "";
    sheet: object | null = null;
    remove() {
      const index = sheets.indexOf(this);
      if (index >= 0) sheets.splice(index, 1);
    }
    setAttribute(k: string, v: string) {
      if (k !== "nonce") throw new Error("unknown attribute");
      this.nonce = v;
    }
  }
  const document = {
    querySelector: () => ({ nonce }),
    createElement: () => new Style(),
    head: {
      append: (s: Style) => {
        sheets.push(s);
        s.sheet = allow && s.nonce === nonce ? {} : null;
      },
    },
    defaultView: {
      getComputedStyle: (element: Element) => ({
        getPropertyValue: (key: string) => {
          let value = "baseline";
          for (const s of sheets) {
            const id = s.textContent.match(/="([^"]+)"/)?.[1];
            if (
              !s.sheet ||
              id !== element.getAttribute("data-pulse-theme-view")
            )
              continue;
            const body = s.textContent.match(/\{([^}]+)\}/)?.[1] ?? "";
            for (const pair of body.split(";")) {
              const [k, v] = pair.split(":");
              if (k === key && v) value = v;
            }
          }
          return value;
        },
      }),
    },
  };
  function element() {
    const attrs = new Map<string, string>();
    return {
      ownerDocument: document,
      isConnected: true,
      hasAttribute: (k: string) => attrs.has(k),
      getAttribute: (k: string) => attrs.get(k) ?? null,
      setAttribute: (k: string, v: string) => attrs.set(k, v),
      removeAttribute: (k: string) => attrs.delete(k),
    } as unknown as HTMLElement;
  }
  const host = new GenerationViewHost(new AbortController().signal, vi.fn()),
    scope = host.begin(1n);
  return {
    sheets,
    element,
    host,
    scope,
    computed: (e: HTMLElement, k: string) =>
      document.defaultView.getComputedStyle(e).getPropertyValue(k),
    nonce: (v: string) => {
      nonce = v;
    },
    block: () => {
      allow = false;
    },
    append: (f: (s: Style) => void) => {
      document.head.append = f;
    },
  };
}
function encoded(value = 17) {
  return JSON.stringify({
    version: 1,
    appearance: "dark",
    neutral: "slate",
    accent: "cyan",
    terminal: {
      foreground: [255, 255, 255],
      background: [0, 0, 0],
      ansi: Array.from({ length: 16 }, () => [0, 0, 0]),
    },
    overrides: [
      { slot: "bgBase", color: [value, 34, 51] },
      { slot: "fg", color: [241, 245, 249] },
    ],
    lowForegroundContrast: false,
  });
}
it("prepares inert scoped UI only, borrows existing page nonce and cleans with view", () => {
  const f = fixture(),
    element = f.element(),
    target = new UiThemeOverrideTarget(f.scope, element),
    image = parseThemePaintData(encoded()),
    s = target.prepare(image);
  expect(f.sheets).toHaveLength(0);
  s.apply();
  expect(f.sheets).toHaveLength(1);
  expect(f.sheets[0]?.nonce).toBe("fixture-page-nonce");
  expect(f.computed(element, "--bg-base")).toBe("#112233");
  expect(f.computed(element, "--fg")).toBe("#f1f5f9");
  f.host.dispose();
  expect(f.sheets).toHaveLength(0);
  expect(element.hasAttribute("data-pulse-theme-view")).toBe(false);
  s.dispose();
});
it("missing or unresolved nonce cannot claim target or allocate sheets", () => {
  const f = fixture(),
    element = f.element();
  for (const n of ["", "__TAURI_NONCE__"]) {
    f.nonce(n);
    expect(() => new UiThemeOverrideTarget(f.scope, element)).toThrow(
      "nonce_missing",
    );
    expect(element.hasAttribute("data-pulse-theme-view")).toBe(false);
  }
  f.host.dispose();
});
it("isolates sibling roots and rejects double target ownership", () => {
  const f = fixture(),
    a = f.element(),
    b = f.element(),
    ta = new UiThemeOverrideTarget(f.scope, a),
    tb = new UiThemeOverrideTarget(f.scope, b);
  ta.prepare(parseThemePaintData(encoded())).apply();
  tb.prepare(parseThemePaintData(encoded(68))).apply();
  expect(f.computed(a, "--bg-base")).toBe("#112233");
  expect(f.computed(b, "--bg-base")).toBe("#442233");
  expect(() => new UiThemeOverrideTarget(f.scope, a)).toThrow("target_owned");
  f.host.dispose();
});
it("replacement via guard keeps only one live sheet and removes prior once", async () => {
  const f = fixture(),
    element = f.element(),
    target = new UiThemeOverrideTarget(f.scope, element),
    guard = new ViewThemePaintGuard(f.scope);
  await guard.refresh(
    async () => encoded(),
    (data) => target.prepare(data),
  );
  guard.invalidate();
  await guard.refresh(
    async () => encoded(68),
    (data) => target.prepare(data),
  );
  expect(f.sheets).toHaveLength(1);
  expect(f.computed(element, "--bg-base")).toBe("#442233");
  f.host.dispose();
  expect(f.sheets).toHaveLength(0);
});
it("retired, detached or tampered selector cannot apply a tentative surface", () => {
  for (const type of ["retire", "detach", "tamper"]) {
    const f = fixture(),
      element = f.element(),
      target = new UiThemeOverrideTarget(f.scope, element),
      surface = target.prepare(parseThemePaintData(encoded()));
    if (type === "retire") f.host.retire(1n);
    if (type === "detach")
      Object.defineProperty(element, "isConnected", { value: false });
    if (type === "tamper")
      element.setAttribute("data-pulse-theme-view", "foreign");
    expect(() => surface.apply()).toThrow("surface_retired");
    surface.dispose();
    f.host.dispose();
    expect(f.sheets).toHaveLength(0);
    if (type === "tamper")
      expect(element.getAttribute("data-pulse-theme-view")).toBe("foreign");
  }
});
it("blocked sheet cannot publish fake applied image or leak tentative style", async () => {
  const f = fixture(),
    element = f.element(),
    target = new UiThemeOverrideTarget(f.scope, element),
    g = new ViewThemePaintGuard(f.scope);
  f.block();
  await expect(
    g.refresh(
      async () => encoded(),
      (data) => target.prepare(data),
    ),
  ).rejects.toThrow("apply_failed");
  expect(g.snapshot().data).toBeUndefined();
  expect(f.sheets).toHaveLength(0);
  expect(f.scope.isCurrent()).toBe(true);
  f.host.dispose();
});
it("caps old plus tentative resources and prevents reuse", () => {
  const f = fixture(),
    target = new UiThemeOverrideTarget(f.scope, f.element()),
    data = parseThemePaintData(encoded()),
    a = target.prepare(data),
    b = target.prepare(data);
  expect(() => target.prepare(data)).toThrow("surface_busy");
  a.apply();
  expect(() => a.apply()).toThrow("surface_reused");
  a.dispose();
  const c = target.prepare(data);
  b.dispose();
  c.dispose();
  f.host.dispose();
});
it("rejects unknown CSS properties even through an unsafe caller cast", () => {
  const f = fixture(),
    target = new UiThemeOverrideTarget(f.scope, f.element()),
    data = parseThemePaintData(encoded());
  const forged = {
    ...data,
    overrides: [{ slot: "backgroundImage", color: [17, 34, 51] }],
  };
  expect(() => target.prepare(forged as unknown as typeof data)).toThrow(
    "theme.paint.invalid",
  );
  expect(f.sheets).toHaveLength(0);
  f.host.dispose();
});
it("all 28 fixed variables render without terminal options or style attributes", () => {
  const f = fixture(),
    element = f.element(),
    target = new UiThemeOverrideTarget(f.scope, element),
    raw = JSON.parse(encoded());
  raw.overrides = Object.keys(THEME_UI_PROPERTIES).map((slot) => ({
    slot,
    color: [17, 34, 51],
  }));
  const data = parseThemePaintData(JSON.stringify(raw));
  target.prepare(data).apply();
  for (const key of Object.values(THEME_UI_PROPERTIES))
    expect(f.computed(element, key)).toBe("#112233");
  expect(element.hasAttribute("style")).toBe(false);
  f.host.dispose();
});
it("selector cleanup failure cannot skip stylesheet cleanup or leak raw failures", () => {
  const f = fixture(),
    element = f.element(),
    target = new UiThemeOverrideTarget(f.scope, element),
    a = target.prepare(parseThemePaintData(encoded())),
    b = target.prepare(parseThemePaintData(encoded(68)));
  a.apply();
  b.apply();
  vi.spyOn(element, "removeAttribute").mockImplementation(() => {
    throw new Error("PRIVATE");
  });
  expect(() => target.dispose()).toThrow("theme.paint.cleanup_failed");
  expect(f.sheets).toHaveLength(0);
  a.dispose();
  b.dispose();
  f.host.dispose();
});
