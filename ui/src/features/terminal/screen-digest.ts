// Visual checksum v1; not authentication or proof of hidden/parser/history parity.
export type DigestScreen = {
  cells: {
    text: string;
    foreground: string;
    background: string;
    width: number;
    bold: boolean;
    italic: boolean;
    underline: boolean;
    inverse: boolean;
  }[][];
  cursor: number[];
  alternate: boolean;
};
export function screenDigest(screen: DigestScreen): string {
  let hash = 0xcbf29ce484222325n;
  const feed = (data: Uint8Array) => {
    for (const byte of data)
      hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
  };
  const uint = (value: number) => {
    if (!Number.isSafeInteger(value) || value < 0 || value > 0xffffffff)
      throw new RangeError("digest integer");
    const b = new Uint8Array(4);
    new DataView(b.buffer).setUint32(0, value, true);
    feed(b);
  };
  const utf8 = new TextEncoder();
  feed(Uint8Array.of(1));
  uint(screen.cells[0]?.length ?? 0);
  uint(screen.cells.length);
  uint(screen.cursor[0] ?? 0);
  uint(screen.cursor[1] ?? 0);
  feed(Uint8Array.of(Number(screen.alternate)));
  for (const row of screen.cells)
    for (const cell of row) {
      for (const text of [cell.text, cell.foreground, cell.background]) {
        const b = utf8.encode(text);
        uint(b.length);
        feed(b);
      }
      feed(
        Uint8Array.of(
          cell.width,
          Number(cell.bold),
          Number(cell.italic),
          Number(cell.underline),
          Number(cell.inverse),
        ),
      );
    }
  return hash.toString(16).padStart(16, "0");
}
