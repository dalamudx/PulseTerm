import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  installTargetThemeBridge,
  type TargetThemeWindow,
} from "./target-theme-probe";

const mock = vi.hoisted(() => ({
  read: vi.fn(),
  confirm: vi.fn(),
  drawn: vi.fn(),
  wait: vi.fn(),
  failed: vi.fn(),
}));
vi.mock("./gen/commands", () => ({
  commands: {
    probeTargetThemeRead: mock.read,
    probeTargetThemeConfirm: mock.confirm,
    probeTargetThemeDrawn: mock.drawn,
    probeTargetThemeWaitRevoked: mock.wait,
    probeTargetThemeFailed: mock.failed,
  },
}));
it("page retirement while awaiting revocation cannot revoke or draw a replacement", async () => {
  const f = setup();
  mock.wait.mockImplementationOnce(async () => {
    f.window.dispatchEvent(new Event("pagehide"));
  });
  await expect(f.hook("run", "17", claim)).rejects.toThrow("retired");
  expect(f.view.revokeTheme).not.toHaveBeenCalled();
  expect(mock.drawn).not.toHaveBeenCalled();
  expect(color).toBe("");
});
it("failed revocation observation cannot become a cleanup receipt", async () => {
  const f = setup();
  mock.wait.mockRejectedValueOnce(new Error("theme.delivery.retired"));
  await expect(f.hook("run", "17", claim)).rejects.toThrow("retired");
  expect(f.view.revokeTheme).not.toHaveBeenCalled();
  expect(mock.drawn).not.toHaveBeenCalled();
  expect(color).toBe("");
  f.stop();
});
it("a stale target generation cannot acknowledge theme cleanup", async () => {
  const f = setup();
  f.view.revokeTheme.mockReturnValueOnce(false);
  await expect(f.hook("run", "17", claim)).rejects.toThrow("revoke_failed");
  expect(mock.drawn).not.toHaveBeenCalled();
  expect(color).toBe("");
  f.stop();
});
it("a palette mismatch cannot produce UI paint or cleanup receipts", async () => {
  const f = setup();
  f.view.verifyCapturedPalette.mockRejectedValueOnce(
    new Error("terminal.palette.render_mismatch"),
  );
  await expect(f.hook("run", "17", claim)).rejects.toThrow("render_mismatch");
  expect(mock.read).not.toHaveBeenCalled();
  expect(mock.drawn).not.toHaveBeenCalled();
  expect(f.view.paintTheme).not.toHaveBeenCalled();
  f.stop();
});
it("retirement during palette diagnostics cannot start UI paint", async () => {
  const f = setup();
  f.view.verifyCapturedPalette.mockImplementationOnce(async () => {
    f.window.dispatchEvent(new Event("pagehide"));
  });
  await expect(f.hook("run", "17", claim)).rejects.toThrow("retired");
  expect(mock.read).not.toHaveBeenCalled();
  expect(mock.drawn).not.toHaveBeenCalled();
  f.stop();
});
const claim = "00000000-0000-0000-0000-000000000001";
let color = "",
  order: string[] = [];
