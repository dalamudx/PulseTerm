import { afterEach, expect, it, vi } from "vitest";

const mock = vi.hoisted(() => ({
  protocol: vi.fn(),
  capture: vi.fn(),
  ack: vi.fn(),
  confirm: vi.fn(),
  done: vi.fn(),
  options: undefined as unknown,
  channel: undefined as unknown,
  finish: vi.fn(),
  dispose: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (data: ArrayBuffer) => void = () => {};
    constructor() {
      mock.channel = this;
    }
  },
}));
vi.mock("./gen/commands", () => ({
  commands: {
    probeSessionProtocol: mock.protocol,
    probePeerView: mock.capture,
    probePeerAck: mock.ack,
    probePeerConfirm: mock.confirm,
    probePeerDone: mock.done,
  },
}));
vi.mock("./session-probe", () => ({ digest: () => "a".repeat(16) }));
vi.mock("./transfer-view-probe", () => ({
  TransferViewProbe: class {
    constructor(options: unknown) {
      mock.options = options;
    }
    receive() {}
    finish = mock.finish;
    dispose = mock.dispose;
  },
}));
type Options = {
  signal: AbortSignal;
  ack: (generation: string, sequence: number) => Promise<void>;
  confirm: (
    generation: string,
    boundary: {
      columns: number;
      rows: number;
      revision: bigint;
      parsedBytes: bigint;
    },
    digest: string,
  ) => Promise<void>;
  failed: (error: unknown) => void;
  live?: { ready: () => Promise<void>; marker: Options["confirm"] };
};
async function setup() {
  vi.resetModules();
  vi.clearAllMocks();
  mock.protocol.mockResolvedValue(2);
  mock.capture.mockResolvedValue(undefined);
  mock.finish.mockResolvedValue(undefined);
  mock.done.mockResolvedValue(undefined);
  mock.ack.mockImplementation(
    async (_run: string, page: string, generation: string) => {
      if (page !== "page" || generation !== "17") throw new Error("denied");
    },
  );
  let confirmed = false;
  mock.confirm.mockImplementation(async (_run: string, page: string) => {
    if (page !== "page" || confirmed) throw new Error("denied");
    confirmed = true;
  });
  const win = new EventTarget();
  const root = { hidden: false };
  const host = { style: {}, setAttribute: vi.fn(), remove: vi.fn() };
  const append = vi.fn();
  vi.stubGlobal("window", win);
  vi.stubGlobal("document", {
    createElement: () => host,
    getElementById: () => root,
    body: { append },
  });
  const { verifyPeerView } = await import("./peer-view-probe");
  return { verifyPeerView, win, root, host, append };
}
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
it("readonly wrapper uses page-scoped ACK/confirmation and cleans its own surface once", async () => {
  const s = await setup();
  await s.verifyPeerView("run", "page");
  const options = mock.options as Options;
  await options.ack("17", 2);
  await options.confirm(
    "17",
    { columns: 40, rows: 12, revision: 3n, parsedBytes: 76n },
    "a".repeat(16),
  );
  expect(mock.ack).toHaveBeenCalledWith("run", "page", "17", 2);
  expect(mock.confirm).toHaveBeenCalledWith("run", "page", "17", {
    columns: 40,
    rows: 12,
    revision: "3",
    parsedBytes: "76",
    digest: "a".repeat(16),
  });
  expect(mock.finish).toHaveBeenCalledOnce();
  expect(mock.done).toHaveBeenCalledOnce();
  expect(mock.done).toHaveBeenCalledWith("run", "page");
  expect(mock.dispose).toHaveBeenCalledOnce();
  expect(s.host.remove).toHaveBeenCalledOnce();
  expect(s.root.hidden).toBe(false);
  expect(options.signal.aborted).toBe(true);
  await expect(s.verifyPeerView("run", "page")).rejects.toThrow(
    "peer.view_duplicate",
  );
  expect(mock.capture).toHaveBeenCalledOnce();
});
it("new document rejects its formerly authorized page nonce on its own ACK and confirm endpoints", async () => {
  const s = await setup();
  await s.verifyPeerView("run", "page", "old-page");
  const options = mock.options as Options;
  await options.ack("17", 2);
  await options.confirm(
    "17",
    { columns: 40, rows: 12, revision: 3n, parsedBytes: 76n },
    "a".repeat(16),
  );
  expect(mock.ack).toHaveBeenCalledWith("run", "old-page", "17", 2);
  expect(mock.confirm).toHaveBeenCalledWith(
    "run",
    "old-page",
    "17",
    expect.objectContaining({ parsedBytes: "76" }),
  );
  expect(mock.ack).toHaveBeenCalledWith("run", "page", "17", 2);
  expect(mock.done).toHaveBeenCalledOnce();
});
it("retained live wrapper confirms bootstrap and Marker while native subscription remains alive until pagehide", async () => {
  const s = await setup();
  let admit!: () => void, close!: () => void;
  const ready = new Promise<void>((r) => {
    admit = r;
  });
  mock.capture.mockImplementation(() => {
    admit();
    return new Promise<void>((r) => {
      close = r;
    });
  });
  const live = s.verifyPeerView("run", "page", undefined, true);
  await ready;
  const options = mock.options as Options;
  expect(options.live).toBeDefined();
  expect(mock.done).not.toHaveBeenCalled();
  await options.live?.ready();
  expect(mock.done).toHaveBeenCalledOnce();
  expect(s.host.remove).not.toHaveBeenCalled();
  expect(options.signal.aborted).toBe(false);
  await options.live?.marker(
    "17",
    { columns: 40, rows: 12, revision: 4n, parsedBytes: 90n },
    "a".repeat(16),
  );
  expect(mock.confirm).toHaveBeenCalledWith(
    "run",
    "page",
    "17",
    expect.objectContaining({ revision: "4", parsedBytes: "90" }),
  );
  s.win.dispatchEvent(new Event("pagehide"));
  await live;
  expect(s.host.remove).toHaveBeenCalledOnce();
  expect(mock.dispose).toHaveBeenCalledOnce();
  close();
  await Promise.resolve();
  expect(mock.capture).toHaveBeenCalledOnce();
  expect(s.root.hidden).toBe(false);
});
it("native live closure before bootstrap is not a successful display or Done", async () => {
  const s = await setup();
  await expect(
    s.verifyPeerView("run", "page", undefined, true),
  ).rejects.toThrow("peer.live_closed_before_bootstrap");
  expect(mock.done).not.toHaveBeenCalled();
  expect(s.host.remove).toHaveBeenCalledOnce();
});
it("native live failure after bootstrap is preserved and disposes only the old surface", async () => {
  const s = await setup();
  let admit!: () => void, fail!: (error: Error) => void;
  const ready = new Promise<void>((r) => {
    admit = r;
  });
  mock.capture.mockImplementation(() => {
    admit();
    return new Promise<void>((_r, reject) => {
      fail = reject;
    });
  });
  const live = s.verifyPeerView("run", "page", undefined, true);
  const failed = expect(live).rejects.toThrow("native.marker_failed");
  await ready;
  await (mock.options as Options).live?.ready();
  fail(new Error("native.marker_failed"));
  await failed;
  expect(mock.done).toHaveBeenCalledOnce();
  expect(s.host.remove).toHaveBeenCalledOnce();
  expect(s.root.hidden).toBe(false);
});
it("pagehide cancels only the surface even while native command awaits and late completion cannot recreate it", async () => {
  const s = await setup();
  let admitted!: () => void;
  const ready = new Promise<void>((r) => {
    admitted = r;
  });
  let complete!: () => void;
  mock.capture.mockImplementation(() => {
    admitted();
    return new Promise<void>((r) => {
      complete = r;
    });
  });
  const running = s.verifyPeerView("run", "page");
  const failed = expect(running).rejects.toThrow("peer.view_cancelled");
  await ready;
  s.win.dispatchEvent(new Event("pagehide"));
  await failed;
  complete();
  await Promise.resolve();
  expect(mock.dispose).toHaveBeenCalledOnce();
  expect(mock.finish).not.toHaveBeenCalled();
  expect(mock.done).not.toHaveBeenCalled();
  expect(s.host.remove).toHaveBeenCalledOnce();
  expect(s.root.hidden).toBe(false);
  (mock.channel as { onmessage: (b: ArrayBuffer) => void }).onmessage(
    new ArrayBuffer(0),
  );
  expect(s.append).toHaveBeenCalledOnce();
});
it("render failure is preserved and native completion cannot turn cancellation into success", async () => {
  const s = await setup();
  let admitted!: () => void;
  const ready = new Promise<void>((r) => {
    admitted = r;
  });
  mock.capture.mockImplementation(() => {
    admitted();
    return new Promise<void>(() => {});
  });
  const run = s.verifyPeerView("run", "page");
  const failed = expect(run).rejects.toThrow("render.failed");
  await ready;
  (mock.options as Options).failed(new Error("render.failed"));
  await failed;
  expect(mock.finish).not.toHaveBeenCalled();
  expect(mock.done).not.toHaveBeenCalled();
  expect(s.root.hidden).toBe(false);
  expect(mock.dispose).toHaveBeenCalledOnce();
});
it("protocol mismatch and deadline reject rather than leaving a parked DOM host", async () => {
  const s = await setup();
  mock.protocol.mockResolvedValue(3);
  await expect(s.verifyPeerView("run", "page")).rejects.toThrow(
    "peer.view_protocol",
  );
  expect(mock.capture).not.toHaveBeenCalled();
  expect(s.host.remove).toHaveBeenCalledOnce();
  const next = await setup();
  vi.useFakeTimers();
  let admitted!: () => void;
  const ready = new Promise<void>((r) => {
    admitted = r;
  });
  mock.capture.mockImplementation(() => {
    admitted();
    return new Promise<void>(() => {});
  });
  const run = next.verifyPeerView("run", "page");
  const failed = expect(run).rejects.toThrow("peer.view_deadline");
  await ready;
  await vi.advanceTimersByTimeAsync(7000);
  await failed;
  expect(next.host.remove).toHaveBeenCalledOnce();
  expect(next.root.hidden).toBe(false);
  expect(mock.dispose).toHaveBeenCalledOnce();
});
