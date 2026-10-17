/** DOM uses NBSP to paint terminal blanks. Written trailing spaces and empty
 * trailing cells paint the same glyphs; retain all other whitespace and text. */
export function projectedRowText(text: string): string {
  return text.replaceAll("\u00a0", " ").replace(/ +$/, "");
}
