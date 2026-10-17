import { afterEach, expect, it, vi } from "vitest";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => {
    resolve = yes;
  });
  return { promise, resolve };
}

import {
  type ConfigPreview,
  type checkConfigSource,
  decodeConfigPreview,
} from "./workspace-ssh-config";
import { WorkspaceConfigDialog } from "./workspace-ssh-config-dialog";
import { decodeConfigImport } from "./workspace-ssh-config-import";

class Node extends EventTarget {
  ownerDocument = {};
  children: Node[] = [];
  value = "";
  textContent = "";
  className = "";
  disabled = false;
  open = false;
  focused = false;
  append(...nodes: Node[]) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
  }
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
  focus() {
    this.focused = true;
  }
}
const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const value = () =>
  decodeConfigPreview({
    version: 1,
    token: nonce,
    host: "demo.example",
    port: 22,
    username: "alice",
    identityFileCount: 2,
  });
afterEach(() => vi.unstubAllGlobals());
function fixture(statusCheck?: typeof checkConfigSource) {
  vi.stubGlobal("document", { createElement: () => new Node() });
  const dialog = new Node(),
    form = new Node(),
    alias = new Node(),
    read = new Node(),
    confirm = new Node(),
    fields = new Node(),
    status = new Node(),
    life = new AbortController();
  const label = new Node(),
    prepare = new Node(),
    save = new Node();
  let page: string | undefined = nonce,
    enabled = true;
  const working = vi.fn(),
    restoreFocus = vi.fn();
  const dependencies = {
    status: statusCheck,
    read: vi.fn(async (_nonce: string, _alias: string, _signal: AbortSignal) =>
      value(),
    ),
    confirm: vi.fn(
      async (_nonce: string, _token: string, _signal: AbortSignal) => {},
    ),
    prepare: vi.fn(
      async (
        _nonce: string,
        _token: string,
        name: string,
        _signal: AbortSignal,
      ) =>
        decodeConfigImport({
          ...value(),
          token: "03969d3e-a25e-4a50-a8c4-f1d5c03e6aa6",
          label: name,
        }),
    ),
    save: vi.fn(
      async (_nonce: string, _token: string, _signal: AbortSignal) =>
        "dddddddd-dddd-7ddd-8ddd-dddddddddddd",
    ),
    cancel: vi.fn(async (_nonce: string, _token: string) => {}),
  };
  const controller = new WorkspaceConfigDialog(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      form: form as unknown as HTMLFormElement,
      alias: alias as unknown as HTMLInputElement,
      read: read as unknown as HTMLButtonElement,
      confirm: confirm as unknown as HTMLButtonElement,
      fields: fields as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      importControls: {
        label: label as unknown as HTMLInputElement,
        prepare: prepare as unknown as HTMLButtonElement,
        save: save as unknown as HTMLButtonElement,
      },
      signal: life.signal,
      nonce: () => page,
      enabled: () => enabled,
      working,
      restoreFocus,
    },
    dependencies,
  );
  const input = (text: string) => {
    alias.value = text;
    alias.dispatchEvent(new Event("input"));
  };
  const submit = async () => {
    form.dispatchEvent(new Event("submit", { cancelable: true }));
    await Promise.resolve();
    await Promise.resolve();
  };
  return {
    controller,
    dialog,
    form,
    alias,
    read,
    confirm,
    fields,
    status,
    label,
    prepare,
    save,
    life,
    dependencies,
    working,
    restoreFocus,
    input,
    submit,
    page: (value: string | undefined) => {
      page = value;
    },
    enabled: (value: boolean) => {
      enabled = value;
    },
  };
}
it("unsupported or unconfirmed pages cannot open or call native", () => {
  const f = fixture();
  f.enabled(false);
  f.controller.open();
  expect(f.dialog.open).toBe(false);
  f.enabled(true);
  f.page(undefined);
  f.controller.open();
  expect(f.dialog.open).toBe(false);
  expect(f.dependencies.read).not.toHaveBeenCalled();
});
it("initial read is disabled, alias has focus, empty submissions reach no native", async () => {
  const f = fixture();
  f.controller.open();
  expect(f.alias.focused).toBe(true);
  expect(f.read.disabled).toBe(true);
  expect(f.confirm.disabled).toBe(true);
  await f.submit();
  expect(f.dependencies.read).not.toHaveBeenCalled();
});
it("renders literal metadata, then consumes confirmation exactly once", async () => {
  const f = fixture();
  f.controller.open();
  f.input("Prod");
  f.dependencies.read.mockResolvedValueOnce({ ...value(), username: "<img>" });
  await f.submit();
  expect(f.fields.children[0]?.children[5]?.textContent).toBe("<img>");
  expect(f.confirm.disabled).toBe(false);
  f.confirm.dispatchEvent(new Event("click"));
  f.confirm.dispatchEvent(new Event("click"));
  await Promise.resolve();
  await Promise.resolve();
  expect(f.dependencies.confirm).toHaveBeenCalledTimes(1);
  expect(f.confirm.disabled).toBe(true);
  expect(f.status.textContent).toContain("未变");
});
it("changing alias clears preview and makes the old confirmation unavailable", async () => {
  const f = fixture();
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.input("other");
  f.confirm.dispatchEvent(new Event("click"));
  expect(f.fields.children).toHaveLength(0);
  expect(f.confirm.disabled).toBe(true);
  expect(f.dependencies.confirm).not.toHaveBeenCalled();
});
it("late read cannot repaint a closed and reopened dialog", async () => {
  const f = fixture();
  let finish!: (v: ConfigPreview) => void;
  f.dependencies.read.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.controller.close();
  f.controller.open();
  f.input("other");
  await f.submit();
  finish({ ...value(), host: "old.example" });
  await Promise.resolve();
  await Promise.resolve();
  expect(f.fields.children[0]?.children[1]?.textContent).toBe("demo.example");
  expect(f.restoreFocus).toHaveBeenCalledTimes(1);
});
it("an alias change during pending read aborts it and blocks its late result", async () => {
  const f = fixture();
  let finish!: (v: ConfigPreview) => void;
  f.dependencies.read.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  f.controller.open();
  f.input("prod");
  await f.submit();
  const signal = f.dependencies.read.mock.calls[0]?.[2];
  f.input("other");
  expect(signal?.aborted).toBe(true);
  finish(value());
  await Promise.resolve();
  await Promise.resolve();
  expect(f.fields.children).toHaveLength(0);
  expect(f.confirm.disabled).toBe(true);
});
it("late confirmation cannot affect a reopened dialog", async () => {
  const f = fixture();
  let finish!: () => void;
  f.dependencies.confirm.mockImplementationOnce(
    () =>
      new Promise<void>((r) => {
        finish = r;
      }),
  );
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.confirm.dispatchEvent(new Event("click"));
  f.controller.close();
  f.controller.open();
  finish();
  await Promise.resolve();
  await Promise.resolve();
  expect(f.status.textContent).toContain("输入 Host");
  expect(f.confirm.disabled).toBe(true);
});
it("page identity changes cannot display a late read", async () => {
  const f = fixture();
  let finish!: (v: ConfigPreview) => void;
  f.dependencies.read.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.page("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
  finish(value());
  await Promise.resolve();
  await Promise.resolve();
  expect(f.fields.children).toHaveLength(0);
  expect(f.confirm.disabled).toBe(true);
});
it("source conflict leaves confirmation consumed and requires a fresh preview", async () => {
  const f = fixture();
  f.dependencies.confirm.mockRejectedValueOnce(
    Error("workspace.config_conflict"),
  );
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.confirm.dispatchEvent(new Event("click"));
  await Promise.resolve();
  await Promise.resolve();
  f.confirm.dispatchEvent(new Event("click"));
  expect(f.dependencies.confirm).toHaveBeenCalledTimes(1);
  expect(f.status.textContent).toContain("重新预览");
  expect(f.confirm.disabled).toBe(true);
});
it("Escape and page disposal abort work, clear values and keep disposed dialog closed", async () => {
  const f = fixture();
  f.dependencies.read.mockImplementationOnce(() => new Promise(() => {}));
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
  expect(f.alias.value).toBe("");
  expect(f.dependencies.read.mock.calls[0]?.[2]?.aborted).toBe(true);
  f.life.abort();
  f.controller.open();
  expect(f.dialog.open).toBe(false);
});

const flush = async () => {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
};
async function preparedFixture() {
  const f = fixture();
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.prepare.dispatchEvent(new Event("click"));
  await flush();
  return f;
}
it("prepare uses only preview ticket and user name; save is separate", async () => {
  const f = await preparedFixture();
  expect(f.dependencies.save).not.toHaveBeenCalled();
  expect(f.save.disabled).toBe(false);
  expect(f.confirm.disabled).toBe(true);
  expect(f.save.focused).toBe(true);
  f.save.dispatchEvent(new Event("click"));
  f.save.dispatchEvent(new Event("click"));
  await flush();
  expect(f.dependencies.save).toHaveBeenCalledTimes(1);
  expect(f.save.disabled).toBe(true);
  expect(f.status.textContent).toContain("已保存主机");
});
it("closing ready import revokes only its ticket", async () => {
  const f = await preparedFixture();
  f.controller.close();
  expect(f.dependencies.cancel).toHaveBeenCalledWith(
    nonce,
    "03969d3e-a25e-4a50-a8c4-f1d5c03e6aa6",
  );
  expect(f.dependencies.save).not.toHaveBeenCalled();
  expect(f.save.disabled).toBe(true);
});
it("changing name invalidates prepared authority", async () => {
  const f = await preparedFixture();
  f.label.value = "Changed";
  f.label.dispatchEvent(new Event("input"));
  f.save.dispatchEvent(new Event("click"));
  expect(f.dependencies.cancel).toHaveBeenCalledTimes(1);
  expect(f.dependencies.save).not.toHaveBeenCalled();
  expect(f.prepare.disabled).toBe(true);
});
it("late prepare after close is revoked and cannot repopulate", async () => {
  const f = fixture();
  let finish!: (v: ReturnType<typeof decodeConfigImport>) => void;
  f.dependencies.prepare.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.prepare.dispatchEvent(new Event("click"));
  f.controller.close();
  finish(
    decodeConfigImport({
      ...value(),
      label: "prod",
      token: "03969d3e-a25e-4a50-a8c4-f1d5c03e6aa6",
    }),
  );
  await flush();
  expect(f.dependencies.prepare.mock.calls[0]?.[3]?.aborted).toBe(true);
  expect(f.dependencies.cancel).toHaveBeenCalledTimes(1);
  expect(f.save.disabled).toBe(true);
  expect(f.fields.children).toHaveLength(0);
});
it("late prepare from another page cannot enable save", async () => {
  const f = fixture();
  let finish!: (v: ReturnType<typeof decodeConfigImport>) => void;
  f.dependencies.prepare.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.prepare.dispatchEvent(new Event("click"));
  f.page("9f3da39f-e375-4692-8390-814d18191660");
  finish(
    decodeConfigImport({
      ...value(),
      label: "prod",
      token: "03969d3e-a25e-4a50-a8c4-f1d5c03e6aa6",
    }),
  );
  await flush();
  expect(f.dependencies.cancel).toHaveBeenCalledWith(
    nonce,
    "03969d3e-a25e-4a50-a8c4-f1d5c03e6aa6",
  );
  expect(f.save.disabled).toBe(true);
});
it("prepare denial clears preview and requires new read", async () => {
  const f = fixture();
  f.dependencies.prepare.mockRejectedValueOnce(
    Error("workspace.config_conflict"),
  );
  f.controller.open();
  f.input("prod");
  await f.submit();
  f.prepare.dispatchEvent(new Event("click"));
  await flush();
  expect(f.prepare.disabled).toBe(true);
  expect(f.save.disabled).toBe(true);
  expect(f.status.textContent).toContain("重新预览");
});
it("save failure cannot be retried by duplicate or later click", async () => {
  const f = await preparedFixture();
  f.dependencies.save.mockRejectedValueOnce(Error("config.timeout"));
  f.save.dispatchEvent(new Event("click"));
  await flush();
  f.save.dispatchEvent(new Event("click"));
  await flush();
  expect(f.dependencies.save).toHaveBeenCalledTimes(1);
  expect(f.status.textContent).toContain("先查看主机目录");
  expect(f.status.textContent).toContain("不会自动重试");
});
it("closing active save aborts waiter and rejects late visual success", async () => {
  const f = await preparedFixture();
  let finish!: (v: string) => void;
  f.dependencies.save.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  f.save.dispatchEvent(new Event("click"));
  f.controller.close();
  expect(f.dependencies.save.mock.calls[0]?.[2]?.aborted).toBe(true);
  finish("dddddddd-dddd-7ddd-8ddd-dddddddddddd");
  await flush();
  expect(f.status.textContent).toBe("");
  expect(f.dialog.open).toBe(false);
});
it("new open has no leftover save name or authority", async () => {
  const f = await preparedFixture();
  f.controller.close();
  f.controller.open();
  expect(f.label.value).toBe("");
  expect(f.save.disabled).toBe(true);
  expect(f.prepare.disabled).toBe(true);
});

it("source change preserves displayed metadata and disables old-source import", async () => {
  vi.useFakeTimers();
  const check = vi.fn(async () => "changed" as const),
    f = fixture(check);
  try {
    f.controller.open();
    f.input("prod");
    await f.submit();
    await flush();
    const displayed = f.fields.children[0];
    await vi.advanceTimersByTimeAsync(2000);
    expect(f.status.textContent).toContain("配置源已变化");
    expect(f.fields.children[0]).toBe(displayed);
    expect(f.confirm.disabled).toBe(true);
    expect(f.prepare.disabled).toBe(true);
    f.prepare.dispatchEvent(new Event("click"));
    await flush();
    expect(f.dependencies.prepare).not.toHaveBeenCalled();
  } finally {
    f.controller.dispose();
    vi.useRealTimers();
  }
});
it("uncertain source checks do not retry or enable saving", async () => {
  vi.useFakeTimers();
  const check = vi.fn(async (): Promise<"unchanged"> => {
      throw Error("config.timeout");
    }),
    f = fixture(check);
  try {
    f.controller.open();
    f.input("prod");
    await f.submit();
    await flush();
    await vi.advanceTimersByTimeAsync(10000);
    expect(check).toHaveBeenCalledTimes(1);
    expect(f.status.textContent).toContain("源状态未确认");
    expect(f.prepare.disabled).toBe(true);
  } finally {
    f.controller.dispose();
    vi.useRealTimers();
  }
});
it("closing during a check aborts it and rejects late visual publication", async () => {
  vi.useFakeTimers();
  const wait = deferred<"changed">(),
    check = vi.fn(
      (_nonce: string, _token: string, _signal: AbortSignal) => wait.promise,
    ),
    f = fixture(check);
  try {
    f.controller.open();
    f.input("prod");
    await f.submit();
    await flush();
    await vi.advanceTimersByTimeAsync(2000);
    f.controller.close();
    expect(check.mock.calls[0]?.[2].aborted).toBe(true);
    wait.resolve("changed");
    await flush();
    expect(f.status.textContent).toBe("");
    expect(f.fields.children).toHaveLength(0);
  } finally {
    f.controller.dispose();
    vi.useRealTimers();
  }
});
