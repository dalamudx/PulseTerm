import { afterEach, expect, it, vi } from "vitest";
import { WorkspaceQuickSaveDialog } from "./workspace-quick-save-dialog";
import type { TemporaryTarget } from "./workspace-temporary-target";

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
const target: TemporaryTarget = {
  version: 1,
  token: nonce,
  target: {
    id: "019a9c6d-7000-7000-8000-000000000001",
    label: "临时主机",
    host: "127.0.0.1",
    port: 22,
    username: "user",
  },
};
function fixture() {
  vi.stubGlobal("HTMLElement", Node);
  const dialog = new Node(),
    label = new Node(),
    description = new Node(),
    status = new Node(),
    save = new Node(),
    cancel = new Node();
  const life = new AbortController(),
    working = vi.fn(),
    saved = vi.fn(async () => {});
  const persist = vi.fn(async () => target.target.id),
    discard = vi.fn(async () => {});
  const controller = new WorkspaceQuickSaveDialog(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      label: label as unknown as HTMLInputElement,
      description: description as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      save: save as unknown as HTMLButtonElement,
      cancel: cancel as unknown as HTMLButtonElement,
      signal: life.signal,
      nonce: () => nonce,
      working,
      saved,
    },
    { save: persist, cancel: discard },
  );
  return {
    dialog,
    label,
    status,
    save,
    cancel,
    saved,
    persist,
    discard,
    controller,
  };
}
const tick = async () => {
  await Promise.resolve();
  await Promise.resolve();
};
it("optional discard performs no save and invalid names remain correctable before consuming the proof", async () => {
  const f = fixture();
  f.controller.open(nonce, target);
  f.label.value = "";
  f.save.dispatchEvent(new Event("click"));
  await tick();
  expect(f.persist).not.toHaveBeenCalled();
  expect(f.label.disabled).toBe(false);
  f.label.value = "保存名称";
  f.save.dispatchEvent(new Event("click"));
  await tick();
  expect(f.persist).toHaveBeenCalledTimes(1);
  expect(f.saved).toHaveBeenCalledWith(nonce, target.target.id);
  f.controller.open(nonce, target);
  f.cancel.dispatchEvent(new Event("click"));
  await tick();
  expect(f.persist).toHaveBeenCalledTimes(1);
  expect(f.discard).toHaveBeenCalledWith(nonce, target.token);
});
it("duplicate clicks cannot consume another proof and a late save cannot publish after closing", async () => {
  const f = fixture();
  let finish: (value: string) => void = () => {};
  f.persist.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  f.controller.open(nonce, target);
  f.save.dispatchEvent(new Event("click"));
  f.save.dispatchEvent(new Event("click"));
  expect(f.persist).toHaveBeenCalledTimes(1);
  f.controller.close();
  finish(target.target.id);
  await tick();
  expect(f.saved).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(false);
});
