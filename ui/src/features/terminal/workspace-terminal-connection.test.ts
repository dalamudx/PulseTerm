import { readFileSync } from "node:fs";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, expect, it, vi } from "vitest";
import { GenerationViewHost } from "./view-host";
import {
  prepareWorkspaceTerminal,
  type TerminalConnectionOptions,
  type TerminalConnectionRpc,
  TerminalLayoutChanged,
} from "./workspace-terminal-connection";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: vi.fn() }));
afterEach(() => {
  vi.unstubAllEnvs();
  vi.mocked(invoke).mockReset();
});

const wires = readFileSync(
  new URL(
    "../../../../tests/fixtures/frames/attachment-v2.hex",
    import.meta.url,
  ),
  "utf8",
)
  .trim()
  .split("\n")
  .map((row) =>
    Uint8Array.from(row.split(/\s+/), (v) => Number.parseInt(v, 16)),
  );
const generation = (1n << 60n) + 7n;
function frame(index: number): ArrayBuffer {
  const wire = wires[index];
  if (!wire) throw new Error("fixture");
  return wire.slice().buffer;
}
function fixture() {
  const abort = new AbortController();
  const meta = {
    ticket: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    generation: generation.toString(),
    palette: JSON.stringify({
      version: 1,
      palette: {
        ansi: Array.from({ length: 16 }, () => [0, 0, 0]),
        foreground: [255, 255, 255],
        background: [0, 0, 0],
        cursor: [255, 255, 255],
      },
    }),
    history: 16,
    ended: false,
  };
  const rpc = {
    openLink: vi.fn<NonNullable<TerminalConnectionRpc["openLink"]>>(
      async () => {},
    ),
    prepare: vi.fn(async (): Promise<unknown> => meta),
    connect: vi.fn(async () => {}),
    acknowledge: vi.fn<TerminalConnectionRpc["acknowledge"]>(async () => {}),
    confirm: vi.fn<TerminalConnectionRpc["confirm"]>(async () => {}),
    detach: vi.fn(async () => {}),
    resize: vi.fn<NonNullable<TerminalConnectionRpc["resize"]>>(
      async (
        _nonce: string,
        _ticket: string,
        size: { columns: number; rows: number },
      ) => ({ ...size, basis: "local-native-confirmed" }),
    ),
    enableInput: vi.fn(async (_nonce: string, _ticket: string) => {}),
    input: vi.fn(
      async (_nonce: string, _ticket: string, _bytes: Uint8Array) => {},
    ),
  };
  let input: ((text: string) => void) | undefined;
  const surface = {
    enableInput: vi.fn((listener: (text: string) => void) => {
      input = listener;
      return () => {
        input = undefined;
      };
    }),
    writeParsed: vi.fn(async () => {}),
    finishParsed: vi.fn(async () => {}),
    resize: vi.fn(),
    beginRepair: vi.fn(async () => {}),
    visibleDigest: vi.fn(() => "0000000000000000"),
    measuredDimensions: vi.fn<
      () => { columns: number; rows: number } | undefined
    >(() => undefined),
    prepareDraw: vi.fn(() => null),
    verifyDraw: vi.fn(),
    dispose: vi.fn(),
  };
  const renderer = {
    create: vi.fn<NonNullable<TerminalConnectionOptions["renderer"]>["create"]>(
      async () => surface,
    ),
    draw: vi.fn(async () => {}),
  };
  let receive: (wire: ArrayBuffer) => void = () => {};
  const host = new GenerationViewHost(abort.signal, vi.fn());
  const options: TerminalConnectionOptions = {
    nonce: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
    session: "1",
    element: new EventTarget() as HTMLElement,
    host,
    signal: abort.signal,
    failed: vi.fn(),
    rpc,
    renderer,
    channel: (callback) => {
      receive = callback;
      return {};
    },
  };
  const send = (index: number) => receive(frame(index));
  return {
    input: (text: string) => input?.(text),
    options,
    rpc,
    renderer,
    surface,
    host,
    abort,
    meta,
    send,
    receive: (wire: ArrayBuffer) => receive(wire),
  };
}
it("only the explicit timing bundle exports once at EOF under the original page and connection", async () => {
  vi.stubEnv("VITE_ECHO_TIMING_PROBE", "1");
  vi.mocked(invoke).mockResolvedValue(undefined);
  const f = fixture();
  const c = await prepareWorkspaceTerminal(f.options);
  for (let i = 0; i < 3; i++) f.send(i);
  await c.ready;
  expect(invoke).not.toHaveBeenCalled();
  f.send(3);
  const eof = frame(3);
  new Uint8Array(eof)[7] = 7;
  new DataView(eof).setUint32(2, 4, true);
  f.receive(eof);
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledOnce());
  const [command, args] = vi.mocked(invoke).mock.calls[0] ?? [];
  expect(command).toBe("workspace_terminal_load_timings");
  expect(args).toMatchObject({
    nonce: f.options.nonce,
    connection: f.meta.ticket,
  });
  expect(
    JSON.parse(String((args as Record<string, unknown>).timing)),
  ).toMatchObject({
    schemaVersion: 1,
    overflow: false,
  });
  await vi.waitFor(() =>
    expect(f.rpc.acknowledge).toHaveBeenCalledWith(
      f.options.nonce,
      f.meta.ticket,
      4,
    ),
  );
  c.dispose();
});
it("ordinary builds and retired timing generations never publish diagnostics", async () => {
  for (const active of [false, true]) {
    vi.stubEnv("VITE_ECHO_TIMING_PROBE", active ? "1" : "0");
    const f = fixture();
    const c = await prepareWorkspaceTerminal(f.options);
    for (let i = 0; i < 3; i++) f.send(i);
    await c.ready;
    f.send(3);
    if (active) c.dispose();
    const eof = frame(3);
    new Uint8Array(eof)[7] = 7;
    new DataView(eof).setUint32(2, 4, true);
    f.receive(eof);
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(invoke).not.toHaveBeenCalled();
    c.dispose();
  }
});

