import { expect, it, vi } from "vitest";
import { nonceDocument } from "./nonce-document";

function fixture(nonce: string | undefined) {
  const made: { tag: string; setAttribute: ReturnType<typeof vi.fn> }[] = [];
  const doc = {
    querySelector: () => (nonce === undefined ? null : { nonce }),
    createElement(tag: string) {
      const element = { tag, setAttribute: vi.fn() };
      made.push(element);
      return element;
    },
    createTextNode() {
      expect(this).toBe(doc);
      return "text";
    },
  };
  return { doc: doc as unknown as Document, made };
}
it("uses only the actual page nonce and only on locally created styles", () => {
  const f = fixture("random-page-value"),
    proxy = nonceDocument(f.doc);
  proxy.createElement("style");
  proxy.createElement("div");
  expect(f.made[0]?.setAttribute).toHaveBeenCalledWith(
    "nonce",
    "random-page-value",
  );
  expect(f.made[1]?.setAttribute).not.toHaveBeenCalled();
  expect(proxy.createTextNode("x")).toBe("text");
});
it("does not mutate the global or original document createElement", () => {
  const f = fixture("page-value");
  const original = f.doc.createElement;
  const proxy = nonceDocument(f.doc);
  expect(f.doc.createElement).toBe(original);
  const own = f.doc.createElement("style");
  expect(
    (own as unknown as { setAttribute: ReturnType<typeof vi.fn> }).setAttribute,
  ).not.toHaveBeenCalled();
  proxy.createElement("STYLE");
  expect(f.made[1]?.setAttribute).toHaveBeenCalledWith("nonce", "page-value");
});
it("fails closed without a runtime Tauri nonce", () => {
  for (const nonce of [undefined, "", "__TAURI_STYLE_NONCE__"])
    expect(() => nonceDocument(fixture(nonce).doc)).toThrow(
      "style_nonce_missing",
    );
});
