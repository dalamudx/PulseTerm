import { expect, it, vi } from "vitest";
import { confirmWorkspaceClose } from "../../workspace-close";
import { applyUiLanguage } from "../../workspace-language";
import { confirmTerminalPaste } from "./paste-dialog";
import { assessPaste } from "./paste-policy";

class NodeFixture {
  children: NodeFixture[] = [];
  attributes = new Map<string, string>();
  listeners = new Map<string, EventListener[]>();
  id = "";
  className = "";
  textContent = "";
  value = "";
  type = "";
  wrap = "";
  readOnly = false;
  rows = 0;
  autofocus = false;
  open = false;
  removed = false;
  constructor(readonly doc: DocFixture) {}
  get ownerDocument() {
    return this.doc;
  }
  setAttribute(k: string, v: string) {
    this.attributes.set(k, v);
  }
  append(...nodes: NodeFixture[]) {
    this.children.push(...nodes);
  }
  addEventListener(k: string, f: EventListener) {
    this.listeners.set(k, [...(this.listeners.get(k) ?? []), f]);
  }
  removeEventListener(k: string, f: EventListener) {
    this.listeners.set(
      k,
      (this.listeners.get(k) ?? []).filter((x) => x !== f),
    );
  }
  dispatch(k: string) {
    const preventDefault = vi.fn();
    for (const f of this.listeners.get(k) ?? [])
      f({ preventDefault } as unknown as Event);
    return preventDefault;
  }
  showModal() {
    if (this.doc.fail) throw Error("unavailable");
    this.open = true;
  }
  close() {
    this.open = false;
    this.dispatch("close");
  }
  remove() {
    this.removed = true;
    this.doc.body.children = this.doc.body.children.filter((x) => x !== this);
  }
  focus() {
    this.doc.focused = this;
  }
}
class DocFixture {
  documentElement = { lang: "zh-CN" };
  body = new NodeFixture(this);
  focused: NodeFixture | undefined;
  fail = false;
  createElement() {
    return new NodeFixture(this);
  }
}
function fixture(text = "a\nb", english = false) {
  const doc = new DocFixture();
  applyUiLanguage(doc as unknown as Document, english ? "en" : "zh-CN");
  const controller = new AbortController();
  const job = confirmTerminalPaste(
    doc as unknown as Document,
    text,
    assessPaste(text),
    controller.signal,
  );
  const dialog = doc.body.children[0];
  if (!dialog) throw Error("missing test dialog");
  const buttons = dialog.children[3]?.children;
  return {
    doc,
    controller,
    job,
    dialog,
    cancel: buttons?.[0],
    accept: buttons?.[1],
  };
}
it("plain-text preview cannot become HTML, default focus is Cancel, English labels exist", async () => {
  const f = fixture("<script>\nhello", true);
  expect(f.dialog.children[2]?.value).toBe("<script>\nhello");
  expect(f.dialog.children[2]?.readOnly).toBe(true);
  expect(f.doc.focused).toBe(f.cancel);
  expect(f.cancel?.textContent).toBe("Cancel");
  expect(f.accept?.textContent).toBe("Paste once");
  f.cancel?.dispatch("click");
  expect(await f.job).toBe(false);
  expect(f.dialog.removed).toBe(true);
});
it("directional/BOM characters are visible escaped, not interpreted as preview direction", async () => {
  const f = fixture("a\u061c\u202e\u2060\ufeff");
  expect(f.dialog.children[2]?.value).toBe(
    "a\\u{61c}\\u{202e}\\u{2060}\\u{feff}",
  );
  f.cancel?.dispatch("click");
  expect(await f.job).toBe(false);
});
it("explicit acceptance resolves once then removes handlers and dialog", async () => {
  const f = fixture();
  f.accept?.dispatch("click");
  expect(await f.job).toBe(true);
  expect(f.dialog.removed).toBe(true);
  expect(f.accept?.listeners.get("click")).toHaveLength(0);
  f.accept?.dispatch("click");
  expect(f.doc.body.children).toHaveLength(0);
});
it("Escape/external close cancel, never default approval", async () => {
  let f = fixture();
  expect(f.dialog.dispatch("cancel")).toHaveBeenCalledOnce();
  expect(await f.job).toBe(false);
  f = fixture();
  f.dialog.close();
  expect(await f.job).toBe(false);
  expect(f.dialog.removed).toBe(true);
});
it("scope abort dismisses synchronously and late button cannot approve", async () => {
  const f = fixture();
  f.controller.abort();
  expect(f.dialog.removed).toBe(true);
  f.accept?.dispatch("click");
  expect(await f.job).toBe(false);
  expect(f.doc.body.children).toHaveLength(0);
});
it("pre-aborted and unsupported native dialog send no decision authority", async () => {
  const doc = new DocFixture(),
    controller = new AbortController();
  controller.abort();
  expect(
    await confirmTerminalPaste(
      doc as unknown as Document,
      "a\nb",
      assessPaste("a\nb"),
      controller.signal,
    ),
  ).toBe(false);
  expect(doc.body.children).toHaveLength(0);
  doc.fail = true;
  expect(
    await confirmTerminalPaste(
      doc as unknown as Document,
      "a\nb",
      assessPaste("a\nb"),
      new AbortController().signal,
    ),
  ).toBe(false);
  expect(doc.body.children).toHaveLength(0);
});
it("only one document decision exists; paste cannot open over destructive close", async () => {
  const doc = new DocFixture(),
    signal = new AbortController().signal;
  const closing = confirmWorkspaceClose(
    doc as unknown as Document,
    "1",
    signal,
  );
  expect(doc.body.children).toHaveLength(1);
  expect(
    await confirmTerminalPaste(
      doc as unknown as Document,
      "a\nb",
      assessPaste("a\nb"),
      signal,
    ),
  ).toBe(false);
  expect(doc.body.children).toHaveLength(1);
  const dialog = doc.body.children[0];
  expect(dialog?.children[0]?.textContent).toBe("关闭会话 1？");
  const buttons = dialog?.children[2]?.children;
  expect(doc.focused).toBe(buttons?.[0]);
  expect(buttons?.[0]?.textContent).toBe("保留会话");
  buttons?.[0]?.dispatch("click");
  expect(await closing).toBe(false);
});
it("DOM construction exception cannot permanently occupy a decision slot", async () => {
  const doc = new DocFixture(),
    create = doc.createElement.bind(doc);
  let fail = true;
  doc.createElement = () => {
    if (fail) throw Error("DOM fault");
    return create();
  };
  await expect(
    confirmWorkspaceClose(
      doc as unknown as Document,
      "1",
      new AbortController().signal,
    ),
  ).rejects.toThrow("DOM fault");
  fail = false;
  const job = confirmWorkspaceClose(
    doc as unknown as Document,
    "1",
    new AbortController().signal,
  );
  expect(doc.body.children).toHaveLength(1);
  doc.body.children[0]?.children[2]?.children[0]?.dispatch("click");
  expect(await job).toBe(false);
});