it("authorized Raw Begin uses sparse Profile overrides and retains clipboard settings", async () => {
  const f = fixture();
  const appearance = {
    fontSize: 18,
    fontFamily: "Pulse Mono",
    cursorBlink: false,
  };
  f.rpc.prepare.mockResolvedValue({ ...f.meta, appearance });
  f.options.display = {
    fontSize: 14,
    fontFamily: "Mono",
    cursorBlink: true,
    copyOnSelect: true,
    rightClickPaste: false,
  };
  const c = await prepareWorkspaceTerminal(f.options);
  for (let i = 0; i < 4; i++) f.send(i);
  await c.ready;
  expect(f.renderer.create).toHaveBeenCalledWith(
    expect.anything(),
    f.options.element,
    f.meta.palette,
    expect.objectContaining({
      display: { ...f.options.display, ...appearance },
    }),
  );
  expect(f.rpc.enableInput).not.toHaveBeenCalled();
  c.dispose();
});
it("invalid prepared Profile appearance never creates renderer or input", async () => {
  const f = fixture();
  f.rpc.prepare.mockResolvedValue({
    ...f.meta,
    appearance: { fontSize: 73, fontFamily: null, cursorBlink: null },
  });
  await expect(prepareWorkspaceTerminal(f.options)).rejects.toThrow();
  expect(f.renderer.create).not.toHaveBeenCalled();
  expect(f.rpc.enableInput).not.toHaveBeenCalled();
});
it.each(["local-native-confirmed", "ssh-request-admitted"])(
  "layout before Commit (%s) sends no old drawing ACK/confirm/input and rebuilds only after backend layout success",
  async (basis) => {
    vi.useFakeTimers();
    const f = fixture();
    f.surface.measuredDimensions.mockReturnValue({ columns: 90, rows: 30 });
    let finish = () => {};
    const native = new Promise<unknown>((resolve) => {
      finish = () => resolve({ columns: 90, rows: 30, basis });
    });
    f.rpc.resize.mockImplementation(() => native);
    f.rpc.detach.mockRejectedValue("workspace.denied");
    f.options.input = true;
    const c = await prepareWorkspaceTerminal(f.options);
    const rejected = expect(c.ready).rejects.toBeInstanceOf(
      TerminalLayoutChanged,
    );
    f.send(0);
    f.send(1);
    f.send(2);
    f.send(3);
    await vi.advanceTimersByTimeAsync(50);
    expect(f.rpc.resize).toHaveBeenCalledExactlyOnceWith(
      f.options.nonce,
      f.meta.ticket,
      { columns: 90, rows: 30 },
    );
    expect(f.rpc.acknowledge.mock.calls.map((call) => call[2])).toEqual([0, 1]); // Header/metadata only, never Data/Commit.
    expect(f.rpc.confirm).not.toHaveBeenCalled();
    expect(f.rpc.enableInput).not.toHaveBeenCalled();
    expect(f.rpc.detach).not.toHaveBeenCalled();
    expect(f.options.failed).not.toHaveBeenCalled();
    finish();
    await vi.advanceTimersByTimeAsync(1);
    await rejected;
    expect(f.rpc.detach).toHaveBeenCalledOnce();
    await expect(c.suspend()).resolves.toBeUndefined();
    expect(f.options.failed).not.toHaveBeenCalled();
  },
);
it("uncertain bootstrap layout never reports known success or retries", async () => {
  vi.useFakeTimers();
  const f = fixture();
  f.surface.measuredDimensions.mockReturnValue({ columns: 90, rows: 30 });
  f.rpc.resize.mockImplementation(() => new Promise<void>(() => {}));
  const c = await prepareWorkspaceTerminal(f.options);
  const rejected = expect(c.ready).rejects.toThrow("fit.uncertain");
  for (let i = 0; i < 4; i++) f.send(i);
  await vi.advanceTimersByTimeAsync(6050);
  await rejected;
  expect(f.rpc.resize).toHaveBeenCalledOnce();
  expect(f.rpc.enableInput).not.toHaveBeenCalled();
  expect(f.options.failed).toHaveBeenCalledOnce();
});
it("prepares before connect, creates scope from Raw Begin, ACKs and confirms capture without lossy coordinates", async () => {
  const f = fixture();
  const connection = await prepareWorkspaceTerminal(f.options);
  expect(f.renderer.create).not.toHaveBeenCalled();
  expect(f.rpc.acknowledge).not.toHaveBeenCalled();
  f.send(0);
  f.send(1);
  f.send(2);
  await connection.ready;
  expect(f.rpc.acknowledge.mock.calls.map((call) => call[2])).toEqual([
    0, 1, 2,
  ]);
  expect(f.rpc.confirm.mock.calls[0]?.[2]).toMatchObject({
    generation,
    kind: "capture",
    sourceBytes: (1n << 60n) + 33n,
    utf8Bytes: (1n << 60n) + 43n,
  });
  connection.dispose();
  expect(f.rpc.detach).toHaveBeenCalledOnce();
  expect(f.surface.dispose).toHaveBeenCalledOnce();
});
it("native connect may deliver Begin before the connect invocation returns", async () => {
  const f = fixture();
  f.rpc.connect.mockImplementation(async () => {
    f.send(0);
    f.send(1);
    f.send(2);
  });
  const connection = await prepareWorkspaceTerminal(f.options);
  await connection.ready;
  connection.dispose();
});
it("capture confirmation failure detaches and rejects ready", async () => {
  const f = fixture();
  f.rpc.confirm.mockRejectedValue(new Error("capture mismatch"));
  const connection = await prepareWorkspaceTerminal(f.options);
  f.send(0);
  f.send(1);
  f.send(2);
  await expect(connection.ready).rejects.toThrow("capture mismatch");
  expect(f.options.failed).toHaveBeenCalledOnce();
  expect(f.rpc.detach).toHaveBeenCalledOnce();
});
it("page cancellation ignores late Raw callbacks and detaches once", async () => {
  const f = fixture();
  const connection = await prepareWorkspaceTerminal(f.options);
  f.abort.abort();
  f.send(0);
  await expect(connection.ready).rejects.toThrow("retired");
  expect(f.renderer.create).not.toHaveBeenCalled();
  expect(f.rpc.acknowledge).not.toHaveBeenCalled();
  expect(f.rpc.detach).toHaveBeenCalledOnce();
});
it("non-raw callbacks, wrong generation and non-Begin each fail without creating scope or ACK", async () => {
  for (const kind of ["json", "generation", "nonbegin"]) {
    const f = fixture();
    const connection = await prepareWorkspaceTerminal(f.options);
    if (kind === "json") f.receive([1, 2] as unknown as ArrayBuffer);
    else if (kind === "nonbegin") f.send(1);
    else {
      const b = new Uint8Array(frame(0));
      b[10] = (b[10] ?? 0) ^ 1;
      f.receive(b.buffer);
    }
    await expect(connection.ready).rejects.toThrow();
    expect(f.renderer.create).not.toHaveBeenCalled();
    expect(f.rpc.acknowledge).not.toHaveBeenCalled();
    expect(f.rpc.detach).toHaveBeenCalledOnce();
  }
});
it("invalid metadata cannot connect, but a valid ticket is detached", async () => {
  for (const bad of [
    { generation: 1 },
    { generation: "01" },
    { generation: "18446744073709551616" },
    { history: 10001 },
    { extra: true },
    { palette: "{}" },
  ]) {
    const f = fixture();
    f.rpc.prepare.mockResolvedValue({ ...f.meta, ...bad });
    await expect(prepareWorkspaceTerminal(f.options)).rejects.toThrow();
    expect(f.rpc.connect).not.toHaveBeenCalled();
    expect(f.rpc.detach).toHaveBeenCalledOnce();
  }
});
it("cancelled prepare result is detached without starting Raw Channel", async () => {
  const f = fixture();
  f.rpc.prepare.mockImplementation(async () => {
    f.abort.abort();
    return f.meta;
  });
  await expect(prepareWorkspaceTerminal(f.options)).rejects.toThrow("retired");
  expect(f.rpc.connect).not.toHaveBeenCalled();
  expect(f.rpc.detach).toHaveBeenCalledOnce();
});
it("connect errors dispose prepared lease, never close root session", async () => {
  const f = fixture();
  f.rpc.connect.mockRejectedValue(new Error("denied"));
  await expect(prepareWorkspaceTerminal(f.options)).rejects.toThrow("denied");
  expect(f.rpc.detach).toHaveBeenCalledOnce();
  expect(f.options.failed).toHaveBeenCalledOnce();
});
it("missing Begin has a fixed three-second deadline and does not leak a native ticket", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  try {
    const f = fixture();
    const connection = await prepareWorkspaceTerminal(f.options);
    const result = expect(connection.ready).rejects.toThrow(
      "bootstrap_timeout",
    );
    await vi.advanceTimersByTimeAsync(3000);
    await result;
    expect(f.rpc.detach).toHaveBeenCalledOnce();
    expect(f.options.failed).toHaveBeenCalledOnce();
    f.send(0);
    expect(f.renderer.create).not.toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
});
it("late Begin is rejected by monotonic time even before a delayed timer runs", async () => {
  const f = fixture();
  const connection = await prepareWorkspaceTerminal(f.options);
  const future = performance.now() + 3000;
  const clock = vi.spyOn(performance, "now").mockReturnValue(future);
  try {
    f.send(0);
    await expect(connection.ready).rejects.toThrow("bootstrap_timeout");
    expect(f.renderer.create).not.toHaveBeenCalled();
    expect(f.rpc.detach).toHaveBeenCalledOnce();
  } finally {
    clock.mockRestore();
  }
});
it("host retirement before Commit rejects readiness and prevents later ACKs", async () => {
  const f = fixture();
  const connection = await prepareWorkspaceTerminal(f.options);
  f.send(0);
  f.host.retire(generation);
  await expect(connection.ready).rejects.toThrow("retired");
  const count = f.rpc.acknowledge.mock.calls.length;
  f.send(1);
  f.send(2);
  expect(f.rpc.acknowledge.mock.calls.length).toBe(count);
  expect(f.rpc.detach).toHaveBeenCalledOnce();
});

