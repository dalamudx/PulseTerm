import { decodeHistoryPage, type HistoryPage } from "./workspace-history-page";
import { message, renderUiMessage, type UiMessage } from "./workspace-language";

export interface HistorySearchRequest {
  query: string;
  matchCase: boolean;
  backwards: boolean;
  alternate: boolean;
  regex: boolean;
  wholeWord: boolean;
  cursor: number | null;
  revision: string | null;
}
interface SearchResult {
  revision: string;
  totalRows: number;
  screenRows: number;
  alternate: boolean;
  regex: boolean;
  wholeWord: boolean;
  cursor: number;
  complete: boolean;
  matchedRow: number | null;
  matchedEnd: number | null;
  page: HistoryPage | null;
}
function decode(value: unknown): SearchResult {
  const invalid = () => new Error("workspace.history_invalid");
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw invalid();
  const v = value as Record<string, unknown>;
  const integer = (n: unknown): n is number =>
    typeof n === "number" && Number.isSafeInteger(n) && n >= 0;
  if (
    Object.keys(v).sort().join() !==
      "alternate,complete,cursor,matchedEnd,matchedRow,page,regex,revision,screenRows,totalRows,wholeWord" ||
    typeof v.alternate !== "boolean" ||
    typeof v.regex !== "boolean" ||
    typeof v.wholeWord !== "boolean" ||
    typeof v.revision !== "string" ||
    !/^(0|[1-9]\d{0,19})$/.test(v.revision) ||
    (v.revision.length === 20 && v.revision > "18446744073709551615") ||
    !integer(v.screenRows) ||
    v.screenRows < 1 ||
    v.screenRows > 256 ||
    !integer(v.totalRows) ||
    v.totalRows < v.screenRows ||
    v.totalRows > 100000 + v.screenRows ||
    (v.alternate && v.totalRows !== v.screenRows) ||
    !integer(v.cursor) ||
    v.cursor > v.totalRows ||
    typeof v.complete !== "boolean" ||
    (v.matchedRow !== null &&
      (!integer(v.matchedRow) || v.matchedRow >= v.totalRows)) ||
    (v.matchedEnd !== null &&
      (!integer(v.matchedEnd) || v.matchedEnd > v.totalRows)) ||
    (v.page === null) !== (v.matchedRow === null) ||
    (v.matchedRow === null) !== (v.matchedEnd === null) ||
    (v.matchedRow !== null && Number(v.matchedEnd) <= Number(v.matchedRow))
  )
    throw invalid();
  if (v.page !== null) {
    const page = decodeHistoryPage(v.page, v.screenRows);
    if (
      page.revision !== v.revision ||
      page.totalRows !== v.totalRows ||
      (v.alternate && page.truncated) ||
      page.end !== v.matchedEnd
    )
      throw invalid();
  }
  return v as unknown as SearchResult;
}