beforeEach(() => {
  vi.clearAllMocks();
  color = "";
  order = [];
  vi.stubGlobal("getComputedStyle", () => ({
    getPropertyValue: (p: string) => (p === "--bg-base" ? color : ""),
  }));
  mock.read.mockImplementation(async () => {
    order.push("read");
    return { ticket: claim, generation: "17", paintJson: "fixed" };
  });
  mock.confirm.mockImplementation(async () => {
    order.push("confirm");
  });
  mock.wait.mockImplementation(async () => {
    order.push("wait");
  });
  mock.drawn.mockImplementation(async () => {
    order.push("drawn");
  });
});
afterEach(() => vi.unstubAllGlobals());
function setup() {
  const window = new EventTarget() as EventTarget & TargetThemeWindow;
  const life = new AbortController();
  const view = {
    verifyCapturedPalette: vi.fn(async () => {}),
    paintTheme: vi.fn(
      async (_g: string, reader: (signal: AbortSignal) => Promise<unknown>) => {
        expect(await reader(life.signal)).toBe("fixed");
        order.push("paint");
        color = "#112233";
      },
    ),
    revokeTheme: vi.fn(() => {
      order.push("revoke");
      color = "";
      return true;
    }),
    dispose: vi.fn(() => {
      color = "";
    }),
  };
  const release = vi.fn(() => {
    order.push("release");
  });
  const stop = installTargetThemeBridge(
    window,
    "run",
    {} as HTMLElement,
    life.signal,
    view,
    release,
  );
  const hook = window.__PULSE_TARGET_THEME__;
  if (!hook) throw new Error("test hook missing");
  return { window, life, view, stop, hook, release };
}
it("fixed target read-confirm-paint-cleanup-drawn order is mandatory", async () => {
  const f = setup();
  await f.hook("run", "17", claim);
  expect(order).toEqual([
    "read",
    "confirm",
    "paint",
    "wait",
    "revoke",
    "release",
    "drawn",
  ]);
  expect(color).toBe("");
  expect(f.view.dispose).toHaveBeenCalled();
  f.stop();
  expect(f.release).toHaveBeenCalledTimes(1);
  expect(f.window.__PULSE_TARGET_THEME__).toBeUndefined();
  await expect(f.hook("run", "17", claim)).rejects.toThrow();
});
it("host cleanup failure rejects drawn but still clears busy ownership and hook", async () => {
  const f = setup();
  mock.read.mockRejectedValueOnce(new Error("read failure"));
  f.release.mockImplementationOnce(() => {
    throw new Error("host cleanup failure");
  });
  await expect(f.hook("run", "17", claim)).rejects.toThrow(
    "host cleanup failure",
  );
  expect(mock.drawn).not.toHaveBeenCalled();
  // A failed destructor cannot leave Busy stuck; the underlying mock permits a new transaction.
  await f.hook("run", "17", claim);
  expect(mock.drawn).toHaveBeenCalledTimes(1);
  f.stop();
  expect(f.window.__PULSE_TARGET_THEME__).toBeUndefined();
  expect(f.release).toHaveBeenCalledTimes(1);
});
it("wrong run/generation/claim cannot reach the read endpoint", async () => {
  const f = setup();
  for (const [nonce, generation, token] of [
    ["foreign", "17", claim],
    ["run", "017", claim],
    ["run", "17", "wrong"],
  ] as const)
    await expect(f.hook(nonce, generation, token)).rejects.toThrow();
  expect(mock.read).not.toHaveBeenCalled();
  f.stop();
});
it("wrong returned identity cannot confirm or draw", async () => {
  const f = setup();
  mock.read.mockResolvedValueOnce({
    ticket: claim,
    generation: "18",
    paintJson: "fixed",
  });
  await expect(f.hook("run", "17", claim)).rejects.toThrow();
  expect(mock.confirm).not.toHaveBeenCalled();
  expect(mock.drawn).not.toHaveBeenCalled();
  f.stop();
});
it("pagehide during confirmation rejects late paint and preserves a replacement hook", async () => {
  const f = setup();
  const replacement = vi.fn();
  mock.confirm.mockImplementationOnce(async () => {
    f.window.__PULSE_TARGET_THEME__ = replacement;
    f.window.dispatchEvent(new Event("pagehide"));
  });
  await expect(f.hook("run", "17", claim)).rejects.toThrow();
  expect(mock.drawn).not.toHaveBeenCalled();
  expect(color).toBe("");
  expect(f.window.__PULSE_TARGET_THEME__).toBe(replacement);
});
it("cleanup failure is never a drawn receipt", async () => {
  const f = setup();
  f.view.revokeTheme.mockImplementation(() => true);
  await expect(f.hook("run", "17", claim)).rejects.toThrow("cleanup");
  expect(mock.drawn).not.toHaveBeenCalled();
  f.view.dispose.mockImplementation(() => {
    color = "";
  });
  f.stop();
});
it("active confirmation retains busy ownership and abort rejects its late result", async () => {
  const f = setup();
  let release!: () => void;
  const held = new Promise<void>((r) => {
    release = r;
  });
  let started!: () => void;
  const atConfirm = new Promise<void>((r) => {
    started = r;
  });
  mock.confirm.mockImplementationOnce(async () => {
    started();
    await held;
  });
  const first = f.hook("run", "17", claim);
  await atConfirm;
  await expect(f.hook("run", "17", claim)).rejects.toThrow();
  f.life.abort();
  release();
  await expect(first).rejects.toThrow();
  expect(mock.drawn).not.toHaveBeenCalled();
  expect(color).toBe("");
});
