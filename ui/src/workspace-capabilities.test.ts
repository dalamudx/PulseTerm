import { expect, it, vi } from "vitest";
import {
  decodeWorkspaceCapabilities,
  readWorkspaceCapabilities,
} from "./workspace-capabilities";
import {
  workspaceCommandIds,
  workspacePaletteOnlyCommandIds,
} from "./workspace-command-catalog";

it("keeps new navigation commands exclusive to the palette", () => {
  const actions = workspacePaletteOnlyCommandIds.map((id) => ({
    ...action(id),
    surfaces: ["palette"],
  }));
  expect(
    decodeWorkspaceCapabilities({ version: 1, actions }).map(
      (value) => value.id,
    ),
  ).toEqual([...workspacePaletteOnlyCommandIds]);
  for (const value of actions) {
    expect(() =>
      decodeWorkspaceCapabilities({
        version: 1,
        actions: [{ ...value, surfaces: ["palette", "keybinding"] }],
      }),
    ).toThrow();
    expect(() =>
      decodeWorkspaceCapabilities({
        version: 1,
        actions: [{ ...value, surfaces: ["cli"] }],
      }),
    ).toThrow();
  }
  expect(() =>
    decodeWorkspaceCapabilities({
      version: 1,
      actions: [{ ...action(), surfaces: ["palette"] }],
    }),
  ).toThrow();
});

function action(id = "settings") {
  return {
    id,
    title: `workspace.commands.${id}`,
    scope: "window",
    risk: id === "close-session" ? "confirm" : "safe",
    surfaces: ["palette", "keybinding"],
    args: {
      type: "object",
      additionalProperties: false,
      title: "EmptyArgs",
      $schema: "https://json-schema.org/draft/2020-12/schema",
      properties: {},
    },
  };
}
function body() {
  return { version: 1, actions: [action()] };
}
it("decodes immutable known display metadata, without handlers or permission fields", () => {
  const source = body();
  const value = decodeWorkspaceCapabilities(source);
  const first = source.actions[0];
  if (!first) throw new Error("missing fixture action");
  first.id = "hosts";
  expect(value).toEqual([
    { id: "settings", title: "workspace.commands.settings", risk: "safe" },
  ]);
  expect(Object.isFrozen(value)).toBe(true);
  expect(Object.isFrozen(value[0])).toBe(true);
  expect(decodeWorkspaceCapabilities({ version: 1, actions: [] })).toEqual([]);
});
it("rejects version, unknown, duplicate and excessive actions", () => {
  for (const source of [
    { version: 2, actions: [] },
    { ...body(), granted: true },
    { version: 1, actions: [action("shell.exec")] },
    { version: 1, actions: [action(), action()] },
    {
      version: 1,
      actions: Array.from({ length: workspaceCommandIds.length + 1 }, () =>
        action(),
      ),
    },
  ])
    expect(() => decodeWorkspaceCapabilities(source)).toThrow();
});
it("rejects foreign scope, risk downgrade, CLI surfaces, code and open or external schemas", () => {
  for (const change of [
    { scope: "global" },
    { risk: "dangerous" },
    { title: "run this code" },
    { surfaces: ["cli"] },
    { handler: "exec" },
    { args: { type: "object", additionalProperties: true } },
    {
      args: {
        type: "object",
        additionalProperties: false,
        $ref: "https://untrusted/schema",
      },
    },
    {
      args: {
        type: "object",
        additionalProperties: false,
        properties: { path: { type: "string" } },
      },
    },
  ])
    expect(() =>
      decodeWorkspaceCapabilities({
        version: 1,
        actions: [{ ...action(), ...change }],
      }),
    ).toThrow();
  expect(() =>
    decodeWorkspaceCapabilities({
      version: 1,
      actions: [{ ...action("close-session"), risk: "safe" }],
    }),
  ).toThrow();
});
it("does not execute accessors or accept inherited fields, sparse arrays and array extra properties", () => {
  const getter = vi.fn();
  const withGetter = { ...action() };
  Object.defineProperty(withGetter, "id", { get: getter });
  expect(() =>
    decodeWorkspaceCapabilities({ version: 1, actions: [withGetter] }),
  ).toThrow();
  expect(getter).not.toHaveBeenCalled();
  expect(() => decodeWorkspaceCapabilities(Object.create(body()))).toThrow();
  expect(() =>
    decodeWorkspaceCapabilities({ version: 1, actions: new Array(1) }),
  ).toThrow();
  const inheritedMap = vi.fn();
  const inherited = [action()];
  Object.setPrototypeOf(inherited, { map: inheritedMap });
  expect(() =>
    decodeWorkspaceCapabilities({ version: 1, actions: inherited }),
  ).toThrow();
  expect(inheritedMap).not.toHaveBeenCalled();
  const extra = Object.assign([action()], { grant: true });
  expect(() =>
    decodeWorkspaceCapabilities({ version: 1, actions: extra }),
  ).toThrow();
});
it("dispatches only a read command with the original nonce", async () => {
  const rpc = vi.fn().mockResolvedValue(body());
  const controller = new AbortController();
  expect(
    await readWorkspaceCapabilities(
      "page",
      controller.signal,
      () => "page",
      rpc,
    ),
  ).toHaveLength(1);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("workspace_cap_list", {
    nonce: "page",
  });
});
it("retired and initially aborted pages never dispatch", async () => {
  const rpc = vi.fn();
  const controller = new AbortController();
  await expect(
    readWorkspaceCapabilities("old", controller.signal, () => "new", rpc),
  ).rejects.toThrow("cancelled");
  controller.abort();
  await expect(
    readWorkspaceCapabilities("page", controller.signal, () => "page", rpc),
  ).rejects.toThrow("cancelled");
  expect(rpc).not.toHaveBeenCalled();
});
it("navigation during the queued dispatch and late reply cannot install old metadata", async () => {
  const controller = new AbortController();
  let nonce = "page";
  const rpc = vi.fn();
  const queued = readWorkspaceCapabilities(
    "page",
    controller.signal,
    () => nonce,
    rpc,
  );
  nonce = "new";
  await expect(queued).rejects.toThrow("cancelled");
  expect(rpc).not.toHaveBeenCalled();
  nonce = "page";
  let resolve: ((value: unknown) => void) | undefined;
  rpc.mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const pending = readWorkspaceCapabilities(
    "page",
    controller.signal,
    () => nonce,
    rpc,
  );
  await Promise.resolve();
  nonce = "new";
  resolve?.(body());
  await expect(pending).rejects.toThrow("cancelled");
});
it("abort and timeout discard late read receipts without retries or write cancellation", async () => {
  vi.useFakeTimers();
  try {
    const controller = new AbortController();
    const rpc = vi.fn().mockImplementation(() => new Promise(() => {}));
    const aborted = readWorkspaceCapabilities(
      "page",
      controller.signal,
      () => "page",
      rpc,
    );
    await Promise.resolve();
    controller.abort();
    await expect(aborted).rejects.toThrow("cancelled");
    expect(rpc).toHaveBeenCalledTimes(1);
    const timeout = readWorkspaceCapabilities(
      "page",
      new AbortController().signal,
      () => "page",
      rpc,
    );
    const rejected = expect(timeout).rejects.toThrow("timeout");
    await vi.advanceTimersByTimeAsync(5000);
    await rejected;
    expect(rpc).toHaveBeenCalledTimes(2);
  } finally {
    vi.useRealTimers();
  }
});
