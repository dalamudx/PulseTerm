import { Channel } from "@tauri-apps/api/core";
import { afterEach, expect, it, vi } from "vitest";

type Frame = { index: number; message: number } | { index: number; end: true };

afterEach(() => vi.unstubAllGlobals());

function fixture(failAt: number) {
  let receiver: ((frame: Frame) => void) | undefined;
  const pendingErrors: VoidFunction[] = [];
  const received: number[] = [];
  const failure = new Error("fixed channel handler failure");
  const unregister = vi.fn();
  vi.stubGlobal("window", {
    __TAURI_INTERNALS__: {
      transformCallback(callback: (frame: Frame) => void) {
        receiver = callback;
        return 17;
      },
      unregisterCallback: unregister,
    },
  });
  // Keep the SDK's asynchronous error observable without leaking an uncaught test error.
  vi.stubGlobal("queueMicrotask", (callback: VoidFunction) =>
    pendingErrors.push(callback),
  );
  const channel = new Channel<number>((value) => {
    received.push(value);
    if (value === failAt) throw failure;
  });
  return {
    channel,
    received,
    pendingErrors,
    failure,
    unregister,
    deliver(frame: Frame) {
      if (!receiver) throw new Error("channel callback not registered");
      receiver(frame);
    },
  };
}

it("actual SDK continues ordered messages and releases End after a handler throws", () => {
  const f = fixture(0);
  expect(() => f.deliver({ index: 0, message: 0 })).not.toThrow();
  f.deliver({ index: 1, message: 1 });
  f.deliver({ index: 2, end: true });
  expect(f.received).toEqual([0, 1]);
  expect(f.pendingErrors).toHaveLength(1);
  expect(() => f.pendingErrors[0]?.()).toThrow(f.failure);
  expect(f.unregister).toHaveBeenCalledExactlyOnceWith(f.channel.id);
});

it("actual SDK drains out-of-order messages through a queued handler failure", () => {
  const f = fixture(1);
  f.deliver({ index: 2, message: 2 });
  f.deliver({ index: 1, message: 1 });
  f.deliver({ index: 3, end: true });
  expect(f.received).toEqual([]);
  expect(f.unregister).not.toHaveBeenCalled();
  expect(() => f.deliver({ index: 0, message: 0 })).not.toThrow();
  expect(f.received).toEqual([0, 1, 2]);
  expect(f.pendingErrors).toHaveLength(1);
  expect(() => f.pendingErrors[0]?.()).toThrow(f.failure);
  expect(f.unregister).toHaveBeenCalledExactlyOnceWith(f.channel.id);
});
