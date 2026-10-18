import { expect, it, vi } from "vitest";
import {
  bindTerminalDeviceSpacing,
  terminalDeviceSpacing,
} from "./terminal-device-spacing";

function fixture(ratio = 1) {
  const queries: (EventTarget & { media: string })[] = [];
  const view = Object.assign(new EventTarget(), {
    devicePixelRatio: ratio,
    matchMedia: vi.fn((media: string) => {
      const query = Object.assign(new EventTarget(), { media });
      queries.push(query);
      return query as unknown as MediaQueryList;
    }),
  });
  const document = { defaultView: view } as unknown as Document;
  const controller = new AbortController();
  const terminal = { options: { letterSpacing: 2 } };
  let current = true;
  const bind = () =>
    bindTerminalDeviceSpacing(
      terminal,
      document,
      2,
      controller.signal,
      () => current,
    );
  return {
    queries,
    view,
    document,
    controller,
    terminal,
    bind,
    retire: () => {
      current = false;
    },
  };
}

it("keeps user spacing in CSS pixels using the owning document's ratio", () => {
  const f = fixture(2);
  expect(terminalDeviceSpacing(2, f.document)).toBe(4);
  f.view.devicePixelRatio = 1.25;
  expect(terminalDeviceSpacing(2, f.document)).toBe(2.5);
  f.view.devicePixelRatio = Number.NaN;
  expect(terminalDeviceSpacing(2, f.document)).toBe(2);
  f.view.devicePixelRatio = 0;
  expect(terminalDeviceSpacing(2, f.document)).toBe(2);
  expect(terminalDeviceSpacing(2, { defaultView: null } as Document)).toBe(2);
});

it("rearms ratio observation, ignores obsolete media and stops all late work after disposal", () => {
  const f = fixture();
  const dispose = f.bind();
  const first = f.queries.at(-1);
  expect(first?.media).toBe("(resolution: 1dppx)");
  f.view.devicePixelRatio = 2;
  first?.dispatchEvent(new Event("change"));
  expect(f.terminal.options.letterSpacing).toBe(4);
  expect(f.queries.at(-1)?.media).toBe("(resolution: 2dppx)");
  f.view.devicePixelRatio = 3;
  first?.dispatchEvent(new Event("change"));
  expect(f.terminal.options.letterSpacing).toBe(4);
  f.view.dispatchEvent(new Event("resize"));
  expect(f.terminal.options.letterSpacing).toBe(6);
  dispose();
  dispose();
  f.view.devicePixelRatio = 4;
  f.view.dispatchEvent(new Event("resize"));
  f.queries.at(-1)?.dispatchEvent(new Event("change"));
  expect(f.terminal.options.letterSpacing).toBe(6);
});

it("aborted and retired surfaces never mutate a later generation", () => {
  const f = fixture();
  f.bind();
  f.controller.abort();
  f.view.devicePixelRatio = 2;
  f.queries.at(-1)?.dispatchEvent(new Event("change"));
  expect(f.terminal.options.letterSpacing).toBe(2);
  const next = fixture();
  next.bind();
  next.retire();
  next.view.devicePixelRatio = 2;
  next.view.dispatchEvent(new Event("resize"));
  expect(next.terminal.options.letterSpacing).toBe(2);
  const alreadyAborted = fixture(3);
  alreadyAborted.controller.abort();
  alreadyAborted.bind();
  expect(alreadyAborted.terminal.options.letterSpacing).toBe(2);
  expect(alreadyAborted.queries).toHaveLength(0);
});

it("default zero spacing requires no extra device watchers", () => {
  const f = fixture(2);
  bindTerminalDeviceSpacing(
    f.terminal,
    f.document,
    0,
    f.controller.signal,
    () => true,
  )();
  expect(f.view.matchMedia).not.toHaveBeenCalled();
});
