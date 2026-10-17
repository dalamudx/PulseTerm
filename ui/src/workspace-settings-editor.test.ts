import { expect, it, vi } from "vitest";
import { decodeWorkspaceSettings } from "./workspace-settings";
import { WorkspaceSettingsEditor } from "./workspace-settings-editor";

class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  readonly attributes = new Map<string, string>();
  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
  }
  value = "";
  type = "";
  className = "";
  disabled = false;
  checked = false;
  min = "";
  max = "";
  step = "";
  maxLength = 0;
  open = false;
  isConnected = true;
  focus() {
    this.ownerDocument.activeElement = this;
  }
  constructor(
    readonly ownerDocument: Doc,
    readonly tag = "div",
  ) {
    super();
    if (tag === "dialog") ownerDocument.dialogs.push(this);
  }
  append(...nodes: Node[]) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
  }
  querySelectorAll(): Node[] {
    return this.children
      .flatMap((n) => [n, ...n.querySelectorAll()])
      .filter(
        (n) => n.tag === "input" || n.tag === "select" || n.tag === "button",
      );
  }
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
}
class Doc {
  activeElement: Node | null = null;
  dialogs: Node[] = [];
  querySelector() {
    return this.dialogs.find((dialog) => dialog.open) ?? null;
  }
  createElement(tag: string): Node {
    return new Node(this, tag);
  }
}
const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
function value(size = 14) {
  return decodeWorkspaceSettings({
    version: 1,
    token: nonce,
    values: {
      font_size: { value: size, source: size === 14 ? "default" : "user" },
      font_family: { value: "monospace", source: "default" },
      cursor_blink: { value: false, source: "default" },
      copy_on_select: { value: false, source: "default" },
      right_click_paste: { value: false, source: "default" },
    },
  });
}
function fixture(withKeymap = false) {
  const doc = new Doc(),
    dialog = new Node(doc, "dialog"),
    fields = new Node(doc),
    status = new Node(doc),
    life = new AbortController(),
    changed = vi.fn(),
    working = vi.fn();
  let currentNonce = nonce;
  const openKeymap = vi.fn();
  const dependencies = {
    read: vi.fn(async () => value()),
    save: vi.fn(async () => value(18)),
  };
  const editor = new WorkspaceSettingsEditor(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      fields: fields as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      signal: life.signal,
      nonce: () => currentNonce,
      openKeymap: withKeymap ? openKeymap : undefined,
      working,
      changed,
    },
    dependencies,
  );
  return {
    editor,
    dialog,
    fields,
    status,
    life,
    changed,
    working,
    dependencies,
    doc,
    openKeymap,
    setNonce: (value: string) => {
      currentNonce = value;
    },
  };
}
const settle = async () => {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
};
it("per-field source and explicit false display; save does not use raw paths", async () => {
  const f = fixture();
  await f.editor.open();
  expect(f.fields.children).toHaveLength(5);
  const form = f.fields.children[0];
  if (!form) throw Error("fixture.form");
  const input = form.children[0]?.children[0];
  if (!input) throw Error("fixture.input");
  input.value = "18";
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenCalledWith(
    nonce,
    nonce,
    { field: "font_size", value: 18 },
    expect.any(AbortSignal),
  );
  expect(f.changed).toHaveBeenCalledOnce();
  expect(f.fields.children[0]?.children[1]?.textContent).toBe("用户覆盖");
  expect(f.fields.children[2]?.children[0]?.children[0]?.checked).toBe(false);
  f.editor.dispose();
});
it("old rendered reset/submit cannot mutate replacement candidate", async () => {
  const f = fixture();
  await f.editor.open();
  const form = f.fields.children[0];
  if (!form) throw Error("fixture.form");
  form.dispatchEvent(new Event("submit"));
  await settle();
  form.dispatchEvent(new Event("submit"));
  form.children[3]?.dispatchEvent(new Event("click"));
  expect(f.dependencies.save).toHaveBeenCalledOnce();
  f.editor.dispose();
});
it("closing awaiting save does not publish late results or clear newer operation", async () => {
  const f = fixture();
  let reply!: (v: ReturnType<typeof value>) => void;
  f.dependencies.save = vi.fn(
    () =>
      new Promise((resolve) => {
        reply = resolve;
      }),
  );
  await f.editor.open();
  f.fields.children[0]?.dispatchEvent(new Event("submit"));
  f.editor.close();
  await f.editor.open();
  const before = f.working.mock.calls.length;
  reply(value(18));
  await settle();
  expect(f.changed).not.toHaveBeenCalled();
  expect(f.working).toHaveBeenCalledTimes(before);
  f.editor.dispose();
});
it("failed write requires fresh read, never retries or restores an older value", async () => {
  const f = fixture();
  f.dependencies.save = vi.fn(async () => {
    throw Error("uncertain");
  });
  await f.editor.open();
  f.fields.children[0]?.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.fields.children).toHaveLength(0);
  expect(f.status.textContent).toContain("不自动重试");
  expect(f.dependencies.save).toHaveBeenCalledOnce();
  f.editor.dispose();
});

