import { expect, it, vi } from "vitest";
import { WorkspaceAppearance } from "./workspace-appearance";
import {
  decodeWorkspaceSettings,
  type SettingsChange,
  type WorkspaceSettings,
} from "./workspace-settings";
import { WorkspaceSettingsEditor } from "./workspace-settings-editor";

class Media extends EventTarget {
  matches = false;
  change(matches: boolean) {
    this.matches = matches;
    this.dispatchEvent(new Event("change"));
  }
}
class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  className = "";
  value = "";
  ariaLabel = "";
  type = "";
  disabled = false;
  open = false;
  constructor(
    readonly tag: string,
    readonly ownerDocument: Document,
  ) {
    super();
  }
  append(...children: Node[]) {
    this.children.push(...children);
  }
  replaceChildren(...children: Node[]) {
    this.children = children;
  }
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  }
  querySelectorAll(query: string): Node[] {
    return this.children.flatMap((node) => [
      ...(query.split(",").includes(node.tag) ? [node] : []),
      ...node.querySelectorAll(query),
    ]);
  }
  removeAttribute() {}
}
function documentFixture(media = new Media()) {
  const classes = new Set<string>();
  const document = {
    documentElement: {
      classList: {
        toggle(name: string, force: boolean) {
          if (force) classes.add(name);
          else classes.delete(name);
        },
      },
    },
    defaultView: { matchMedia: vi.fn(() => media) },
    createElement: (tag: string) =>
      new Node(tag, document as unknown as Document),
  };
  return { document: document as unknown as Document, classes, media };
}
function required<T>(value: T | undefined): T {
  if (value === undefined) throw Error("Missing fixture control");
  return value;
}
const token = "00000000-0000-4000-8000-000000000001";
function projection(value = "light", source = "default"): WorkspaceSettings {
  return decodeWorkspaceSettings({
    version: 4,
    token,
    values: {
      font_size: { value: 14, source: "default" },
      font_family: { value: "monospace", source: "default" },
      line_height_percent: { value: 100, source: "default" },
      letter_spacing: { value: 0, source: "default" },
      font_fallback: { value: "monospace", source: "default" },
      font_ligatures: { value: false, source: "default" },
      cursor_blink: { value: false, source: "default" },
      copy_on_select: { value: false, source: "default" },
      right_click_paste: { value: false, source: "default" },
      ui_appearance: { value, source },
    },
  });
}
function fixture(value = "light") {
  const f = documentFixture(),
    life = new AbortController(),
    appearance = new WorkspaceAppearance(f.document, life.signal),
    dialog = new Node("dialog", f.document),
    fields = new Node("div", f.document),
    status = new Node("p", f.document),
    changed = vi.fn((p: WorkspaceSettings) =>
      appearance.apply(p.values.ui_appearance?.value ?? "light"),
    ),
    dependencies = {
      read: vi.fn(async () =>
        projection(value, value === "light" ? "default" : "user"),
      ),
      save: vi.fn(
        async (_nonce: string, _token: string, change: SettingsChange) =>
          projection(
            change.field === "ui_appearance" ? change.value : "light",
            change.field === "reset_ui_appearance" ? "default" : "user",
          ),
      ),
    };
  appearance.apply(value === "dark" || value === "system" ? value : "light");
  const editor = new WorkspaceSettingsEditor(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      fields: fields as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      signal: life.signal,
      nonce: () => token,
      changed,
      working() {},
    },
    dependencies,
  );
  return {
    ...f,
    life,
    appearance,
    dialog,
    fields,
    status,
    changed,
    dependencies,
    editor,
    row: () => required(fields.children[0]),
    select: () => required(fields.querySelectorAll("select")[0]),
  };
}
async function settle() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}
it("applies explicit modes and follows system only for windows choosing it", () => {
  const a = documentFixture(),
    b = documentFixture(a.media),
    alive = new AbortController();
  const first = new WorkspaceAppearance(a.document, alive.signal),
    second = new WorkspaceAppearance(b.document, new AbortController().signal);
  expect(first.apply("system")).toBe(true);
  second.apply("light");
  a.media.change(true);
  expect(a.classes.has("dark")).toBe(true);
  expect(b.classes.has("light")).toBe(true);
  first.apply("light");
  a.media.change(true);
  expect(a.classes.has("light")).toBe(true);
  first.apply("dark");
  alive.abort();
  a.media.change(false);
  expect(first.apply("light")).toBe(false);
  expect(a.classes.has("dark")).toBe(true);
});
it("unavailable system preference preserves the last displayed UI", () => {
  const f = documentFixture();
  const view = f.document.defaultView;
  if (!view) throw Error("Missing fixture window");
  view.matchMedia = () => {
    throw Error("unavailable");
  };
  const appearance = new WorkspaceAppearance(
    f.document,
    new AbortController().signal,
  );
  appearance.apply("dark");
  expect(appearance.apply("system")).toBe(false);
  expect(f.classes.has("dark")).toBe(true);
});
it("draft previews are local and closing performs no write or root switch", async () => {
  const f = fixture();
  await f.editor.open();
  f.changed.mockClear();
  f.select().value = "dark";
  f.select().dispatchEvent(new Event("change"));
  expect(required(f.row().children[4]).className).toBe(
    "workspace-appearance-preview dark",
  );
  expect(f.classes.has("light")).toBe(true);
  expect(f.changed).not.toHaveBeenCalled();
  expect(f.dependencies.save).not.toHaveBeenCalled();
  f.editor.close();
  expect(f.classes.has("light")).toBe(true);
  f.editor.dispose();
  f.life.abort();
});
it("only a confirmed save updates the root and reset restores default without terminal edits", async () => {
  const f = fixture();
  await f.editor.open();
  let finish!: (p: WorkspaceSettings) => void;
  f.dependencies.save.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  f.select().value = "dark";
  const staleRow = f.row();
  staleRow.dispatchEvent(new Event("submit"));
  staleRow.dispatchEvent(new Event("submit"));
  expect(f.classes.has("light")).toBe(true);
  expect(f.dependencies.save).toHaveBeenCalledOnce();
  finish(projection("dark", "user"));
  await settle();
  expect(f.classes.has("dark")).toBe(true);
  expect(f.status.textContent).toContain("界面外观已应用");
  staleRow.dispatchEvent(new Event("submit"));
  expect(f.dependencies.save).toHaveBeenCalledOnce();
  required(f.row().children[3]).dispatchEvent(new Event("click"));
  await settle();
  expect(f.dependencies.save).toHaveBeenLastCalledWith(
    token,
    token,
    { field: "reset_ui_appearance" },
    expect.any(AbortSignal),
  );
  expect(f.classes.has("light")).toBe(true);
  expect(required(f.row().children[1]).textContent).toBe("内置默认");
  f.editor.dispose();
  f.life.abort();
});
it("save failure retains the displayed mode and requires reread", async () => {
  const f = fixture();
  await f.editor.open();
  f.select().value = "dark";
  const old = f.row();
  f.dependencies.save.mockRejectedValueOnce(
    Error("workspace.settings_save_unconfirmed"),
  );
  f.row().dispatchEvent(new Event("submit"));
  await settle();
  expect(f.classes.has("light")).toBe(true);
  expect(f.status.textContent).toContain("保存未确认");
  old.dispatchEvent(new Event("submit"));
  expect(f.dependencies.save).toHaveBeenCalledOnce();
  f.editor.dispose();
  f.life.abort();
});
it("late save after close cannot change appearance and old preview stops following system", async () => {
  const f = fixture();
  await f.editor.open();
  f.select().value = "system";
  f.select().dispatchEvent(new Event("change"));
  const preview = required(f.row().children[4]);
  let finish!: (p: WorkspaceSettings) => void;
  f.dependencies.save.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  f.row().dispatchEvent(new Event("submit"));
  f.editor.close();
  f.media.change(true);
  finish(projection("dark", "user"));
  await settle();
  expect(f.classes.has("light")).toBe(true);
  expect(preview.className).toBe("workspace-appearance-preview light");
  f.editor.dispose();
  f.life.abort();
});
