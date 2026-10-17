export interface HistoryPage {
  revision: string;
  totalRows: number;
  coldRows: number;
  truncated: boolean;
  start: number;
  end: number;
  rows: readonly { text: string; wrapped: boolean }[];
}
export function decodeHistoryPage(value: unknown, screenRows = 0): HistoryPage {
  const invalid = () => new Error("workspace.history_invalid");
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw invalid();
  const page = value as Record<string, unknown>;
  if (
    Object.keys(page).sort().join() !==
    "coldRows,end,revision,rows,start,totalRows,truncated"
  )
    throw invalid();
  if (
    typeof page.revision !== "string" ||
    !/^(0|[1-9]\d{0,19})$/.test(page.revision) ||
    (page.revision.length === 20 && page.revision > "18446744073709551615")
  )
    throw invalid();
  const integer = (n: unknown): n is number =>
    typeof n === "number" && Number.isSafeInteger(n) && n >= 0;
  if (
    !integer(page.totalRows) ||
    !integer(screenRows) ||
    screenRows > 256 ||
    page.totalRows < screenRows ||
    page.totalRows > 100000 + screenRows ||
    !integer(page.coldRows) ||
    page.coldRows > page.totalRows - screenRows ||
    typeof page.truncated !== "boolean" ||
    !integer(page.start) ||
    !integer(page.end) ||
    page.start > page.end ||
    page.end > page.totalRows ||
    page.end - page.start > 200 ||
    (page.end > 0 && page.start === page.end) ||
    !Array.isArray(page.rows) ||
    page.rows.length !== page.end - page.start
  )
    throw invalid();
  let bytes = 0;
  for (const row of page.rows) {
    if (
      !row ||
      typeof row !== "object" ||
      Object.keys(row).sort().join() !== "text,wrapped" ||
      typeof row.text !== "string" ||
      row.text.length > 131072 ||
      typeof row.wrapped !== "boolean"
    )
      throw invalid();
    bytes += new TextEncoder().encode(row.text).length;
    if (bytes > 131072) throw invalid();
  }
  return page as unknown as HistoryPage;
}