it("selection-copy is default off and saves an explicit boolean or reveals inheritance", async () => {
  const f = fixture();
  await f.editor.open();
  const form = f.fields.children[3],
    input = form?.children[0]?.children[0];
  if (!form || !input) throw Error("fixture.selection-copy");
  expect(input.type).toBe("checkbox");
  expect(input.checked).toBe(false);
  input.checked = true;
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "copy_on_select", value: true },
    expect.any(AbortSignal),
  );
  f.fields.children[3]?.children[3]?.dispatchEvent(new Event("click"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "reset_copy_on_select" },
    expect.any(AbortSignal),
  );
  f.editor.dispose();
});

it("v2 metric rows save independently, reset inheritance and revoke old row events", async () => {
  const f = fixture();
  const modern = decodeWorkspaceSettings({
    ...value(),
    version: 2,
    values: {
      ...value().values,
      line_height_percent: { value: 100, source: "default" },
      letter_spacing: { value: 0, source: "default" },
    },
  });
  f.dependencies.read.mockResolvedValue(modern);
  f.dependencies.save.mockImplementation(async () =>
    decodeWorkspaceSettings({
      ...modern,
      values: {
        ...modern.values,
        line_height_percent: { value: 150, source: "user" },
      },
    }),
  );
  await f.editor.open();
  expect(f.fields.children).toHaveLength(7);
  const height = f.fields.children[2];
  const input = height?.children[0]?.children[0];
  if (!height || !input) throw Error("fixture.metrics");
  expect(input.type).toBe("number");
  expect([input.min, input.max, input.step]).toEqual(["100", "300", "1"]);
  input.value = "150";
  height.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "line_height_percent", value: 150 },
    expect.any(AbortSignal),
  );
  height.children[3]?.dispatchEvent(new Event("click"));
  expect(f.dependencies.save).toHaveBeenCalledTimes(1);
  const spacing = f.fields.children[3];
  spacing?.children[3]?.dispatchEvent(new Event("click"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "reset_letter_spacing" },
    expect.any(AbortSignal),
  );
  f.editor.dispose();
});

it("font preset selection requires explicit save and old selector cannot alter a replacement", async () => {
  const f = fixture();
  await f.editor.open();
  const form = f.fields.children[1],
    input = form?.children[0]?.children[0],
    select = form?.children[0]?.children[1];
  if (!form || !input || !select) throw Error("fixture.font");
  select.value = "jetbrains";
  select.dispatchEvent(new Event("change"));
  expect(input.value).toBe("JetBrains Mono");
  expect(f.dependencies.save).not.toHaveBeenCalled();
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenCalledWith(
    nonce,
    nonce,
    { field: "font_family", value: "JetBrains Mono" },
    expect.any(AbortSignal),
  );
  select.value = "system";
  select.dispatchEvent(new Event("change"));
  expect(input.value).toBe("JetBrains Mono");
  form.dispatchEvent(new Event("submit"));
  expect(f.dependencies.save).toHaveBeenCalledOnce();
  f.editor.dispose();
});

it("operator ligatures require explicit original-page boolean save and a fresh reset row", async () => {
  const f = fixture();
  const projection = decodeWorkspaceSettings({
    version: 3,
    token: nonce,
    values: {
      ...value().values,
      line_height_percent: { value: 100, source: "default" },
      letter_spacing: { value: 0, source: "default" },
      font_fallback: { value: "monospace", source: "default" },
      font_ligatures: { value: false, source: "default" },
    },
  });
  f.dependencies.read.mockResolvedValue(projection);
  f.dependencies.save.mockResolvedValue({
    ...projection,
    values: { ...projection.values },
  });
  await f.editor.open();
  const form = f.fields.children.find(
    (row) => row.children[0]?.textContent === "运算符连字",
  );
  const checkbox = form?.children[0]?.children[0];
  if (!form || !checkbox) throw Error("fixture.ligatures");
  expect(checkbox.type).toBe("checkbox");
  expect(checkbox.disabled).toBe(false);
  checkbox.checked = true;
  expect(f.dependencies.save).not.toHaveBeenCalled();
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "font_ligatures", value: true },
    expect.any(AbortSignal),
  );
  form.dispatchEvent(new Event("submit"));
  expect(f.dependencies.save).toHaveBeenCalledOnce();
  const fresh = f.fields.children.find(
    (row) => row.children[0]?.textContent === "运算符连字",
  );
  fresh?.children[3]?.dispatchEvent(new Event("click"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "reset_font_ligatures" },
    expect.any(AbortSignal),
  );
  f.editor.dispose();
});

