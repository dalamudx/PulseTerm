import { readFileSync } from "node:fs";
import { expect, it, vi } from "vitest";
import { type DrawnBoundary, TerminalViewStream } from "./terminal-view-stream";
import { GenerationViewHost } from "./view-host";

const generation = (1n << 60n) + 7n,
  base = 1n << 60n;
const wires = readFileSync(
  new URL(
    "../../../../tests/fixtures/frames/attachment-v2.hex",
    import.meta.url,
  ),
  "utf8",
)
  .trim()
  .split("\n")
  .map((line) =>
    Uint8Array.from(line.split(/\s+/), (n) => Number.parseInt(n, 16)),
  );
function frame(i: number): Uint8Array {
  const wire = wires[i];
  if (!wire) throw new Error("fixture");
  return wire.slice();
}
function deferred() {
  let resolve = () => {};
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function fixture(beforeDraw?: () => void) {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn()),
    scope = host.begin(generation);
  const writes: Uint8Array[] = [],
    ack: number[] = [],
    observed: Readonly<DrawnBoundary>[] = [];
  const surface = {
    writeParsed: vi.fn(async (bytes: Uint8Array, _bellSignal?: AbortSignal) => {
      writes.push(bytes.slice());
    }),
    beginRepair: vi.fn(),
    finishParsed: vi.fn(async () => {}),
    visibleDigest: vi.fn(() => "0123456789abcdef"),
    prepareDraw: vi.fn(),
    verifyDraw: vi.fn(),
    dispose: vi.fn(),
  };
  const renderer = {
    create: vi.fn(async () => surface),
    draw: vi.fn(async () => {}),
  };
  const callbacks = {
    beforeDraw,
    acknowledge: vi.fn(async (_g: bigint, seq: number) => {
      ack.push(seq);
    }),
    observe: vi.fn(async (b: Readonly<DrawnBoundary>) => {
      observed.push(b);
    }),
  };
  const stream = new TerminalViewStream(
    scope,
    {} as HTMLElement,
    "fixed-palette",
    16,
    callbacks,
    renderer,
  );
  const bootstrap = async () => {
    for (let i = 0; i < 3; i++) await stream.enqueue(frame(i));
  };
  return {
    host,
    scope,
    stream,
    renderer,
    callbacks,
    surface,
    writes,
    ack,
    observed,
    bootstrap,
  };
}
it("resize pause cancels pending frame without ACK or releasing the native-bound scope", async () => {
  const f = fixture();
  await f.bootstrap();
  await f.stream.enqueue(frame(3));
  const gate = deferred();
  f.surface.writeParsed.mockImplementation(() => gate.promise);
  const data = f.stream.enqueue(frame(4));
  const rejected = expect(data).rejects.toThrow("fit.paused");
  await Promise.resolve();
  f.stream.pauseForResize();
  await rejected;
  expect(f.scope.isCurrent()).toBe(true);
  expect(f.surface.dispose).not.toHaveBeenCalled();
  gate.resolve();
  await Promise.resolve();
  expect(f.ack).toEqual([0, 1, 2, 3]);
  f.stream.cancel();
});

