import { describe, expect, it } from "vitest";
import { answerCliInteractive } from "./workspace-cli-interactive";
import { decodeInteractivePrompt } from "./workspace-ssh-interactive";

class Element extends EventTarget {
  children: Element[] = [];
  parent?: Element;
  open = false;
  value = "";
  textContent = "";
  constructor(
    readonly ownerDocument: Doc,
    readonly tag: string,
  ) {
    super();
  }
  append(...children: Element[]) {
    for (const child of children) {
      child.parent = this;
      this.children.push(child);
    }
  }
  setAttribute() {}
  focus() {}
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  }
  remove() {
    if (this.parent)
      this.parent.children = this.parent.children.filter(
        (child) => child !== this,
      );
    this.parent = undefined;
  }
  descendants(tag: string): Element[] {
    return this.children.flatMap((child) => [
      ...(child.tag === tag ? [child] : []),
      ...child.descendants(tag),
    ]);
  }
}
class Doc {
  body = new Element(this, "body");
  createElement(tag: string) {
    return new Element(this, tag);
  }
  createTextNode(text: string) {
    const node = this.createElement("#text");
    node.textContent = text;
    return node;
  }
}
const prompt = decodeInteractivePrompt({
  version: 1,
  round: "11111111-1111-4111-8111-111111111111",
  name: "Literal <server>",
  instructions: "Fixture only",
  prompts: [
    { text: "Account", echo: true },
    { text: "Code", echo: false },
  ],
});

describe("CLI interactive dialog lifecycle", () => {
  for (const reason of ["cancel", "close", "abort"] as const) {
    it(`${reason} finishes the answer waiter and clears both visible and hidden drafts`, async () => {
      const doc = new Doc(),
        controller = new AbortController();
      const result = answerCliInteractive(
        doc as unknown as Document,
        prompt,
        controller.signal,
      );
      const dialog = doc.body.children[0];
      expect(dialog?.open).toBe(true);
      const fields = doc.body.descendants("input");
      expect(fields).toHaveLength(2);
      for (const field of fields) field.value = "discard-this-fixture";
      if (reason === "abort") controller.abort();
      else if (reason === "close") dialog?.close();
      else {
        const event = new Event("cancel", { cancelable: true });
        dialog?.dispatchEvent(event);
        expect(event.defaultPrevented).toBe(true);
      }
      expect(await result).toBeUndefined();
      expect(fields.every((field) => field.value === "")).toBe(true);
      expect(doc.body.children).toHaveLength(0);
      expect(controller.signal.aborted).toBe(reason === "abort");
    });
  }
  it("submits once and removes the dialog without aborting the original request", async () => {
    const doc = new Doc(),
      controller = new AbortController();
    const result = answerCliInteractive(
      doc as unknown as Document,
      prompt,
      controller.signal,
    );
    const fields = doc.body.descendants("input");
    fields.forEach((field, index) => {
      field.value = String(index);
    });
    doc.body
      .descendants("form")[0]
      ?.dispatchEvent(new Event("submit", { cancelable: true }));
    const answers = await result;
    expect(answers).toEqual(["0", "1"]);
    expect(fields.every((field) => field.value === "")).toBe(true);
    expect(doc.body.children).toHaveLength(0);
    expect(controller.signal.aborted).toBe(false);
    answers?.fill("");
  });
});
