import { expect, it, vi } from "vitest";
import { GenerationViewHost } from "../terminal/view-host";
import { ViewThemePaintGuard } from "./view-paint-guard";

function setup() {
  const abort = new AbortController();
  const host = new GenerationViewHost(abort.signal, vi.fn());
  return { host, abort, scope: host.begin(9007199254740993n) };
}
function image(value = 1) {
  return JSON.stringify({
    version: 1,
    appearance: "dark",
    neutral: "slate",
    accent: "cyan",
    terminal: {
      foreground: [255, 255, 255],
      background: [0, 0, 0],
      ansi: Array.from({ length: 16 }, () => [value, 128, 255]),
    },
    overrides: [],
    lowForegroundContrast: false,
  });
}
function surface() {
  return { apply: vi.fn(), dispose: vi.fn() };
}
it("authority revocation removes old pixels but keeps stale metadata and healthy scope", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  const before = g.snapshot().data;
  g.revoke();
  g.revoke();
  expect(a.dispose).toHaveBeenCalledTimes(1);
  expect(g.snapshot()).toEqual({ data: before, current: false });
  expect(s.scope.isCurrent()).toBe(true);
  const b = surface();
  expect(
    await g.refresh(
      async () => image(2),
      () => b,
    ),
  ).toBe("applied");
  s.host.dispose();
});
it("authority revocation clears published resource but retains busy slot until reader exits", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface(),
    job = pending();
  await g.refresh(
    async () => image(),
    () => a,
  );
  let signal: AbortSignal | undefined;
  const read = g.refresh(
    (v) => {
      signal = v;
      return job.promise;
    },
    () => surface(),
  );
  g.revoke();
  expect(signal?.aborted).toBe(true);
  expect(a.dispose).toHaveBeenCalledTimes(1);
  const next = vi.fn(async () => image(3));
  await expect(g.refresh(next, () => surface())).rejects.toThrow("busy");
  expect(next).not.toHaveBeenCalled();
  job.resolve(image(2));
  expect(await read).toBe("stale");
  expect(await g.refresh(next, () => surface())).toBe("applied");
  s.host.dispose();
});
it("revocation cleanup reentrancy never disposes twice or closes healthy scope", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  a.dispose.mockImplementation(() => g.revoke());
  g.revoke();
  expect(a.dispose).toHaveBeenCalledTimes(1);
  expect(s.scope.isCurrent()).toBe(true);
  expect(g.snapshot().current).toBe(false);
  s.host.dispose();
});
it("revocation during apply releases the tentative surface and cannot publish it", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface();
  a.apply.mockImplementation(() => g.revoke());
  expect(
    await g.refresh(
      async () => image(),
      () => a,
    ),
  ).toBe("stale");
  expect(a.dispose).toHaveBeenCalledTimes(1);
  expect(g.snapshot().data).toBeUndefined();
  expect(s.scope.isCurrent()).toBe(true);
  s.host.dispose();
});
it("revocation cleanup failure closes only owner scope and masks private errors", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  a.dispose.mockImplementation(() => {
    throw new Error("PRIVATE source");
  });
  expect(() => g.revoke()).toThrow("theme.paint.cleanup_failed");
  expect(s.scope.isCurrent()).toBe(false);
  expect(() => s.host.begin(9007199254740995n)).toThrow("view.closed");
  const healthy = setup();
  g.revoke();
  expect(healthy.scope.isCurrent()).toBe(true);
  expect(a.dispose).toHaveBeenCalledTimes(1);
  healthy.host.dispose();
  s.host.dispose();
});