it("input is armed only after native capture and validated Commit; UTF8 events are queued once", async () => {
  const f = fixture();
  const connection = await prepareWorkspaceTerminal({
    ...f.options,
    input: true,
  });
  expect(f.surface.enableInput).not.toHaveBeenCalled();
  f.input("early");
  expect(f.rpc.input).not.toHaveBeenCalled();
  f.send(0);
  f.send(1);
  f.send(2);
  await connection.ready;
  expect(f.rpc.enableInput).toHaveBeenCalledOnce();
  f.input("中🙂\r");
  await vi.waitFor(() =>
    expect(f.rpc.input).toHaveBeenCalledExactlyOnceWith(
      f.options.nonce,
      f.meta.ticket,
      new TextEncoder().encode("中🙂\r"),
    ),
  );
  connection.dispose();
  f.input("late");
  expect(f.rpc.input).toHaveBeenCalledOnce();
});
it("keeps the original renderer failure instead of replacing it with scope-retired cleanup", async () => {
  const f = fixture();
  f.surface.verifyDraw.mockImplementation(() => {
    throw new Error("terminal.dom_projection");
  });
  const c = await prepareWorkspaceTerminal(f.options);
  f.send(0);
  f.send(1);
  f.send(2);
  await expect(c.ready).rejects.toThrow("terminal.dom_projection");
  expect(f.options.failed).toHaveBeenCalledWith(
    expect.objectContaining({ message: "terminal.dom_projection" }),
  );
  expect(f.rpc.detach).toHaveBeenCalledOnce();
});

