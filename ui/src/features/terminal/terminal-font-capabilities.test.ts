import { expect, it, vi } from "vitest";
import { createTerminalSurface, type SurfaceFactory } from "./terminal-surface";
import { GenerationViewHost } from "./view-host";

it("failed renderer factories revoke either drawing preference without input", async () => {
  const life = new AbortController();
  const host = new GenerationViewHost(life.signal, vi.fn());
  const factory = vi.fn(() => {
    throw Error("fixture.stop_after_admission");
  });
  const load = vi.fn(async () => factory as unknown as SurfaceFactory);
  const doc = {
    querySelector: () => ({ nonce: "fixture-style-nonce" }),
    createElement: vi.fn(),
  } as unknown as Document;
  const element = { ownerDocument: doc } as HTMLElement;
  const palette = JSON.stringify({
    version: 1,
    palette: {
      ansi: Array.from({ length: 16 }, () => [20, 30, 40]),
      foreground: [200, 200, 200],
      background: [0, 0, 0],
      cursor: [255, 255, 255],
    },
  });
  const geometry = {
    columns: 80,
    rows: 24,
    history: 0,
    display: {
      fontSize: 14,
      fontFamily: "monospace",
      cursorBlink: false,
      fontLigatures: true,
    },
  };
  try {
    await expect(
      createTerminalSurface(host.begin(1n), element, palette, geometry, load),
    ).rejects.toThrow("view.creation_failed");
    expect(load).toHaveBeenCalledOnce();
    await expect(
      createTerminalSurface(
        host.begin(2n),
        element,
        palette,
        { ...geometry, display: { ...geometry.display, fontLigatures: false } },
        load,
      ),
    ).rejects.toThrow("view.creation_failed");
    expect(load).toHaveBeenCalledTimes(2);
    expect(factory).toHaveBeenCalledTimes(2);
  } finally {
    life.abort();
  }
});