it("geometry change during drawing aborts old frame before verification or ACK, keeps scope for resize", async () => {
  let calls = 0,
    enabled = false;
  const f = fixture(() => {
    if (enabled && ++calls === 2) f.stream.pauseForResize();
  });
  await f.bootstrap();
  await f.stream.enqueue(frame(3));
  enabled = true;
  const gate = deferred();
  f.renderer.draw.mockImplementation(() => gate.promise);
  const data = f.stream.enqueue(frame(4));
  const rejected = expect(data).rejects.toThrow("fit.paused");
  await Promise.resolve();
  await Promise.resolve();
  gate.resolve();
  await rejected;
  expect(f.scope.isCurrent()).toBe(true);
  expect(f.ack).toEqual([0, 1, 2, 3]);
  f.stream.cancel();
});
it("holds Commit credit until parser and drawing complete, and observes captured dual coordinates after its ACK", async () => {
  const f = fixture(),
    gate = deferred();
  f.renderer.draw.mockImplementation(() => gate.promise);
  await f.stream.enqueue(frame(0));
  await f.stream.enqueue(frame(1));
  const commit = f.stream.enqueue(frame(2));
  await vi.waitFor(() => expect(f.renderer.draw).toHaveBeenCalledOnce());
  expect(f.ack).toEqual([0, 1]);
  expect(f.observed).toEqual([]);
  gate.resolve();
  await commit;
  expect(f.ack).toEqual([0, 1, 2]);
  expect(f.observed[0]).toMatchObject({
    kind: "capture",
    generation,
    sourceBytes: base + 33n,
    utf8Bytes: base + 43n,
  });
  expect(Object.isFrozen(f.observed[0])).toBe(true);
  f.stream.cancel();
  expect(f.surface.finishParsed).not.toHaveBeenCalled();
});
it("blocks already queued Marker/Data while captured confirmation is pending, without confusing it with latest freshness", async () => {
  const f = fixture(),
    gate = deferred();
  f.callbacks.observe.mockImplementation(async (b) => {
    f.observed.push(b);
    if (b.kind === "capture") await gate.promise;
  });
  await f.stream.enqueue(frame(0));
  await f.stream.enqueue(frame(1));
  const commit = f.stream.enqueue(frame(2));
  await vi.waitFor(() => expect(f.observed).toHaveLength(1));
  const marker = f.stream.enqueue(frame(3)),
    data = f.stream.enqueue(frame(4));
  expect(f.ack).toEqual([0, 1, 2]);
  expect(f.writes).toHaveLength(1);
  gate.resolve();
  await Promise.all([commit, marker, data]);
  expect(f.ack).toEqual([0, 1, 2, 3, 4]);
  expect(f.observed[1]).toMatchObject({
    kind: "marker",
    sourceBytes: base + 34n,
    utf8Bytes: base + 43n,
  });
  expect(f.surface.finishParsed).not.toHaveBeenCalled();
  f.stream.cancel();
});
it("uses CAN screen-only repair without recreating the terminal, and flushes decoder only on ordered End", async () => {
  const f = fixture();
  await f.bootstrap();
  await f.stream.enqueue(frame(3));
  await f.stream.enqueue(frame(4));
  const begin = frame(5);
  new DataView(begin.buffer).setUint32(42, 2, true);
  const original = frame(6),
    replay = new Uint8Array(original.length + 1);
  replay.set(original);
  replay[34] = 0x18;
  replay[35] = 82;
  for (const w of [begin, replay, frame(7), frame(8)])
    await f.stream.enqueue(w);
  expect(f.renderer.create).toHaveBeenCalledOnce();
  expect(f.surface.beginRepair).toHaveBeenCalledOnce();
  expect(f.writes.at(-1)).toEqual(Uint8Array.of(0x18, 82));
  expect(
    f.surface.writeParsed.mock.calls.map(([, signal]) => signal !== undefined),
  ).toEqual([false, true, false]);
  expect(f.surface.finishParsed).toHaveBeenCalledOnce();
  expect(f.observed.at(-1)).toMatchObject({
    kind: "end",
    revision: 4n,
    sourceBytes: base + 35n,
    utf8Bytes: base + 46n,
  });
  f.stream.cancel();
  expect(f.surface.dispose).toHaveBeenCalledOnce();
});
it("rejects a repair lacking CAN and withholds its ACK without clearing history or treating cancellation as EOF", async () => {
  const f = fixture();
  await f.bootstrap();
  for (let i = 3; i < 7; i++) await f.stream.enqueue(frame(i));
  await expect(f.stream.enqueue(frame(7))).rejects.toThrow("requires_can");
  expect(f.ack).toEqual([0, 1, 2, 3, 4, 5, 6]);
  expect(f.surface.beginRepair).not.toHaveBeenCalled();
  expect(f.surface.finishParsed).not.toHaveBeenCalled();
  expect(f.scope.isCurrent()).toBe(false);
  expect(f.surface.dispose).toHaveBeenCalledOnce();
});
it("rejects late drawing after refresh with no Commit ACK, flush or new generation side effects", async () => {
  const f = fixture(),
    gate = deferred();
  f.renderer.draw.mockImplementation(() => gate.promise);
  await f.stream.enqueue(frame(0));
  await f.stream.enqueue(frame(1));
  const commit = f.stream.enqueue(frame(2)),
    rejected = expect(commit).rejects.toThrow("retired");
  await vi.waitFor(() => expect(f.renderer.draw).toHaveBeenCalledOnce());
  const fresh = f.host.begin(generation + 1n);
  await rejected;
  gate.resolve();
  expect(f.ack).toEqual([0, 1]);
  expect(f.observed).toEqual([]);
  expect(f.surface.dispose).toHaveBeenCalledOnce();
  expect(f.surface.finishParsed).not.toHaveBeenCalled();
  expect(fresh.isCurrent()).toBe(true);
  fresh.release();
});
it("failure of captured confirmation poisons queued Data despite already admitted Commit ACK", async () => {
  const f = fixture(),
    gate = deferred();
  f.callbacks.observe.mockImplementation(async () => {
    await gate.promise;
    throw new Error("capture.denied");
  });
  await f.stream.enqueue(frame(0));
  await f.stream.enqueue(frame(1));
  const commit = f.stream.enqueue(frame(2)),
    rejected = expect(commit).rejects.toThrow("capture.denied");
  await vi.waitFor(() => expect(f.callbacks.observe).toHaveBeenCalledOnce());
  const data = f.stream.enqueue(frame(3)),
    dataRejected = expect(data).rejects.toThrow("capture.denied");
  gate.resolve();
  await Promise.all([rejected, dataRejected]);
  expect(f.ack).toEqual([0, 1, 2]);
  expect(f.surface.dispose).toHaveBeenCalledOnce();
});
it("rejects mixed codecs and ignores old generation without granting ACK credit", async () => {
  const f = fixture(),
    foreign = frame(0);
  new DataView(foreign.buffer).setBigUint64(10, generation - 1n, true);
  await expect(f.stream.enqueue(foreign)).resolves.toBeNull();
  expect(f.ack).toEqual([]);
  const wrong = frame(0);
  wrong[6] = 1;
  await expect(f.stream.enqueue(wrong)).rejects.toThrow("version");
  expect(f.scope.isCurrent()).toBe(false);
  expect(f.renderer.create).not.toHaveBeenCalled();
});
it("bootstrap timeout retires a blocked capture without flushing, and cannot retire a replacement scope", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  try {
    const f = fixture(),
      gate = deferred();
    f.callbacks.observe.mockImplementation(() => gate.promise);
    await f.stream.enqueue(frame(0));
    await f.stream.enqueue(frame(1));
    const commit = f.stream.enqueue(frame(2)),
      rejected = expect(commit).rejects.toThrow("bootstrap_timeout");
    await vi.advanceTimersByTimeAsync(3000);
    await rejected;
    expect(f.surface.finishParsed).not.toHaveBeenCalled();
    expect(f.scope.isCurrent()).toBe(false);
    const fresh = f.host.begin(generation + 1n);
    gate.resolve();
    await vi.advanceTimersByTimeAsync(3000);
    expect(fresh.isCurrent()).toBe(true);
    fresh.release();
  } finally {
    vi.useRealTimers();
  }
});
it("does not ACK a parser-successful frame whose DOM row/layout verification failed", async () => {
  const f = fixture();
  await f.stream.enqueue(frame(0));
  await f.stream.enqueue(frame(1));
  f.surface.verifyDraw.mockImplementation(() => {
    throw new Error("terminal.dom_projection");
  });
  await expect(f.stream.enqueue(frame(2))).rejects.toThrow("dom_projection");
  expect(f.ack).toEqual([0, 1]);
  expect(f.observed).toEqual([]);
  expect(f.scope.isCurrent()).toBe(false);
  expect(f.surface.finishParsed).not.toHaveBeenCalled();
});
it("holds frame credit and capture until asynchronous GPU drawing completes", async () => {
  const f = fixture(),
    gate = deferred();
  await f.stream.enqueue(frame(0));
  await f.stream.enqueue(frame(1));
  f.surface.verifyDraw.mockImplementation(() => gate.promise);
  const job = f.stream.enqueue(frame(2));
  await vi.waitFor(() => expect(f.surface.verifyDraw).toHaveBeenCalled());
  expect(f.ack).toEqual([0, 1]);
  expect(f.observed).toEqual([]);
  gate.resolve();
  await job;
  expect(f.ack).toEqual([0, 1, 2]);
  expect(f.observed).toHaveLength(1);
});
it("disarms bootstrap expiry after current capture confirmation rather than evicting a live view", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  try {
    const f = fixture();
    await f.bootstrap();
    await vi.advanceTimersByTimeAsync(6000);
    expect(f.scope.isCurrent()).toBe(true);
    await f.stream.enqueue(frame(3));
    await f.stream.enqueue(frame(4));
    expect(f.ack).toEqual([0, 1, 2, 3, 4]);
    f.stream.cancel();
  } finally {
    vi.useRealTimers();
  }
});
