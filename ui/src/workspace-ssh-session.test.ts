import { afterEach, describe, expect, it, vi } from "vitest";
import type { HostRpc } from "./workspace-hosts";
import {
  connectSavedPrivateKeySsh,
  connectSavedSsh,
} from "./workspace-ssh-session";

const nonce = "11111111-1111-4111-8111-111111111111",
  token = "22222222-2222-4222-8222-222222222222";
const selection = {
  host: "11111111-1111-7111-8111-111111111111",
  identity: "22222222-2222-7222-8222-222222222222",
};
function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});
describe.each([
  { label: "password", connect: connectSavedSsh, privateKey: false },
  {
    label: "private key",
    connect: connectSavedPrivateKeySsh,
    privateKey: true,
  },
])("saved $label SSH session gesture", ({ connect, privateKey }) => {
  it("sends only saved selection and one ticket chain, preserving u64 session ids", async () => {
    const rpc = vi.fn(async (command: string) =>
      command.endsWith("begin")
        ? { version: 1, token }
        : command.endsWith("start")
          ? "9007199254740993"
          : null,
    );
    const id = await connect(
      nonce,
      selection,
      new AbortController().signal,
      () => true,
      rpc as HostRpc,
    );
    expect(id).toBe("9007199254740993");
    expect(rpc.mock.calls.map(([cmd]) => cmd)).toEqual([
      `workspace_ssh${privateKey ? "_key" : ""}_session_begin`,
      `workspace_ssh${privateKey ? "_key" : ""}_session_prepare`,
      "workspace_ssh_session_start",
    ]);
    expect(rpc).toHaveBeenNthCalledWith(
      1,
      `workspace_ssh${privateKey ? "_key" : ""}_session_begin`,
      {
        nonce,
        selection,
      },
    );
    expect(rpc).toHaveBeenNthCalledWith(3, "workspace_ssh_session_start", {
      nonce,
      token,
    });
  });
  it("cancels a late begin ticket without preparing it", async () => {
    const begin = deferred<unknown>();
    const rpc = vi.fn(async (command: string) =>
      command.endsWith("begin") ? begin.promise : null,
    );
    const abort = new AbortController();
    const result = connect(
      nonce,
      selection,
      abort.signal,
      () => true,
      rpc as HostRpc,
    );
    abort.abort();
    await expect(result).rejects.toThrow("cancelled");
    begin.resolve({ version: 1, token });
    await vi.waitFor(() =>
      expect(rpc).toHaveBeenCalledWith("workspace_ssh_session_cancel", {
        nonce,
        token,
      }),
    );
    expect(rpc).toHaveBeenCalledTimes(2);
  });
  it("rejects retired source results without starting", async () => {
    const prepare = deferred<unknown>();
    let current = true;
    const rpc = vi.fn(async (command: string) =>
      command.endsWith("begin")
        ? { version: 1, token }
        : command.endsWith("prepare")
          ? prepare.promise
          : null,
    );
    const result = connect(
      nonce,
      selection,
      new AbortController().signal,
      () => current,
      rpc as HostRpc,
    );
    await vi.waitFor(() => expect(rpc).toHaveBeenCalledTimes(2));
    current = false;
    prepare.resolve(null);
    await expect(result).rejects.toThrow("cancelled");
    expect(rpc).toHaveBeenCalledTimes(3);
    expect(rpc).toHaveBeenLastCalledWith("workspace_ssh_session_cancel", {
      nonce,
      token,
    });
  });
  it("never closes a root session or retries after aborting start", async () => {
    const start = deferred<unknown>();
    const abort = new AbortController();
    const rpc = vi.fn(async (command: string) =>
      command.endsWith("begin")
        ? { version: 1, token }
        : command.endsWith("start")
          ? start.promise
          : null,
    );
    const result = connect(
      nonce,
      selection,
      abort.signal,
      () => true,
      rpc as HostRpc,
    );
    await vi.waitFor(() => expect(rpc).toHaveBeenCalledTimes(3));
    abort.abort();
    await expect(result).rejects.toThrow("cancelled");
    start.resolve("1");
    await Promise.resolve();
    expect(rpc.mock.calls.map(([cmd]) => cmd)).toEqual([
      `workspace_ssh${privateKey ? "_key" : ""}_session_begin`,
      `workspace_ssh${privateKey ? "_key" : ""}_session_prepare`,
      "workspace_ssh_session_start",
      "workspace_ssh_session_cancel",
    ]);
  });
  it.each([
    { version: 2, token },
    { version: 1, token, permission: true },
    { version: 1, token: selection.host },
  ])("rejects unsupported or authority-bearing tickets %j", async (ticket) => {
    const rpc = vi.fn(async () => ticket);
    await expect(
      connect(
        nonce,
        selection,
        new AbortController().signal,
        () => true,
        rpc as HostRpc,
      ),
    ).rejects.toThrow("protocol");
    expect(rpc).toHaveBeenCalledTimes(1);
  });
  it("cancels known tickets if prepare returns unexpected data", async () => {
    const rpc = vi.fn(async (command: string) =>
      command.endsWith("begin") ? { version: 1, token } : true,
    );
    await expect(
      connect(
        nonce,
        selection,
        new AbortController().signal,
        () => true,
        rpc as HostRpc,
      ),
    ).rejects.toThrow("protocol");
    expect(rpc).toHaveBeenCalledTimes(3);
  });
  it("rejects a late start even if the timer callback has not run", async () => {
    const clock = vi.spyOn(performance, "now").mockReturnValue(0);
    const rpc = vi.fn(async (command: string) => {
      if (command.endsWith("begin")) return { version: 1, token };
      if (command.endsWith("start")) {
        clock.mockReturnValue(5001);
        return "1";
      }
      return null;
    });
    await expect(
      connect(
        nonce,
        selection,
        new AbortController().signal,
        () => true,
        rpc as HostRpc,
      ),
    ).rejects.toThrow("uncertain");
    expect(rpc).toHaveBeenCalledTimes(4);
  });
  it("rejects raw connection or secret parameters before IPC", async () => {
    const rpc = vi.fn(async () => null);
    const untrusted = { ...selection, address: "not accepted" };
    await expect(
      connect(
        nonce,
        untrusted,
        new AbortController().signal,
        () => true,
        rpc as HostRpc,
      ),
    ).rejects.toThrow("denied");
    expect(rpc).not.toHaveBeenCalled();
  });
});
