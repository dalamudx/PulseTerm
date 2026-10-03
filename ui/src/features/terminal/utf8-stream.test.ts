import { expect, it } from "vitest";
import { widthHash } from "./gen/widths";
import { SharedUnicodeProvider } from "./unicode-provider";
import { Utf8Stream } from "./utf8-stream";

it("preserves all scalar partitions, BOM, controls, and EOF exactly once", () => {
  const value = "\ufeff中é 👨‍👩‍👧‍👦\x1b[31m!";
  const bytes = new TextEncoder().encode(value);
  for (let size = 1; size <= bytes.length; size++) {
    const stream = new Utf8Stream();
    let actual = "";
    for (let offset = 0; offset < bytes.length; offset += size)
      actual += stream.feed(bytes.subarray(offset, offset + size));
    expect(actual + stream.finish()).toBe(value);
    expect(stream.finish()).toBe("");
    expect(() => stream.feed(new Uint8Array())).toThrow("ended");
  }
});
it("does not submit decoder-only empty chunks, and replaces an incomplete tail", () => {
  const stream = new Utf8Stream();
  expect(stream.feed(new Uint8Array([0xf0]))).toBe("");
  expect(stream.feed(new Uint8Array([0x9f]))).toBe("");
  expect(stream.finish()).toBe("\ufffd");
  expect(stream.finish()).toBe("");
});
it("enforces input chunk limit before consuming bytes", () => {
  const stream = new Utf8Stream();
  expect(() => stream.feed(new Uint8Array(65537))).toThrow("64 KiB");
  expect(stream.feed(new Uint8Array([65]))).toBe("A");
});
it("classifies every codepoint identically to Rust generated fingerprint", () => {
  const provider = new SharedUnicodeProvider();
  let hash = 2166136261;
  for (let cp = 0; cp <= 0x10ffff; cp++)
    hash = Math.imul(hash ^ provider.wcwidth(cp), 16777619) >>> 0;
  expect(hash).toBe(widthHash);
  expect(provider.wcwidth(0x1fae8)).toBe(2);
  expect(provider.wcwidth(-1)).toBe(0);
  expect(provider.wcwidth(Number.NaN)).toBe(0);
});
it("property encoding retains prior wide width for a combining scalar", () => {
  const provider = new SharedUnicodeProvider();
  const wide = provider.charProperties(0x4e2d, 0);
  const combined = provider.charProperties(0x301, wide);
  expect(wide).toBe(4);
  expect(combined).toBe(5);
  expect(provider.charProperties(0x41, combined)).toBe(2);
});