it("history capacity row uses bounded integer input and saves/resets only that field", async () => {
  const f = fixture();
  f.dependencies.read.mockResolvedValue({
    ...value(),
    values: {
      ...value().values,
      history_limit: { value: 100000, source: "default" },
    },
  });
  f.dependencies.save.mockResolvedValue({
    ...value(),
    values: {
      ...value().values,
      history_limit: { value: 2500, source: "user" },
    },
  });
  await f.editor.open();
  const form = f.fields.children[0];
  const input = form?.children[0]?.children[0];
  if (!form || !input) throw Error("fixture.history");
  expect(input.type).toBe("number");
  expect([input.min, input.max, input.step]).toEqual(["1000", "100000", "1"]);
  expect(form.children[1]?.textContent).toContain("新建终端生效");
  input.value = "2500";
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "history_limit", value: 2500 },
    expect.any(AbortSignal),
  );
  f.fields.children[0]?.children[3]?.dispatchEvent(new Event("click"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "reset_history_limit" },
    expect.any(AbortSignal),
  );
  f.editor.dispose();
});

it("input scroll checkbox saves false and resets only its own preference", async () => {
  const f = fixture();
  f.dependencies.read.mockResolvedValue({
    ...value(),
    values: {
      ...value().values,
      scroll_on_input: { value: true, source: "default" },
    },
  });
  f.dependencies.save.mockResolvedValue({
    ...value(),
    values: {
      ...value().values,
      scroll_on_input: { value: false, source: "user" },
    },
  });
  await f.editor.open();
  const form = f.fields.children.find(
    (n) => n.children[0]?.textContent === "输入时回到底部",
  );
  const input = form?.children[0]?.children[0];
  if (!form || !input) throw Error("scroll input row missing");
  expect(input.type).toBe("checkbox");
  expect(input.checked).toBe(true);
  input.checked = false;
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "scroll_on_input", value: false },
    expect.any(AbortSignal),
  );
  const fresh = f.fields.children.find(
    (n) => n.children[0]?.textContent === "输入时回到底部",
  );
  expect(fresh?.children[0]?.children[0]?.checked).toBe(false);
  fresh?.children[3]?.dispatchEvent(new Event("click"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "reset_scroll_on_input" },
    expect.any(AbortSignal),
  );
  f.editor.dispose();
});

it("input scroll checkbox saves false and resets only its own preference", async () => {
  const f = fixture();
  f.dependencies.read.mockResolvedValue({
    ...value(),
    values: {
      ...value().values,
      scroll_on_output: { value: true, source: "default" },
    },
  });
  f.dependencies.save.mockResolvedValue({
    ...value(),
    values: {
      ...value().values,
      scroll_on_output: { value: true, source: "user" },
    },
  });
  await f.editor.open();
  const form = f.fields.children.find(
    (n) => n.children[0]?.textContent === "新输出时回到底部",
  );
  const input = form?.children[0]?.children[0];
  if (!form || !input) throw Error("scroll input row missing");
  expect(input.type).toBe("checkbox");
  expect(input.checked).toBe(true);
  input.checked = true;
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "scroll_on_output", value: true },
    expect.any(AbortSignal),
  );
  const fresh = f.fields.children.find(
    (n) => n.children[0]?.textContent === "新输出时回到底部",
  );
  expect(fresh?.children[0]?.children[0]?.checked).toBe(true);
  fresh?.children[3]?.dispatchEvent(new Event("click"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "reset_scroll_on_output" },
    expect.any(AbortSignal),
  );
  f.editor.dispose();
});

