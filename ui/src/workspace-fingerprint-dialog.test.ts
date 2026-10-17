import { afterEach, expect, it, vi } from "vitest";
import { WorkspaceFingerprintDialog } from "./workspace-fingerprint-dialog";
import type { Fingerprint } from "./workspace-ssh-fingerprint";

const nonce = "00000000-0000-4000-8000-000000000001",
  host = "00000000-0000-7000-8000-000000000003";
const value = (status: Fingerprint["status"] = "unknown"): Fingerprint => ({
  version: 1,
  token: status === "unknown" ? nonce : null,
  label: "<img src=x>",
  host: "127.0.0.1",
  port: 22,
  algorithm: "ssh-ed25519",
  fingerprint: `SHA256:${"A".repeat(43)}`,
  status,
  source: "application_only",
});
class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  className = "";
  tabIndex = -1;
  disabled = false;
  open = false;
  isConnected = true;
  focuses = 0;
  ownerDocument: {
    activeElement: Node | null;
    createElement: (tag: string) => Node;
  };
  constructor(
    readonly tag = "div",
    document?: Node["ownerDocument"],
  ) {
    super();
    this.ownerDocument = document ?? {
      activeElement: null,
      createElement: (tag) => new Node(tag, this.ownerDocument),
    };
  }
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
    this.focuses++;
    this.ownerDocument.activeElement = this;
  }
}
afterEach(() => vi.unstubAllGlobals());
function fixture(status: Fingerprint["status"] = "unknown") {
  const dialog = new Node("dialog"),
    fields = new Node("div", dialog.ownerDocument),
    message = new Node("p", dialog.ownerDocument),
    cancel = new Node("button", dialog.ownerDocument),
    approve = new Node("button", dialog.ownerDocument),
    origin = new Node("button", dialog.ownerDocument);
  origin.focus();
  vi.stubGlobal("HTMLElement", Node);
  const life = new AbortController();
  let currentNonce = nonce;
  const client = {
    observe: vi.fn(async () => value(status)),
    confirm: vi.fn(async () => "1"),
    cancel: vi.fn(),
  };
  const dependencies = { prepare: vi.fn(async () => client) },
    working = vi.fn();
  const controller = new WorkspaceFingerprintDialog(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      fields: fields as unknown as HTMLElement,
      status: message as unknown as HTMLElement,
      cancel: cancel as unknown as HTMLButtonElement,
      approve: approve as unknown as HTMLButtonElement,
      signal: life.signal,
      nonce: () => currentNonce,
      working,
    },
    dependencies,
  );
  return {
    controller,
    dialog,
    fields,
    message,
    cancel,
    approve,
    origin,
    client,
    dependencies,
    life,
    working,
    setNonce: (v: string) => {
      currentNonce = v;
    },
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
it("unknown fingerprint defaults to cancel, literal metadata has no markup, explicit save does not start connection", async () => {
  const f = fixture();
  await f.controller.open(host);
  expect(f.cancel.focuses).toBe(1);
  expect(f.approve.focuses).toBe(0);
  expect(f.approve.disabled).toBe(false);
  expect(f.fields.children[0]?.children[1]?.textContent).toBe("<img src=x>");
  expect(f.client.confirm).not.toHaveBeenCalled();
  f.approve.dispatchEvent(new Event("click"));
  await Promise.resolve();
  await Promise.resolve();
  expect(f.client.confirm).toHaveBeenCalledTimes(1);
  expect(f.message.textContent).toContain("没有启动 SSH");
  f.controller.close();
  expect(f.client.cancel).toHaveBeenCalled();
  expect(f.origin.focuses).toBe(2);
});
it.each(["changed", "already_pinned"] as const)(
  "%s never enables or sends approval even on synthetic button click",
  async (status) => {
    const f = fixture(status);
    await f.controller.open(host);
    expect(f.approve.disabled).toBe(true);
    f.approve.dispatchEvent(new Event("click"));
    await Promise.resolve();
    expect(f.client.confirm).not.toHaveBeenCalled();
  },
);
it("closing before prepare settles cancels late returned client without drawing or stealing focus", async () => {
  const f = fixture(),
    pending = deferred<typeof f.client>();
  f.dependencies.prepare.mockImplementationOnce(() => pending.promise);
  const opening = f.controller.open(host);
  f.controller.close();
  const focus = f.origin.focuses;
  pending.resolve(f.client);
  await opening;
  expect(f.client.cancel).toHaveBeenCalled();
  expect(f.client.observe).not.toHaveBeenCalled();
  expect(f.fields.children).toHaveLength(0);
  expect(f.origin.focuses).toBe(focus);
});
it("old observation cannot populate reopened modal or enable its save", async () => {
  const f = fixture(),
    pending = deferred<Fingerprint>();
  f.client.observe.mockImplementationOnce(() => pending.promise);
  const opening = f.controller.open(host);
  await Promise.resolve();
  f.controller.close();
  await f.controller.open(host);
  const status = f.message.textContent;
  pending.resolve(value("changed"));
  await opening;
  expect(f.message.textContent).toBe(status);
  expect(f.approve.disabled).toBe(false);
});
it("page retirement during confirmation cancels ticket and rejects late receipt/focus publication", async () => {
  const f = fixture(),
    pending = deferred<string>();
  await f.controller.open(host);
  f.client.confirm.mockImplementationOnce(() => pending.promise);
  f.approve.dispatchEvent(new Event("click"));
  f.life.abort();
  const focus = f.cancel.focuses;
  pending.resolve("1");
  await Promise.resolve();
  await Promise.resolve();
  expect(f.client.cancel).toHaveBeenCalled();
  expect(f.dialog.open).toBe(false);
  expect(f.message.textContent).toBe("");
  expect(f.cancel.focuses).toBe(focus);
  expect(f.origin.focuses).toBe(1);
});
it("changed original page nonce cannot enable a late unknown fingerprint", async () => {
  const f = fixture(),
    pending = deferred<Fingerprint>();
  f.client.observe.mockImplementationOnce(() => pending.promise);
  const opening = f.controller.open(host);
  await Promise.resolve();
  f.setNonce("00000000-0000-4000-8000-000000000004");
  pending.resolve(value());
  await opening;
  expect(f.approve.disabled).toBe(true);
  expect(f.message.textContent).not.toContain("首次见到");
  f.controller.close();
  expect(f.origin.focuses).toBe(1);
});
