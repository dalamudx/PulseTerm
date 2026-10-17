import { expect, it, vi } from "vitest";
import { type PaneDependencies, WorkspacePane } from "./workspace-pane";
import type { TerminalConnectionOptions } from "./workspace-terminal-connection";

function fixture(observeVisibility?: PaneDependencies["observeVisibility"]) {
  const parent = new AbortController(),
    phase = vi.fn(),
    dispose = vi.fn(),
    announce = vi.fn(async () => {}),
    stopObserve = vi.fn(),
    fitDispose = vi.fn();
  const prepared: TerminalConnectionOptions[] = [];
  let callbacks: Parameters<PaneDependencies["fit"]>[1] | undefined;
  const connection = {
    ready: Promise.resolve(),
    dispose,
    suspend: vi.fn(async () => {
      dispose();
    }),
    announceInputReady: announce,
    focusOutput: vi.fn(() => true),
    geometry: () => ({ columns: 40, rows: 12 }),
    measuredDimensions: () => ({ columns: 40, rows: 12 }),
    pauseForResize: vi.fn(),
    resize: vi.fn(async () => {}),
    watchGeometry: vi.fn(() => () => {}),
  };
  const dependencies: PaneDependencies = {
    prepare: vi.fn(async (value) => {
      prepared.push(value);
      return connection;
    }),
    fit: vi.fn((_view, next) => {
      callbacks = next;
      return { observe: vi.fn(() => next.settled()), dispose: fitDispose };
    }),
    observe: vi.fn(() => stopObserve),
    observeVisibility,
  };
  const pane = new WorkspacePane(
    {
      nonce: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      session: "1",
      element: {} as HTMLElement,
      input: true,
      signal: parent.signal,
      phase,
    },
    dependencies,
  );
  return {
    parent,
    phase,
    dispose,
    announce,
    stopObserve,
    fitDispose,
    connection,
    dependencies,
    pane,
    prepared,
    get callbacks() {
      return callbacks;
    },
  };
}
it("display focus only after ready and never reinstalls native input", async () => {
  const f = fixture();
  expect(f.pane.focusOutput()).toBe(false);
  await f.pane.mount();
  expect(f.pane.focusOutput()).toBe(true);
  expect(f.announce).toHaveBeenCalledOnce();
  f.callbacks?.paused();
  expect(f.pane.focusOutput()).toBe(false);
  f.pane.dispose();
  expect(f.pane.focusOutput()).toBe(false);
  expect(f.connection.focusOutput).toHaveBeenCalledOnce();
});
it("mount coalesces, disposal detaches once without native close", async () => {
  const f = fixture(),
    first = f.pane.mount();
  expect(f.pane.mount()).toBe(first);
  await first;
  expect(f.dependencies.prepare).toHaveBeenCalledTimes(1);
  expect(f.pane.state).toBe("ready");
  expect(f.announce).toHaveBeenCalledOnce();
  f.pane.dispose();
  f.pane.dispose();
  expect(f.dispose).toHaveBeenCalledOnce();
  expect(f.stopObserve).toHaveBeenCalledOnce();
  expect(f.fitDispose).toHaveBeenCalledOnce();
  expect(f.pane.state).toBe("disposed");
});
it("two panes own distinct hosts even with equal backend Begin generation", async () => {
  const a = fixture(),
    b = fixture();
  await Promise.all([a.pane.mount(), b.pane.mount()]);
  expect(a.prepared[0]?.host).not.toBe(b.prepared[0]?.host);
  const first = a.prepared[0]?.host.begin(1n),
    second = b.prepared[0]?.host.begin(1n);
  a.pane.dispose();
  expect(first?.isCurrent()).toBe(false);
  expect(second?.isCurrent()).toBe(true);
  b.pane.dispose();
});
it("late prepare on retired page disposes receipt without fit or ready publication", async () => {
  const f = fixture();
  let resolve!: (v: typeof f.connection) => void;
  f.dependencies.prepare = vi.fn(async (value) => {
    expect(value.signal.aborted).toBe(false);
    return new Promise<typeof f.connection>((r) => {
      resolve = r;
    });
  });
  const mounted = f.pane.mount();
  f.parent.abort();
  resolve(f.connection);
  await mounted;
  expect(f.dispose).toHaveBeenCalledOnce();
  expect(f.dependencies.fit).not.toHaveBeenCalled();
  expect(f.phase).not.toHaveBeenCalledWith("ready", undefined);
});
it("Ended capture renders but never starts fit or input announcement", async () => {
  const f = fixture();
  f.dependencies.prepare = vi.fn(async (options) => {
    options.observed?.({
      kind: "end",
      generation: 1n,
      columns: 40,
      rows: 12,
      revision: 1n,
      sourceBytes: 0n,
      utf8Bytes: 0n,
      digest: "x",
    });
    return f.connection;
  });
  await f.pane.mount();
  expect(f.pane.state).toBe("ended");
  expect(f.dependencies.fit).not.toHaveBeenCalled();
  expect(f.announce).not.toHaveBeenCalled();
  f.pane.dispose();
});
it("known native resize recaptures; stale callbacks cannot end/fail replacement", async () => {
  const f = fixture();
  await f.pane.mount();
  const old = f.callbacks,
    options = f.prepared[0];
  old?.paused();
  expect(f.pane.state).toBe("resizing");
  old?.replaced();
  await f.pane.mount();
  expect(f.dependencies.prepare).toHaveBeenCalledTimes(2);
  const count = f.dispose.mock.calls.length;
  old?.failed(Error("old"));
  options?.observed?.({
    kind: "end",
    generation: 1n,
    columns: 40,
    rows: 12,
    revision: 1n,
    sourceBytes: 0n,
    utf8Bytes: 0n,
    digest: "x",
  });
  options?.failed(Error("old"));
  expect(f.dispose).toHaveBeenCalledTimes(count);
  expect(f.pane.state).not.toBe("ended");
  f.pane.dispose();
});
it("uncertain resize fails without automatic prepare or native close", async () => {
  const f = fixture();
  await f.pane.mount();
  f.callbacks?.failed(Error("uncertain"));
  expect(f.pane.state).toBe("failed");
  expect(f.dependencies.prepare).toHaveBeenCalledTimes(1);
  expect(f.dispose).toHaveBeenCalledOnce();
  f.pane.dispose();
});
it("pre-retired lifecycle cannot allocate a connection or observer", async () => {
  const f = fixture();
  f.parent.abort();
  await expect(f.pane.mount()).rejects.toThrow("view.retired");
  expect(f.dependencies.prepare).not.toHaveBeenCalled();
  expect(f.dependencies.observe).not.toHaveBeenCalled();
});
it("hidden views stop work synchronously and wait for detach before recapturing", async () => {
  const f = fixture();
  await f.pane.mount();
  const old = f.prepared[0];
  let release!: () => void;
  f.connection.suspend.mockImplementation(() => {
    f.dispose();
    return new Promise<void>((resolve) => {
      release = resolve;
    });
  });
  const paused = f.pane.setVisible(false);
  expect(f.pane.state).toBe("paused");
  expect(f.pane.focusOutput()).toBe(false);
  expect(f.stopObserve).toHaveBeenCalledOnce();
  expect(f.fitDispose).toHaveBeenCalledOnce();
  expect(old?.signal.aborted).toBe(true);
  const shown = f.pane.setVisible(true);
  expect(f.prepared).toHaveLength(1);
  release();
  await Promise.all([paused, shown]);
  expect(f.prepared).toHaveLength(2);
  expect(f.prepared[1]?.signal.aborted).toBe(false);
  expect(f.pane.state).toBe("ready");
  f.pane.dispose();
});
it("document and layout visibility both gate attachment and unsubscribe on disposal", async () => {
  let page = (_visible: boolean) => {};
  const stop = vi.fn();
  const f = fixture((_element, notify) => {
    page = notify;
    notify(false);
    return stop;
  });
  await f.pane.mount();
  expect(f.prepared).toHaveLength(0);
  expect(f.pane.state).toBe("paused");
  page(true);
  await f.pane.mount();
  expect(f.prepared).toHaveLength(1);
  await f.pane.setVisible(false);
  page(false);
  page(true);
  expect(f.prepared).toHaveLength(1);
  await f.pane.setVisible(true);
  expect(f.prepared).toHaveLength(2);
  f.pane.dispose();
  expect(stop).toHaveBeenCalledOnce();
  page(false);
  page(true);
  expect(f.prepared).toHaveLength(2);
});
it("hide and reveal during prepare dispose the late view before one fresh capture", async () => {
  const f = fixture();
  let resolve!: (view: typeof f.connection) => void;
  f.dependencies.prepare = vi.fn(async (options) => {
    f.prepared.push(options);
    if (f.prepared.length === 1)
      return new Promise<typeof f.connection>((r) => {
        resolve = r;
      });
    return f.connection;
  });
  const mounted = f.pane.mount();
  const paused = f.pane.setVisible(false);
  const shown = f.pane.setVisible(true);
  expect(f.prepared[0]?.signal.aborted).toBe(true);
  expect(f.prepared).toHaveLength(1);
  resolve(f.connection);
  await Promise.all([mounted, paused, shown]);
  expect(f.prepared).toHaveLength(2);
  expect(f.pane.state).toBe("ready");
  const count = f.phase.mock.calls.length;
  f.prepared[0]?.failed(Error("stale"));
  expect(f.phase.mock.calls).toHaveLength(count);
  f.pane.dispose();
});
it("detach rejection blocks automatic recovery even after another reveal", async () => {
  const f = fixture();
  await f.pane.mount();
  f.connection.suspend.mockRejectedValue(Error("native refused"));
  await expect(f.pane.setVisible(false)).rejects.toThrow(
    "terminal.detach_failed",
  );
  await f.pane.setVisible(true);
  expect(f.pane.state).toBe("failed");
  expect(f.prepared).toHaveLength(1);
  await expect(f.pane.mount()).rejects.toThrow();
  f.pane.dispose();
});
it("a missing detach receipt fails within its bound without another prepare", async () => {
  vi.useFakeTimers();
  const f = fixture();
  try {
    await f.pane.mount();
    f.connection.suspend.mockImplementation(() => new Promise(() => {}));
    const paused = f.pane.setVisible(false);
    const rejected = expect(paused).rejects.toThrow("terminal.detach_failed");
    await vi.advanceTimersByTimeAsync(1500);
    await rejected;
    await f.pane.setVisible(true);
    expect(f.pane.state).toBe("failed");
    expect(f.prepared).toHaveLength(1);
  } finally {
    f.pane.dispose();
    vi.useRealTimers();
  }
});
it("disposing while hidden prevents any later reattachment", async () => {
  const f = fixture();
  await f.pane.mount();
  await f.pane.setVisible(false);
  f.pane.dispose();
  await f.pane.setVisible(true);
  expect(f.pane.state).toBe("disposed");
  expect(f.prepared).toHaveLength(1);
});

it("failure during prepare rejects mount and never restarts on reveal", async () => {
  const f = fixture();
  f.dependencies.prepare = vi.fn(async (options) => {
    f.prepared.push(options);
    options.failed(Error("terminal.connection_failed"));
    return f.connection;
  });
  await expect(f.pane.mount()).rejects.toThrow("terminal.connection_failed");
  await f.pane.setVisible(false);
  await f.pane.setVisible(true);
  expect(f.prepared).toHaveLength(1);
  expect(f.pane.state).toBe("failed");
  f.pane.dispose();
});