function pending() {
  let resolve: ((v: unknown) => void) | undefined;
  let reject: ((e: Error) => void) | undefined;
  const promise = new Promise<unknown>((ok, no) => {
    resolve = ok;
    reject = no;
  });
  return {
    promise,
    resolve: (v: unknown) => {
      if (!resolve) throw new Error("missing resolver");
      resolve(v);
    },
    reject: (e: Error) => {
      if (!reject) throw new Error("missing rejector");
      reject(e);
    },
  };
}
it("reuses backend bigint generation and disposes only this view's surface", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface();
  expect(
    await g.refresh(
      async () => image(),
      () => a,
    ),
  ).toBe("applied");
  expect(a.apply).toHaveBeenCalledTimes(1);
  expect(g.snapshot().current).toBe(true);
  s.host.begin(9007199254740994n);
  expect(a.dispose).toHaveBeenCalledTimes(1);
  expect(g.snapshot().data).toBeUndefined();
  await expect(
    g.refresh(
      async () => image(),
      () => surface(),
    ),
  ).rejects.toThrow("view_retired");
});
it("source switch retains read slot until exit and rejects A→B→A late result", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    job = pending(),
    a = surface();
  let signal: AbortSignal | undefined;
  const result = g.refresh(
    (sig) => {
      signal = sig;
      return job.promise;
    },
    () => a,
  );
  g.invalidate();
  g.invalidate();
  expect(signal?.aborted).toBe(true);
  const next = vi.fn(async () => image(2));
  await expect(g.refresh(next, () => surface())).rejects.toThrow("busy");
  expect(next).not.toHaveBeenCalled();
  job.resolve(image());
  expect(await result).toBe("stale");
  expect(a.apply).not.toHaveBeenCalled();
  expect(g.snapshot().data).toBeUndefined();
  expect(await g.refresh(next, () => surface())).toBe("applied");
  s.host.dispose();
});
it("scope retirement rejects late bytes before decoding or preparing", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    job = pending(),
    prepare = vi.fn(() => surface());
  const result = g.refresh(() => job.promise, prepare);
  s.host.retire(s.scope.generation);
  const replacement = s.host.begin(9007199254740994n);
  job.resolve("PRIVATE invalid JSON");
  expect(await result).toBe("stale");
  expect(prepare).not.toHaveBeenCalled();
  expect(replacement.isCurrent()).toBe(true);
  s.host.dispose();
});
it("parent abort closes the image but doesn't prematurely settle reader ownership", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    job = pending();
  const result = g.refresh(
    () => job.promise,
    () => surface(),
  );
  s.abort.abort();
  let finished = false;
  void result.then(() => {
    finished = true;
  });
  await Promise.resolve();
  expect(finished).toBe(false);
  job.resolve(image());
  expect(await result).toBe("stale");
  expect(g.snapshot().current).toBe(false);
});
it("bad refresh preserves last-good resource and never reaches prepare", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  const before = g.snapshot().data;
  g.invalidate();
  const prepare = vi.fn(() => surface());
  await expect(g.refresh(async () => "bad", prepare)).rejects.toThrow(
    "theme.paint.invalid",
  );
  expect(prepare).not.toHaveBeenCalled();
  expect(g.snapshot().data).toBe(before);
  expect(g.snapshot().current).toBe(false);
  expect(a.dispose).not.toHaveBeenCalled();
  s.host.dispose();
});
it("successful replacement cleans old once without growing resource ownership", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface(),
    b = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  g.invalidate();
  await g.refresh(
    async () => image(2),
    () => b,
  );
  expect(a.dispose).toHaveBeenCalledTimes(1);
  expect(g.snapshot().current).toBe(true);
  g.dispose();
  g.dispose();
  s.host.dispose();
  expect(a.dispose).toHaveBeenCalledTimes(1);
  expect(b.dispose).toHaveBeenCalledTimes(1);
});
it("reentrant invalidation during prepare prevents all application", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface();
  expect(
    await g.refresh(
      async () => image(),
      () => {
        g.invalidate();
        return a;
      },
    ),
  ).toBe("stale");
  expect(a.apply).not.toHaveBeenCalled();
  expect(a.dispose).toHaveBeenCalledTimes(1);
  s.host.dispose();
});
it("reentrant invalidation during apply cleans tentative surface and does not publish", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface();
  a.apply.mockImplementation(() => g.invalidate());
  expect(
    await g.refresh(
      async () => image(),
      () => a,
    ),
  ).toBe("stale");
  expect(a.dispose).toHaveBeenCalledTimes(1);
  expect(g.snapshot().data).toBeUndefined();
  s.host.dispose();
});
it("partial apply failure is cleaned, redacted and preserves old current", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface(),
    b = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  b.apply.mockImplementation(() => {
    throw new Error("PRIVATE path");
  });
  await expect(
    g.refresh(
      async () => image(2),
      () => b,
    ),
  ).rejects.toThrow("theme.paint.apply_failed");
  expect(b.dispose).toHaveBeenCalledTimes(1);
  expect(a.dispose).not.toHaveBeenCalled();
  expect(g.snapshot().current).toBe(true);
  s.host.dispose();
});
it("reader and prepare failures are redacted and don't retire healthy view", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope);
  await expect(
    g.refresh(
      async () => {
        throw new Error("PRIVATE path");
      },
      () => surface(),
    ),
  ).rejects.toThrow("theme.paint.read_failed");
  await expect(
    g.refresh(
      async () => image(),
      () => {
        throw new Error("PRIVATE path");
      },
    ),
  ).rejects.toThrow("theme.paint.prepare_failed");
  expect(s.scope.isCurrent()).toBe(true);
  s.host.dispose();
});
it("late reader failure cannot retire replacement view", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    job = pending();
  const result = g.refresh(
    () => job.promise,
    () => surface(),
  );
  s.host.begin(9007199254740994n);
  job.reject(new Error("PRIVATE path"));
  expect(await result).toBe("stale");
  const fresh = s.host.begin(9007199254740995n);
  expect(fresh.isCurrent()).toBe(true);
  s.host.dispose();
});
it("throwing old cleanup closes only its owner view and attempts all surfaces", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface(),
    b = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  a.dispose.mockImplementation(() => {
    throw new Error("PRIVATE path");
  });
  await expect(
    g.refresh(
      async () => image(2),
      () => b,
    ),
  ).rejects.toThrow("theme.paint.cleanup_failed");
  expect(a.dispose).toHaveBeenCalledTimes(1);
  expect(b.dispose).toHaveBeenCalledTimes(1);
  expect(s.scope.isCurrent()).toBe(false);
});
it("old cleanup reentrancy cannot publish after source invalidation", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface(),
    b = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  const before = g.snapshot().data;
  a.dispose.mockImplementation(() => g.invalidate());
  expect(
    await g.refresh(
      async () => image(2),
      () => b,
    ),
  ).toBe("stale");
  expect(g.snapshot().current).toBe(false);
  expect(g.snapshot().data).toBe(before);
  expect(b.dispose).toHaveBeenCalledTimes(1);
  s.host.dispose();
});
it("old cleanup retirement cannot repopulate a closed image", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface(),
    b = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  a.dispose.mockImplementation(() => s.host.retire(s.scope.generation));
  expect(
    await g.refresh(
      async () => image(2),
      () => b,
    ),
  ).toBe("stale");
  expect(g.snapshot().data).toBeUndefined();
  expect(b.dispose).toHaveBeenCalledTimes(1);
  s.host.dispose();
});
it("reusing active or disposed surface cannot mutate old good or enable a fake paint", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    a = surface(),
    b = surface();
  await g.refresh(
    async () => image(),
    () => a,
  );
  await expect(
    g.refresh(
      async () => image(2),
      () => a,
    ),
  ).rejects.toThrow("resource_reused");
  expect(a.apply).toHaveBeenCalledTimes(1);
  expect(a.dispose).not.toHaveBeenCalled();
  expect(g.snapshot().current).toBe(true);
  await g.refresh(
    async () => image(2),
    () => b,
  );
  await expect(
    g.refresh(
      async () => image(),
      () => a,
    ),
  ).rejects.toThrow("resource_reused");
  expect(b.dispose).not.toHaveBeenCalled();
  expect(g.snapshot().current).toBe(true);
  s.host.dispose();
});
it("catch-branch cleanup failure also retires the view and skips no resources", async () => {
  const s = setup(),
    g = new ViewThemePaintGuard(s.scope),
    resource = surface(),
    other = vi.fn();
  s.scope.own({ dispose: other });
  const current = s.scope.isCurrent.bind(s.scope);
  resource.dispose.mockImplementation(() => {
    throw new Error("PRIVATE");
  });
  const prepare = () => {
    let once = true;
    vi.spyOn(s.scope, "isCurrent").mockImplementation(() => {
      if (once) {
        once = false;
        throw new Error("PRIVATE");
      }
      return current();
    });
    return resource;
  };
  await expect(g.refresh(async () => image(), prepare)).rejects.toThrow(
    "theme.paint.cleanup_failed",
  );
  expect(s.scope.signal.aborted).toBe(true);
  expect(g.snapshot().data).toBeUndefined();
  expect(resource.dispose).toHaveBeenCalledTimes(1);
  expect(other).toHaveBeenCalledTimes(1);
  s.host.dispose();
});
