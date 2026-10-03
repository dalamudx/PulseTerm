import { describe, expect, it } from "vitest";
import { type DigestScreen, screenDigest } from "./screen-digest";

const base: DigestScreen = {
  cursor: [0, 0],
  alternate: false,
  cells: [
    [
      {
        text: "中́",
        width: 2,
        foreground: "rgb:1,2,3",
        background: "default",
        bold: false,
        italic: false,
        underline: false,
        inverse: false,
      },
    ],
  ],
};
describe("visual digest v1", () => {
  it("is deterministic with multibyte text", () => {
    expect(screenDigest(base)).toMatch(/^[0-9a-f]{16}$/);
    expect(screenDigest(structuredClone(base))).toBe(screenDigest(base));
  });
  it("observes text, style, cursor and buffer", () => {
    for (const mutate of [
      (s: DigestScreen) => {
        s.cursor = [1, 0];
      },
      (s: DigestScreen) => {
        s.alternate = true;
      },
      (s: DigestScreen) => {
        const c = s.cells[0]?.[0];
        if (c) c.bold = true;
      },
      (s: DigestScreen) => {
        const c = s.cells[0]?.[0];
        if (c) c.text = "文";
      },
    ]) {
      const s = structuredClone(base);
      mutate(s);
      expect(screenDigest(s)).not.toBe(screenDigest(base));
    }
  });
  it("rejects invalid coordinates", () => {
    expect(() => screenDigest({ ...base, cursor: [-1, 0] })).toThrow();
  });
});