it("resize pauses input and ignores raw live frames without detaching before native confirmation", async () => {
  const f = fixture();
  const c = await prepareWorkspaceTerminal({ ...f.options, input: true });
  f.send(0);
  f.send(1);
  f.send(2);
  await c.ready;
  c.pauseForResize();
  expect(f.rpc.detach).not.toHaveBeenCalled();
  f.input("ignored");
  expect(f.rpc.input).not.toHaveBeenCalled();
  await c.resize({ columns: 48, rows: 10 });
  expect(f.rpc.resize).toHaveBeenCalledExactlyOnceWith(
    f.options.nonce,
    f.meta.ticket,
    { columns: 48, rows: 10 },
  );
  expect(f.rpc.detach).not.toHaveBeenCalled();
  c.dispose();
  expect(f.rpc.detach).toHaveBeenCalledOnce();
});

it("input-enable denial never opens local stdin and cancels the view", async () => {
  const f = fixture();
  f.rpc.enableInput.mockRejectedValue(new Error("unconfirmed"));
  const c = await prepareWorkspaceTerminal({ ...f.options, input: true });
  f.send(0);
  f.send(1);
  f.send(2);
  await expect(c.ready).rejects.toThrow("unconfirmed");
  expect(f.surface.enableInput).not.toHaveBeenCalled();
  expect(f.rpc.input).not.toHaveBeenCalled();
  expect(f.rpc.detach).toHaveBeenCalledOnce();
});
it("frozen Ended metadata waits for End and never enables input or emits input-ready", async () => {
  const f = fixture();
  f.meta.ended = true;
  const c = await prepareWorkspaceTerminal({ ...f.options, input: true });
  for (let i = 0; i < 3; i++) f.send(i);
  await new Promise<void>((r) => setTimeout(r, 0));
  expect(f.rpc.confirm).toHaveBeenCalledOnce();
  expect(f.rpc.enableInput).not.toHaveBeenCalled();
  f.send(3);
  const eof = frame(3);
  new Uint8Array(eof)[7] = 7;
  new DataView(eof).setUint32(2, 4, true);
  f.receive(eof);
  await c.ready;
  expect(c.hasEnded()).toBe(true);
  expect(f.surface.enableInput).not.toHaveBeenCalled();
  expect(f.options.failed).not.toHaveBeenCalled();
  await expect(c.announceInputReady()).rejects.toThrow("fit.not_ready");
  c.dispose();
});
it("EOF before late input-enable rejection resolves a readonly view and never installs input", async () => {
  const f = fixture();
  let entered = () => {},
    rejectEnable: (error: Error) => void = () => {};
  const started = new Promise<void>((r) => {
    entered = r;
  });
  f.rpc.enableInput.mockImplementation(() => {
    entered();
    return new Promise<void>((_, reject) => {
      rejectEnable = reject;
    });
  });
  const c = await prepareWorkspaceTerminal({ ...f.options, input: true });
  for (let i = 0; i < 3; i++) f.send(i);
  await started;
  f.send(3);
  const eof = frame(3);
  new Uint8Array(eof)[7] = 7;
  new DataView(eof).setUint32(2, 4, true);
  f.receive(eof);
  await c.ready;
  expect(c.hasEnded()).toBe(true);
  expect(() => c.pauseForResize()).toThrow("fit.not_ready");
  await expect(c.resize({ columns: 90, rows: 30 })).rejects.toThrow(
    "fit.not_ready",
  );
  await expect(c.announceInputReady()).rejects.toThrow("fit.not_ready");
  expect(f.rpc.resize).not.toHaveBeenCalled();
  expect(f.surface.enableInput).not.toHaveBeenCalled();
  rejectEnable(new Error("expired generation"));
  await new Promise<void>((r) => setTimeout(r, 0));
  expect(f.options.failed).not.toHaveBeenCalled();
  expect(f.surface.dispose).not.toHaveBeenCalled();
  c.dispose();
});
it("EOF does not hide a subsequent malformed frame", async () => {
  const f = fixture();
  const c = await prepareWorkspaceTerminal(f.options);
  for (let i = 0; i < 3; i++) f.send(i);
  await c.ready;
  f.send(3);
  const eof = frame(3);
  new Uint8Array(eof)[7] = 7;
  new DataView(eof).setUint32(2, 4, true);
  f.receive(eof);
  await new Promise<void>((r) => setTimeout(r, 0));
  expect(c.hasEnded()).toBe(true);
  f.receive(new ArrayBuffer(1));
  await new Promise<void>((r) => setTimeout(r, 0));
  expect(f.options.failed).toHaveBeenCalledOnce();
  expect(f.surface.dispose).toHaveBeenCalledOnce();
});
it("EOF cancels pending input without clearing the final surface or reporting a transport failure", async () => {
  const f = fixture();
  let observedEnd = () => {};
  const end = new Promise<void>((r) => {
    observedEnd = r;
  });
  f.options.observed = (b) => {
    if (b.kind === "end") observedEnd();
  };
  let rejectInput: (error: Error) => void = () => {};
  f.rpc.input.mockImplementation(
    () =>
      new Promise<void>((_, reject) => {
        rejectInput = reject;
      }),
  );
  let rejectFailure: (error: Error) => void = () => {};
  const failed = new Promise<never>((_, reject) => {
    rejectFailure = reject;
  });
  const c = await prepareWorkspaceTerminal({
    ...f.options,
    input: true,
    failed: (error) => {
      f.options.failed(error);
      rejectFailure(error);
    },
  });
  for (let i = 0; i < 3; i++) f.send(i);
  await c.ready;
  f.input("pending");
  expect(f.rpc.input).toHaveBeenCalledOnce();
  f.send(3); // Real state-only marker, not codec-only checkpoint.
  const eof = frame(3);
  new Uint8Array(eof)[7] = 7;
  new DataView(eof).setUint32(2, 4, true);
  f.receive(eof);
  await Promise.race([end, failed]);
  await new Promise<void>((r) => queueMicrotask(r));
  expect(f.surface.finishParsed).toHaveBeenCalledOnce();
  expect(f.surface.dispose).not.toHaveBeenCalled();
  expect(f.options.failed).not.toHaveBeenCalled();
  rejectInput(new Error("retired native result"));
  await new Promise<void>((r) => setTimeout(r, 0));
  f.input("late");
  expect(f.rpc.input).toHaveBeenCalledOnce();
  expect(f.surface.dispose).not.toHaveBeenCalled();
  c.dispose();
});
it("uncertain input timeout stops the generation and never resends", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  try {
    const f = fixture();
    f.rpc.input.mockImplementation(() => new Promise(() => {}));
    const c = await prepareWorkspaceTerminal({ ...f.options, input: true });
    f.send(0);
    f.send(1);
    f.send(2);
    await c.ready;
    f.input("one");
    await vi.advanceTimersByTimeAsync(1000);
    f.input("two");
    expect(f.rpc.input).toHaveBeenCalledOnce();
    expect(f.rpc.detach).toHaveBeenCalledOnce();
    expect(f.options.failed).toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
});

