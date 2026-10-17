import { afterEach, expect, it, vi } from "vitest";
import type { HostCatalogue, HostFields } from "./workspace-hosts";
import { WorkspaceQuickConnectDialog } from "./workspace-quick-connect-dialog";

class Node extends EventTarget {
  value = "";
  textContent = "";
  disabled = false;
  open = false;
  isConnected = true;
  focus = vi.fn();
  ownerDocument: { activeElement: Node | null } = { activeElement: null };
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
}
afterEach(() => vi.unstubAllGlobals());
const nonce = "01010101-0101-4101-8101-010101010101";
const id = "019a9c6d-7000-7000-8000-000000000001";
const empty: HostCatalogue = {
  version: 1,
  token: nonce,
  revision: "0",
  hosts: [],
};
function fixture(value = empty) {
  vi.stubGlobal("HTMLElement", Node);
  const dialog = new Node(),
    address = new Node(),
    label = new Node(),
    inspect = new Node(),
    next = new Node(),
    cancel = new Node(),
    preview = new Node(),
    status = new Node(),
    life = new AbortController();
  const temporary = new Node();
  const selectTemporary = vi.fn();
  const mint = vi.fn(async (_nonce: string, fields: HostFields) => ({
    version: 1 as const,
    token: nonce,
    target: { id, ...fields },
  }));
  let current = nonce;
  const working = vi.fn(),
    changed = vi.fn(),
    select = vi.fn();
  const read = vi.fn(async () => value),
    save = vi.fn(async () => ({
      ...empty,
      revision: "1",
      hosts: [
        {
          id,
          label: "fixture.invalid",
          host: "fixture.invalid",
          port: 22,
          username: "user",
        },
      ],
    }));
  const controller = new WorkspaceQuickConnectDialog(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      address: address as unknown as HTMLInputElement,
      label: label as unknown as HTMLInputElement,
      inspect: inspect as unknown as HTMLButtonElement,
      continue: next as unknown as HTMLButtonElement,
      temporary: temporary as unknown as HTMLButtonElement,
      selectTemporary,
      cancel: cancel as unknown as HTMLButtonElement,
      preview: preview as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      signal: life.signal,
      nonce: () => current,
      enabled: () => true,
      working,
      changed,
      select,
    },
    { read, save, temporary: mint },
  );
  const click = (node: Node) => node.dispatchEvent(new Event("click"));
  return {
    controller,
    temporary,
    mint,
    selectTemporary,
    dialog,
    address,
    label,
    inspect,
    next,
    cancel,
    preview,
    status,
    life,
    read,
    save,
    changed,
    select,
    working,
    click,
    setNonce: (v: string) => {
      current = v;
    },
  };
}
const settle = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};
it("hands an unsaved frozen target to the original connection form and rechecks changed previews", async () => {
  const f = fixture();
  await f.controller.open();
  f.address.value = "user@fixture.invalid";
  f.click(f.inspect);
  f.address.value = "user@other.invalid";
  f.click(f.temporary);
  await settle();
  expect(f.mint).not.toHaveBeenCalled();
  f.click(f.temporary);
  await settle();
  expect(f.save).not.toHaveBeenCalled();
  expect(f.selectTemporary).toHaveBeenCalledWith(
    nonce,
    expect.objectContaining({
      target: expect.objectContaining({ host: "other.invalid" }),
    }),
  );
  expect(f.dialog.open).toBe(false);
});
it("a temporary mint completing after cancellation cannot open a connection form", async () => {
  const f = fixture();
  let finish: (value: Awaited<ReturnType<typeof f.mint>>) => void = () => {};
  f.mint.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await f.controller.open();
  f.address.value = "user@fixture.invalid";
  f.click(f.inspect);
  f.click(f.temporary);
  f.click(f.cancel);
  finish({
    version: 1,
    token: nonce,
    target: {
      id,
      label: "fixture.invalid",
      host: "fixture.invalid",
      port: 22,
      username: "user",
    },
  });
  await settle();
  expect(f.selectTemporary).not.toHaveBeenCalled();
  expect(f.save).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(false);
});

it("cancel after preview performs no save or target handoff", async () => {
  const f = fixture();
  await f.controller.open();
  f.address.value = "user@fixture.invalid";
  f.click(f.inspect);
  expect(f.next.disabled).toBe(false);
  f.click(f.cancel);
  expect(f.save).not.toHaveBeenCalled();
  expect(f.select).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(false);
});
it("one explicit save hands only a new saved identity to the existing connection form", async () => {
  const f = fixture();
  await f.controller.open();
  f.address.value = "user@fixture.invalid";
  f.click(f.inspect);
  f.click(f.next);
  f.click(f.next);
  await settle();
  expect(f.save).toHaveBeenCalledTimes(1);
  expect(f.select).toHaveBeenCalledWith(id);
  expect(f.dialog.open).toBe(false);
  expect(f.changed).toHaveBeenCalledTimes(1);
});
it("reuses an exact saved target without a write and refuses ambiguous duplicates", async () => {
  const host = {
    id,
    label: "saved",
    host: "fixture.invalid",
    port: 22,
    username: "user",
  };
  const f = fixture({ ...empty, hosts: [host] });
  await f.controller.open();
  f.address.value = "user@fixture.invalid";
  f.click(f.inspect);
  f.click(f.next);
  await settle();
  expect(f.save).not.toHaveBeenCalled();
  expect(f.select).toHaveBeenCalledWith(id);
  const ambiguous = fixture({
    ...empty,
    hosts: [host, { ...host, id: "019a9c6d-7000-7000-8000-000000000002" }],
  });
  await ambiguous.controller.open();
  ambiguous.address.value = "user@fixture.invalid";
  ambiguous.click(ambiguous.inspect);
  ambiguous.click(ambiguous.next);
  await settle();
  expect(ambiguous.next.disabled).toBe(true);
  expect(ambiguous.save).not.toHaveBeenCalled();
  expect(ambiguous.select).not.toHaveBeenCalled();
});
it("changing the input without an event still requires reviewing the new target", async () => {
  const f = fixture();
  await f.controller.open();
  f.address.value = "user@fixture.invalid";
  f.click(f.inspect);
  f.address.value = "user@other.invalid";
  f.click(f.next);
  await settle();
  expect(f.save).not.toHaveBeenCalled();
  expect(f.select).not.toHaveBeenCalled();
  expect(f.preview.textContent).toContain("other.invalid");
});
it("cancelled or replaced page cannot use a late successful save to open a connection", async () => {
  for (const cancel of [true, false]) {
    const f = fixture();
    let reply: ((v: Awaited<ReturnType<typeof f.save>>) => void) | undefined;
    f.save.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          reply = resolve;
        }),
    );
    await f.controller.open();
    f.address.value = "user@fixture.invalid";
    f.click(f.inspect);
    f.click(f.next);
    expect(f.address.disabled).toBe(true);
    if (cancel) f.click(f.cancel);
    else f.setNonce("02020202-0202-4202-8202-020202020202");
    reply?.({
      ...empty,
      revision: "1",
      hosts: [
        {
          id,
          label: "fixture.invalid",
          host: "fixture.invalid",
          port: 22,
          username: "user",
        },
      ],
    });
    await settle();
    expect(f.select).not.toHaveBeenCalled();
    expect(f.changed).not.toHaveBeenCalled();
    f.controller.close();
  }
});