/** One authorized literal pass; busy/errors never retry or change the source revision. */
export class HistoryFinder {
  busy = false;
  stopping = false;
  expired = false;
  private messageValue: string | UiMessage = "";
  get message(): string {
    return typeof this.messageValue === "string"
      ? this.messageValue
      : renderUiMessage("zh-CN", this.messageValue);
  }
  set message(value: string | UiMessage) {
    this.messageValue = value;
  }
  get presentation(): string | UiMessage {
    return this.messageValue;
  }
  matchedRow: number | undefined;
  matchedEnd: number | undefined;
  private revision: string | undefined;
  private query = "";
  private matchCase = false;
  private alternate = false;
  private regex = false;
  private wholeWord = false;
  private disposed = false;
  private operation = 0;
  constructor(
    private readonly read: (request: HistorySearchRequest) => Promise<unknown>,
    private readonly current: () => boolean,
    private readonly found: (page: HistoryPage) => void,
    private readonly changed: () => void,
  ) {}
  /** Stop scheduling batches; retain admission until the current native read returns. */
  stop(): void {
    if (this.disposed || !this.busy || this.stopping) return;
    this.operation++;
    this.stopping = true;
    this.message = "正在停止查找，当前页已保留。";
    this.changed();
  }
  clear(): void {
    this.operation++;
    this.stopping = false;
    this.revision = undefined;
    this.matchedRow = this.matchedEnd = undefined;
    this.expired = false;
    this.message = "";
    this.changed();
  }
  dispose(): void {
    this.disposed = true;
    this.operation++;
  }
  async find(
    query: string,
    matchCase: boolean,
    backwards: boolean,
    alternate = false,
    regex = false,
    wholeWord = false,
  ): Promise<void> {
    if (this.disposed || this.busy) return;
    if (
      !query ||
      new TextEncoder().encode(query).length > 512 ||
      Array.from(query).some((char) => {
        const code = char.codePointAt(0) ?? 0;
        return code < 32 || (code >= 127 && code <= 159);
      })
    ) {
      this.message = "请输入不含控制字符的查找文字，最多 512 字节。";
      this.changed();
      return;
    }
    const same =
      this.query === query &&
      this.matchCase === matchCase &&
      this.alternate === alternate &&
      this.regex === regex &&
      this.wholeWord === wholeWord &&
      !this.expired;
    const request: HistorySearchRequest = {
      query,
      matchCase,
      backwards,
      alternate,
      regex,
      wholeWord,
      revision:
        same && this.matchedRow !== undefined ? (this.revision ?? null) : null,
      cursor:
        same && this.matchedRow !== undefined
          ? backwards
            ? this.matchedRow
            : (this.matchedEnd ?? null)
          : null,
    };
    const operation = ++this.operation;
    this.busy = true;
    const sourceName = alternate ? "备用屏" : "历史与主屏";
    this.message = message("正在查找{0}…", message(sourceName));
    this.changed();
    let total: number | undefined;
    let screenRows: number | undefined;
    try {
      for (let turn = 0; turn <= 100256; turn++) {
        if (!this.current()) throw new Error("workspace.history_changed");
        const result = decode(await this.read({ ...request }));
        if (this.disposed || operation !== this.operation) return;
        if (!this.current()) throw new Error("workspace.history_changed");
        const start = request.cursor ?? (backwards ? result.totalRows : 0);
        const delta = backwards ? start - result.cursor : result.cursor - start;
        if (
          result.alternate !== alternate ||
          result.regex !== regex ||
          result.wholeWord !== wholeWord ||
          (request.revision !== null && request.revision !== result.revision) ||
          (total !== undefined && total !== result.totalRows) ||
          (screenRows !== undefined && screenRows !== result.screenRows) ||
          delta < 0 ||
          delta > 2048 ||
          (!result.complete && result.matchedRow === null && delta === 0) ||
          result.complete !==
            (result.cursor === (backwards ? 0 : result.totalRows)) ||
          (result.matchedRow !== null &&
            (result.cursor !==
              (backwards ? result.matchedRow : result.matchedEnd) ||
              delta === 0 ||
              (backwards
                ? Number(result.matchedEnd) > start
                : result.matchedRow < start)))
        )
          throw new Error("workspace.history_invalid");
        total = result.totalRows;
        screenRows = result.screenRows;
        if (result.page !== null) {
          this.query = query;
          this.matchCase = matchCase;
          this.alternate = alternate;
          this.regex = regex;
          this.wholeWord = wholeWord;
          this.revision = result.revision;
          this.matchedRow = result.matchedRow ?? undefined;
          this.matchedEnd = result.matchedEnd ?? undefined;
          this.expired = false;
          const historyRows = result.totalRows - result.screenRows;
          const location = (row: number) =>
            alternate
              ? message("备用屏第 {0} 行", row + 1)
              : row < historyRows
                ? message("历史第 {0} 行", row + 1)
                : message("主屏第 {0} 行", row - historyRows + 1);
          this.message = message(
            "匹配{0}{1}。{2}",
            location(Number(result.matchedRow)),
            Number(result.matchedEnd) > Number(result.matchedRow) + 1
              ? message("至{0}", location(Number(result.matchedEnd) - 1))
              : "",
            result.page.start > Number(result.matchedRow)
              ? message("片段超过一页，仅显示末尾部分。")
              : "",
          );
          this.found(result.page);
          return;
        }
        if (result.complete) {
          this.message =
            same && this.matchedRow !== undefined
              ? message(
                  "已到{0}内容，没有更多匹配。",
                  message(backwards ? "最早" : "最新"),
                )
              : message("{0}中没有匹配。", message(sourceName));
          return;
        }
        request.cursor = result.cursor;
        request.revision = result.revision;
        this.message = message(
          "正在查找{0}… {1} / {2} 行。",
          message(sourceName),
          result.cursor,
          total ?? 0,
        );
        this.changed();
      }
      throw new Error("workspace.history_invalid");
    } catch (error) {
      if (this.disposed || operation !== this.operation) return;
      this.expired = true;
      this.message = String(error).includes("workspace.history_changed")
        ? "终端内容或会话已变化，当前页已保留。请重新查找。"
        : String(error).includes("workspace.history_pattern")
          ? "正则表达式无效或超过编译限制。请修改表达式，当前页已保留。"
          : String(error).includes("workspace.history_too_large")
            ? "内容超过单次查找预算，当前页已保留。可关闭正则和整词匹配后重试，或缩小范围。"
            : alternate &&
                String(error).includes("workspace.history_unavailable")
              ? "备用屏未启用或保留内容不可用，当前页已保留。请选择历史与主屏，或启用备用屏后重新查找。"
              : "查找未完成，当前页已保留。请稍后重新查找。";
    } finally {
      this.busy = false;
      if (this.stopping) {
        this.stopping = false;
        this.message = "已停止查找，当前页已保留。";
      }
      if (!this.disposed) this.changed();
    }
  }
}