it("invalid document or session identity is rejected before native prepare", async () => {
  for (const bad of [
    { nonce: "bad" },
    { session: "01" },
    { session: "0" },
    { session: "18446744073709551616" },
  ]) {
    const f = fixture();
    await expect(
      prepareWorkspaceTerminal({ ...f.options, ...bad }),
    ).rejects.toThrow();
    expect(f.rpc.prepare).not.toHaveBeenCalled();
  }
});

it("suspend synchronously releases input and renderer but shares one native receipt", async () => {
  const f = fixture();
  const c = await prepareWorkspaceTerminal({ ...f.options, input: true });
  f.send(0);
  f.send(1);
  f.send(2);
  await c.ready;
  let release!: () => void;
  f.rpc.detach.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        release = resolve;
      }),
  );
  const receipt = c.suspend();
  c.dispose();
  expect(c.suspend()).toBe(receipt);
  expect(f.rpc.detach).toHaveBeenCalledOnce();
  expect(f.surface.dispose).toHaveBeenCalledOnce();
  f.input("late");
  f.send(0);
  expect(f.rpc.input).not.toHaveBeenCalled();
  let settled = false;
  void receipt.then(() => {
    settled = true;
  });
  await Promise.resolve();
  expect(settled).toBe(false);
  release();
  await receipt;
  expect(settled).toBe(true);
  f.abort.abort();
});
it("a canceled pending prepare waits for native detach before settling", async () => {
  const f = fixture();
  let published!: (value: unknown) => void, release!: () => void;
  f.rpc.prepare.mockImplementation(
    () =>
      new Promise((resolve) => {
        published = resolve;
      }),
  );
  f.rpc.detach.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        release = resolve;
      }),
  );
  const preparing = prepareWorkspaceTerminal(f.options);
  const rejected = expect(preparing).rejects.toThrow("view.retired");
  f.abort.abort();
  published(f.meta);
  await vi.waitFor(() => expect(f.rpc.detach).toHaveBeenCalledOnce());
  expect(f.rpc.connect).not.toHaveBeenCalled();
  release();
  await rejected;
});

