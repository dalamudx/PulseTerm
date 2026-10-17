import { expect, it, vi } from "vitest";
import { profileDisplay } from "../../workspace-profile-editor";
import {
  createTerminalSurface,
  type SurfaceDriver,
  type SurfaceFactory,
} from "./terminal-surface";
import { GenerationViewHost } from "./view-host";

function fixture() {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn());
  const scope = host.begin(1n);
  const document = {
    querySelector: () => ({ nonce: "runtime-style-nonce" }),
    createElement: () => ({ setAttribute: vi.fn() }),
  } as unknown as Document;
  const hook = () => ({ dispose: vi.fn() });
  const terminal = {
    options: { disableStdin: true },
    parser: {
      registerCsiHandler: hook,
      registerDcsHandler: hook,
      registerEscHandler: hook,
      registerOscHandler: hook,
    },
    unicode: { versions: ["6"], activeVersion: "6", register: vi.fn() },
    attachCustomKeyEventHandler: vi.fn(),
    open: vi.fn(),
    dispose: vi.fn(),
    onData: vi.fn(),
  };
  const factory: SurfaceFactory = vi.fn(
    () => terminal as unknown as SurfaceDriver,
  );
  const load = vi.fn(async () => factory);
  const palette = JSON.stringify({
    version: 1,
    palette: {
      foreground: [200, 200, 200],
      background: [0, 0, 0],
      cursor: [255, 255, 255],
      ansi: Array.from({ length: 16 }, () => [20, 30, 40]),
    },
  });
  return {
    host,
    scope,
    element: { ownerDocument: document } as HTMLElement,
    terminal,
    factory,
    load,
    palette,
  };
}
it("defaults to input scrolling and preserves explicit false without enabling input", async () => {
  for (const scrollOnInput of [undefined, false, true]) {
    const f = fixture();
    const base = {
      fontSize: 14,
      fontFamily: "monospace",
      cursorBlink: false,
      ...(scrollOnInput === undefined ? {} : { scrollOnInput }),
    };
    const display = profileDisplay(base, {
      fontSize: 18,
      fontFamily: null,
      cursorBlink: null,
    });
    const surface = await createTerminalSurface(
      f.scope,
      f.element,
      f.palette,
      { columns: 32, rows: 8, history: 16, display },
      f.load,
    );
    expect(f.factory).toHaveBeenCalledWith(
      expect.objectContaining({
        scrollOnUserInput: scrollOnInput ?? true,
        fontSize: 18,
        disableStdin: true,
      }),
    );
    expect(f.terminal.onData).not.toHaveBeenCalled();
    surface.dispose();
    f.host.dispose();
  }
});
it("invalid scroll types are rejected before loading a renderer", async () => {
  const f = fixture();
  await expect(
    createTerminalSurface(
      f.scope,
      f.element,
      f.palette,
      {
        columns: 32,
        rows: 8,
        history: 16,
        display: {
          fontSize: 14,
          fontFamily: "monospace",
          cursorBlink: false,
          scrollOnInput: "false",
        } as never,
      },
      f.load,
    ),
  ).rejects.toThrow("view.creation_failed");
  expect(f.load).not.toHaveBeenCalled();
  f.host.dispose();
});

it("Option Meta defaults false and survives sparse Profile merging without enabling input", async () => {
  for (const macOptionIsMeta of [undefined, false, true]) {
    const f = fixture();
    const display = profileDisplay(
      {
        fontSize: 14,
        fontFamily: "monospace",
        cursorBlink: false,
        ...(macOptionIsMeta === undefined ? {} : { macOptionIsMeta }),
      },
      { fontSize: 18, fontFamily: null, cursorBlink: null },
    );
    const surface = await createTerminalSurface(
      f.scope,
      f.element,
      f.palette,
      { columns: 32, rows: 8, history: 16, display },
      f.load,
    );
    expect(f.factory).toHaveBeenCalledWith(
      expect.objectContaining({
        macOptionIsMeta: macOptionIsMeta ?? false,
        fontSize: 18,
        disableStdin: true,
      }),
    );
    expect(f.terminal.onData).not.toHaveBeenCalled();
    surface.dispose();
    f.host.dispose();
  }
});