it("Option Meta checkbox saves and resets only its own preference", async () => {
  const f = fixture();
  f.dependencies.read.mockResolvedValue({
    ...value(),
    values: {
      ...value().values,
      mac_option_as_meta: { value: false, source: "default" },
    },
  });
  f.dependencies.save
    .mockResolvedValueOnce({
      ...value(),
      values: {
        ...value().values,
        mac_option_as_meta: { value: true, source: "user" },
      },
    })
    .mockResolvedValueOnce(value());
  await f.editor.open();
  const form = f.fields.children.find(
    (n) => n.children[0]?.textContent === "macOS Option 作为 Meta 键",
  );
  const input = form?.children[0]?.children[0];
  if (!form || !input) throw Error("missing Option Meta control");
  expect(input.type).toBe("checkbox");
  expect(input.checked).toBe(false);
  input.checked = true;
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "mac_option_as_meta", value: true },
    expect.any(AbortSignal),
  );
  const fresh = f.fields.children.find(
    (n) => n.children[0]?.textContent === "macOS Option 作为 Meta 键",
  );
  expect(fresh?.children[0]?.children[0]?.checked).toBe(true);
  fresh?.children[3]?.dispatchEvent(new Event("click"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "reset_mac_option_as_meta" },
    expect.any(AbortSignal),
  );
  f.editor.dispose();
});

it("Backspace selection saves one closed value and restores inheritance", async () => {
  const f = fixture();
  f.dependencies.read.mockResolvedValue({
    ...value(),
    values: {
      ...value().values,
      backspace_send: { value: "del", source: "default" },
    },
  });
  f.dependencies.save
    .mockResolvedValueOnce({
      ...value(),
      values: {
        ...value().values,
        backspace_send: { value: "bs", source: "user" },
      },
    })
    .mockResolvedValueOnce(value());
  await f.editor.open();
  const form = f.fields.children.find(
    (n) => n.children[0]?.textContent === "Backspace 发送",
  );
  const select = form?.children[0]?.children[0];
  if (!form || !select) throw Error("fixture.backspace");
  expect(select.value).toBe("del");
  select.value = "bs";
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "backspace_send", value: "bs" },
    expect.any(AbortSignal),
  );
  const fresh = f.fields.children.find(
    (n) => n.children[0]?.textContent === "Backspace 发送",
  );
  expect(fresh?.children[0]?.children[0]?.value).toBe("bs");
  fresh?.children[3]?.dispatchEvent(new Event("click"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    nonce,
    nonce,
    { field: "reset_backspace_send" },
    expect.any(AbortSignal),
  );
  f.editor.dispose();
});

it("searchable keymap entry preserves drafts and returns once to its original control", async () => {
  const f = fixture(true);
  await f.editor.open();
  const form = f.fields.children[0];
  const input = form?.children[0]?.children[0];
  const button = f.fields.children.at(-1)?.children[0];
  if (!form || !input || !button) throw Error("fixture.keymap");
  input.value = "21";
  button.focus();
  button.dispatchEvent(new Event("click"));
  expect(f.openKeymap).toHaveBeenCalledOnce();
  const resume = f.editor.suspend();
  expect(resume).toBeTypeOf("function");
  expect(f.dialog.open).toBe(false);
  form.dispatchEvent(new Event("submit"));
  button.dispatchEvent(new Event("click"));
  await f.editor.open();
  expect(f.dependencies.read).toHaveBeenCalledOnce();
  expect(f.dependencies.save).not.toHaveBeenCalled();
  expect(f.openKeymap).toHaveBeenCalledOnce();
  resume?.();
  expect(f.dialog.open).toBe(true);
  expect(f.fields.children[0]).toBe(form);
  expect(input.value).toBe("21");
  expect(f.doc.activeElement).toBe(button);
  resume?.();
  expect(f.dependencies.read).toHaveBeenCalledOnce();
  f.editor.dispose();
});

it("old child returns cannot revive closed, disposed or changed-page settings", async () => {
  for (const mode of ["close", "dispose", "nonce"] as const) {
    const f = fixture(true);
    await f.editor.open();
    const resume = f.editor.suspend();
    if (mode === "close") f.editor.close();
    if (mode === "dispose") f.editor.dispose();
    if (mode === "nonce") f.setNonce("22222222-2222-4222-8222-222222222222");
    resume?.();
    expect(f.dialog.open).toBe(false);
    expect(f.dependencies.save).not.toHaveBeenCalled();
    f.editor.dispose();
  }
});

it("a superseded return cannot replace a fresh read or stack over another dialog", async () => {
  const f = fixture(true);
  await f.editor.open();
  const resume = f.editor.suspend();
  const other = new Node(f.doc, "dialog");
  f.working.mockClear();
  other.showModal();
  resume?.();
  expect(f.dialog.open).toBe(false);
  expect(other.open).toBe(true);
  expect(f.working).not.toHaveBeenCalled();
  other.close();
  await f.editor.open();
  const fresh = f.fields.children[0];
  resume?.();
  expect(f.dialog.open).toBe(true);
  expect(f.fields.children[0]).toBe(fresh);
  expect(f.dependencies.read).toHaveBeenCalledTimes(2);
  f.editor.dispose();
});