it.each([
  { confirmed: true, error: "workspace.denied", succeeds: true },
  { confirmed: false, error: "workspace.denied", succeeds: false },
  { confirmed: true, error: "workspace.unavailable", succeeds: false },
])(
  "EOF suspension requires confirmed End and exact retired-ticket response: %j",
  async ({ confirmed, error, succeeds }) => {
    const f = fixture();
    f.rpc.acknowledge.mockImplementation(async (...args: unknown[]) => {
      if (!confirmed && args[2] === 4) await new Promise<void>(() => {});
    });
    const c = await prepareWorkspaceTerminal(f.options);
    for (let i = 0; i < 3; i++) f.send(i);
    await c.ready;
    f.send(3);
    const eof = frame(3);
    new Uint8Array(eof)[7] = 7;
    new DataView(eof).setUint32(2, 4, true);
    f.receive(eof);
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(c.hasEnded()).toBe(true);
    expect(f.rpc.acknowledge).toHaveBeenCalledWith(
      f.options.nonce,
      f.meta.ticket,
      4,
    );
    f.rpc.detach.mockRejectedValue(error);
    if (succeeds) await expect(c.suspend()).resolves.toBeUndefined();
    else await expect(c.suspend()).rejects.toThrow("terminal.detach_failed");
    expect(f.surface.dispose).toHaveBeenCalledOnce();
  },
);

