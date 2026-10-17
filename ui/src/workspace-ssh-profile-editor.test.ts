import { afterEach, expect, it, vi } from "vitest";
import { WorkspaceSshProfileEditor } from "./workspace-ssh-profile-editor";
import { decodeSshProfiles } from "./workspace-ssh-profiles";

const token = "11111111-1111-4111-8111-111111111111";
const id = (n: number) =>
  `${String(n).padStart(8, "0")}-1111-7111-8111-111111111111`;
const view = () =>
  decodeSshProfiles({
    token,
    data: {
      version: 1,
      revision: "1",
      migrated: true,
      machines: [{ id: id(1), label: "Machine" }],
      entrances: [2, 3].map((n) => ({
        id: id(n),
        label: `Entrance ${n}`,
        host: "fixture.invalid",
        port: 22,
        username: "u",
        machineId: id(1),
      })),
      profiles: [2, 3].map((n) => ({
        id: id(n + 2),
        endpointId: id(n),
        label: `Profile ${n}`,
        username: "u",
        identityId: null,
      })),
      identities: [
        {
          id: id(6),
          kind: "key",
          label: "Shared key",
          username: "u",
          available: true,
        },
      ],
    },
  });
class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  value = "";
  disabled = false;
  open = false;
  id = "";
  isConnected = true;
  ownerDocument = doc;
  constructor(readonly tag = "div") {
    super();
  }
  append(...nodes: Node[]) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
  }
  setAttribute(_name: string, _value: string) {}
  get childElementCount() {
    return this.children.length;
  }
  focus() {
    doc.activeElement = this;
  }
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
}
const doc = {
  activeElement: undefined as Node | undefined,
  createElement: (tag: string) => new Node(tag),
};
function fixture() {
  vi.stubGlobal("HTMLElement", Node);
  vi.stubGlobal("document", doc);
  const dialog = new Node("dialog"),
    fields = new Node(),
    status = new Node(),
    done = new Node("button"),
    life = new AbortController(),
    changed = vi.fn(),
    working = vi.fn(),
    dependencies = {
      read: vi.fn(async () => view()),
      save: vi.fn(async () => view()),
    };
  const editor = new WorkspaceSshProfileEditor(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      fields: fields as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      done: done as unknown as HTMLButtonElement,
      signal: life.signal,
      nonce: () => token,
      enabled: () => true,
      working,
      changed,
    },
    dependencies,
  );
  const node = (idOrTag: string): Node => {
    const walk = (node: Node): Node[] => [node, ...node.children.flatMap(walk)];
    return walk(fields).find((n) => n.id === idOrTag || n.tag === idOrTag)!;
  };
  return {
    editor,
    fields,
    dialog,
    status,
    done,
    life,
    changed,
    working,
    dependencies,
    node,
  };
}
afterEach(() => vi.unstubAllGlobals());
it("cancel before save makes zero writes and close precedes control reenable", async () => {
  const f = fixture();
  await f.editor.open();
  let open = true;
  f.working.mockImplementation(() => {
    open = f.dialog.open;
  });
  f.done.dispatchEvent(new Event("click"));
  expect(open).toBe(false);
  expect(f.dependencies.save).not.toHaveBeenCalled();
  expect(f.fields.children).toEqual([]);
});
it("stale form cannot save after the current picker silently changes", async () => {
  const f = fixture();
  await f.editor.open();
  const old = f.node("form"),
    picker = f.node("workspace-ssh-profile-picker");
  picker.value = id(5);
  old.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(f.dependencies.save).not.toHaveBeenCalled();
  picker.dispatchEvent(new Event("change"));
  old.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(f.dependencies.save).not.toHaveBeenCalled();
  f.node("workspace-ssh-profile-label").value = "Independent";
  f.node("workspace-ssh-profile-identity").value = id(6);
  f.node("form").dispatchEvent(new Event("submit", { cancelable: true }));
  await vi.waitFor(() => expect(f.dependencies.save).toHaveBeenCalledTimes(1));
  expect(f.dependencies.save).toHaveBeenCalledWith(
    token,
    token,
    {
      version: 2,
      via: null,
      id: id(5),
      label: "Independent",
      username: "u",
      identityId: id(6),
    },
    expect.any(AbortSignal),
  );
  f.editor.close();
});
it("late save result cannot revive a closed form or publish to the tree", async () => {
  const f = fixture();
  let resolve!: (value: ReturnType<typeof view>) => void;
  f.dependencies.save.mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await f.editor.open();
  f.changed.mockClear();
  f.node("form").dispatchEvent(new Event("submit", { cancelable: true }));
  f.editor.close();
  resolve(view());
  await Promise.resolve();
  expect(f.changed).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(false);
  expect(f.fields.children).toEqual([]);
});
it("sends jump and preferences once and rejects injected self or foreign choices", async () => {
  const f = fixture();
  await f.editor.open();
  const jump = f.node("workspace-ssh-profile-jump");
  expect(jump.children.some((n) => n.value === id(4))).toBe(false);
  for (const via of [id(4), id(6)]) {
    jump.value = via;
    f.node("form").dispatchEvent(new Event("submit", { cancelable: true }));
    expect(f.dependencies.save).not.toHaveBeenCalled();
  }
  jump.value = id(5);
  f.node("workspace-ssh-profile-label").value = "Routed";
  f.node("workspace-ssh-profile-username").value = "explicit-user";
  f.node("form").dispatchEvent(new Event("submit", { cancelable: true }));
  await vi.waitFor(() => expect(f.dependencies.save).toHaveBeenCalledTimes(1));
  expect(f.dependencies.save).toHaveBeenCalledWith(
    token,
    token,
    {
      version: 2,
      via: id(5),
      id: id(4),
      label: "Routed",
      username: "explicit-user",
      identityId: null,
    },
    expect.any(AbortSignal),
  );
  f.editor.close();
});
