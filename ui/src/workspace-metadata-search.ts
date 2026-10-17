/** Search and rank bounded display metadata; never derive IDs, targets or permissions. */
function normalized(value: string): string {
  return value.normalize("NFKC").toLowerCase();
}
function score(needle: string, value: string): number | undefined {
  if (value === needle) return 0;
  if (value.startsWith(needle)) return 4;
  const contiguous = value.indexOf(needle);
  if (contiguous >= 0) return 12 + contiguous / (value.length + 1);
  let first = -1,
    last = -1,
    gaps = 0;
  for (const char of needle) {
    const next = value.indexOf(char, last + 1);
    if (next < 0) return undefined;
    if (first < 0) first = next;
    else gaps += next - last - 1;
    last = next;
  }
  return 100 + first + gaps;
}
export function rankMetadata<T>(
  rows: readonly T[],
  query: string,
  fields: (row: T) => readonly string[],
): readonly T[] {
  if (query.length > 256) return [];
  const terms = normalized(query).trim().split(/\s+/u).filter(Boolean);
  if (!terms.length) return rows;
  const ranked: { row: T; index: number; score: number }[] = [];
  for (const [index, row] of rows.entries()) {
    const texts = fields(row).map(normalized);
    let total = 0,
      matched = true;
    for (const term of terms) {
      let best: number | undefined;
      for (const text of texts) {
        const current = score(term, text);
        if (current !== undefined && (best === undefined || current < best))
          best = current;
      }
      if (best === undefined) {
        matched = false;
        break;
      }
      total += best;
    }
    if (matched) ranked.push({ row, index, score: total });
  }
  ranked.sort((a, b) => a.score - b.score || a.index - b.index);
  return ranked.map((item) => item.row);
}