it("link opening keeps the original page, needs presentation but not input, survives EOF and rejects retired views", async () => {
  const f = fixture();
  const originalNonce = f.options.nonce;
  let reachedEnd = () => {};
  const ended = new Promise<void>((resolve) => {
    reachedEnd = resolve;
  });
  f.options.observed = (b) => {
    if (b.kind === "end") reachedEnd();
  };
  let releaseSurface = () => {};
  const surfaceGate = new Promise<void>((resolve) => {
    releaseSurface = resolve;
  });
  f.renderer.create.mockImplementationOnce(async () => {
    await surfaceGate;
    return f.surface;
  });
  const c = await prepareWorkspaceTerminal(f.options);
  f.send(0);
  f.send(1);
  f.send(2);
  await vi.waitFor(() => expect(f.renderer.create).toHaveBeenCalledOnce());
  const open = f.renderer.create.mock.calls[0]?.[3].openLink;
  if (!open) throw new Error("missing link callback");
  await expect(open("https://example.test/")).rejects.toThrow("view.retired");
  releaseSurface();
  await c.ready;
  f.options.nonce = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
  await open("https://example.test/");
  expect(f.rpc.openLink).toHaveBeenCalledExactlyOnceWith(
    originalNonce,
    "https://example.test/",
  );
  f.send(3);
  const eof = frame(3);
  new Uint8Array(eof)[7] = 7;
  new DataView(eof).setUint32(2, 4, true);
  f.receive(eof);
  await ended;
  await open("https://example.test/final");
  expect(f.rpc.openLink).toHaveBeenCalledTimes(2);
  expect(f.rpc.input).not.toHaveBeenCalled();
  expect(f.rpc.enableInput).not.toHaveBeenCalled();
  c.dispose();
  await expect(open("https://example.test/late")).rejects.toThrow(
    "view.retired",
  );
  expect(f.rpc.openLink).toHaveBeenCalledTimes(2);
});
