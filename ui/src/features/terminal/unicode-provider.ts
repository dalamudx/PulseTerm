// Use xterm's PUBLIC experimental Unicode provider interface, not private core
// state. Width/property encoding must be revalidated when upgrading xterm.
import type { IUnicodeVersionProvider } from "@xterm/headless";
import { unicodeVersion, widthRanges } from "./gen/widths";
export class SharedUnicodeProvider implements IUnicodeVersionProvider {
  readonly version = `${unicodeVersion}-pulse-scalar`;
  wcwidth(codepoint: number): 0 | 1 | 2 {
    if (!Number.isInteger(codepoint) || codepoint < 0 || codepoint > 0x10ffff)
      return 0;
    let low = 0;
    let high = widthRanges.length;
    while (low < high) {
      const mid = (low + high) >>> 1;
      const range = widthRanges[mid];
      if (!range) throw new Error("Malformed generated Unicode table");
      if (codepoint < range[0]) high = mid;
      else if (codepoint > range[1]) low = mid + 1;
      else return range[2] as 0 | 2;
    }
    return 1;
  }
  // Compatible scalar combining behavior; deliberately NOT grapheme cluster
  // collapsing. This follows xterm's public provider property bit layout.
  charProperties(codepoint: number, preceding: number): number {
    const width = this.wcwidth(codepoint);
    const priorWidth = (preceding >>> 1) & 3;
    const join = width === 0 && priorWidth > 0;
    return ((join ? priorWidth : width) << 1) | Number(join);
  }
}
