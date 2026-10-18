import { invoke } from "@tauri-apps/api/core";
import { CompositionGuard } from "./keybindings/composition";
import { decodeHistoryPage, type HistoryPage } from "./workspace-history-page";
import { HistoryFinder } from "./workspace-history-search";
import { message, uiAttribute, uiNode, uiText } from "./workspace-language";

export { decodeHistoryPage, type HistoryPage } from "./workspace-history-page";

/** Presentation state only; native page and binding checks still authorize reads. */
export function historyViewAvailable(state: string | undefined): boolean {
  return state === "ready" || state === "ended";
}
/** One page in memory, only cursors for navigation; no terminal input or replay. */
export class HistoryPager {
  page: HistoryPage | undefined;
  busy = false;
  expired = false;
  error = "";
  private disposed = false;
  private newerEnds: number[] = [];
  constructor(
    private readonly read: (
      before?: number,
      revision?: string,
    ) => Promise<unknown>,
    private readonly current: () => boolean,
    private readonly changed: () => void,
  ) {}
  get canOlder(): boolean {
    return !this.busy && !this.expired && !!this.page?.start;
  }
  get canNewer(): boolean {
    return !this.busy && !this.expired && this.newerEnds.length > 0;
  }
  latest(): Promise<void> {
    return this.load("latest");
  }
  older(): Promise<void> {
    return this.canOlder ? this.load("older") : Promise.resolve();
  }
  newer(): Promise<void> {
    return this.canNewer ? this.load("newer") : Promise.resolve();
  }
  dispose(): void {
    this.disposed = true;
    this.page = undefined;
    this.newerEnds = [];
  }
  /** Accept a contextual search page, keeping only one page and resetting navigation. */
  acceptSearch(page: HistoryPage): void {
    if (this.disposed || this.busy || !this.current()) return;
    this.page = page;
    this.newerEnds = [];
    this.error = "";
    this.expired = false;
    this.changed();
  }
  private async load(direction: "latest" | "older" | "newer"): Promise<void> {
    if (this.disposed || this.busy) return;
    if (!this.current()) {
      this.expired = true;
      this.error = "当前会话已变化，请关闭后重新打开历史。";
      this.changed();
      return;
    }
    const previous = this.page;
    const before =
      direction === "latest"
        ? undefined
        : direction === "older"
          ? previous?.start
          : this.newerEnds.at(-1);
    this.busy = true;
    this.error = "";
    this.changed();
    try {
      const page = decodeHistoryPage(
        await this.read(
          before,
          direction === "latest" ? undefined : previous?.revision,
        ),
      );
      if (this.disposed) return;
      if (!this.current()) {
        this.expired = true;
        this.error = "当前会话已变化，请关闭后重新打开历史。";
        return;
      }
      if (
        (before !== undefined &&
          (page.end !== before ||
            page.revision !== previous?.revision ||
            page.totalRows !== previous?.totalRows ||
            page.coldRows !== previous?.coldRows ||
            page.truncated !== previous?.truncated)) ||
        (before === undefined && page.end !== page.totalRows)
      )
        throw new Error("workspace.history_invalid");
      if (direction === "latest") this.newerEnds = [];
      else if (direction === "older" && previous)
        this.newerEnds.push(previous.end);
      else this.newerEnds.pop();
      this.page = page;
      this.expired = false;
    } catch (error) {
      if (this.disposed) return;
      const message = String(error);
      if (message.includes("workspace.history_changed")) {
        this.expired = true;
        this.error = "终端内容已变化，当前页已保留。请点“最新历史”重新读取。";
      } else if (message.includes("workspace.history_busy")) {
        this.error = "终端正在处理其他操作，请稍后重试。";
      } else {
        this.expired = true;
        this.error = "无法读取历史，请确认会话仍可查看后重新读取。";
      }
    } finally {
      this.busy = false;
      if (!this.disposed) this.changed();
    }
  }
}

export function showWorkspaceHistory(options: {
  document: Document;
  nonce: string;
  session: string;
  current(): boolean;
  signal: AbortSignal;
  opener: HTMLElement;
}): void {
  if (
    options.signal.aborted ||
    !options.current() ||
    options.document.querySelector("dialog[open]")
  )
    return;
  const doc = options.document;
  const dialog = doc.createElement("dialog");
  dialog.className = "terminal-paste-dialog workspace-history";
  const title = doc.createElement("h2");
  title.id = "workspace-history-title";
  uiText(title, "终端历史与查找");
  dialog.setAttribute("aria-labelledby", title.id);
  const description = doc.createElement("p");
  uiText(
    description,
    "查找历史与主屏或当前备用屏，支持跨软换行，不跨硬换行。整词按 Unicode 单词边界，连续中文不自动分词。整词和正则每次定位一条完整逻辑行，不支持回溯引用或环视。查找不会向终端发送内容。",
  );
  const search = doc.createElement("div");
  search.className = "workspace-history-search";
  const scopeLabel = doc.createElement("label");
  scopeLabel.className = "workspace-history-label";
  uiText(scopeLabel, "查找范围");
  const searchScope = doc.createElement("select");
  uiAttribute(searchScope, "aria-label", "查找范围");
  for (const [value, text] of [
    ["primary", "历史与主屏"],
    ["alternate", "备用屏"],
  ] as const) {
    const option = doc.createElement("option");
    option.value = value;
    uiText(option, text);
    searchScope.append(option);
  }
  scopeLabel.append(searchScope);
  const label = doc.createElement("label");
  const query = doc.createElement("input");
  query.type = "search";
  query.className = "workspace-history-query";
  label.className = "workspace-history-label";
  query.id = "workspace-history-query";
  query.maxLength = 512;
  label.htmlFor = query.id;
  uiText(label, "查找文字");
  uiAttribute(query, "aria-label", "查找历史与主屏");
  const caseLabel = doc.createElement("label");
  const matchCase = doc.createElement("input");
  matchCase.type = "checkbox";
  caseLabel.className = "workspace-history-label";
  caseLabel.append(matchCase, uiNode(doc, "区分大小写"));
  const regexLabel = doc.createElement("label");
  regexLabel.className = "workspace-history-label";
  const regex = doc.createElement("input");
  regex.type = "checkbox";
  uiAttribute(regex, "aria-label", "正则表达式");
  regexLabel.append(regex, uiNode(doc, "正则表达式"));
  const wordLabel = doc.createElement("label");
  wordLabel.className = "workspace-history-label";
  const wholeWord = doc.createElement("input");
  wholeWord.type = "checkbox";
  uiAttribute(wholeWord, "aria-label", "整词匹配");
  uiAttribute(
    wholeWord,
    "title",
    "按 Unicode 单词边界匹配；连续中文不自动分词。",
  );
  wordLabel.append(wholeWord, uiNode(doc, "整词匹配"));
  const makeButton = (text: string) => {
    const node = doc.createElement("button");
    node.type = "button";
    uiText(node, text);
    return node;
  };
  const previous = makeButton("上一个"),
    next = makeButton("下一个");
  const stop = makeButton("停止查找");
  uiAttribute(previous, "title", "较早的匹配（Shift+Enter）");
  uiAttribute(next, "title", "较新的匹配（Enter）");
  search.append(
    scopeLabel,
    label,
    query,
    caseLabel,
    regexLabel,
    wordLabel,
    previous,
    next,
    stop,
  );
  const content = doc.createElement("pre");
  content.className = "workspace-history-content";
  content.tabIndex = 0;
  content.setAttribute("role", "region");
  uiAttribute(content, "aria-label", "当前页历史文本");
  const status = doc.createElement("p");
  status.setAttribute("role", "status");
  uiAttribute(status, "aria-label", "历史分页状态");
  status.setAttribute("aria-live", "polite");
  const actions = doc.createElement("div");
  actions.className = "terminal-paste-actions";
  const older = makeButton("更早"),
    newer = makeButton("较新"),
    latest = makeButton("最新历史"),
    done = makeButton("关闭");
  actions.append(older, newer, latest, done);
  const current = () => !options.signal.aborted && options.current();
  let searchPage: HistoryPage | undefined;
  let renderedPage: HistoryPage | undefined;
  let renderedStart: number | undefined;
  let renderedEnd: number | undefined;
  const render = () => {
    const busy = pager.busy || finder.busy;
    older.disabled = busy || !!searchPage || !pager.canOlder;
    newer.disabled = busy || !!searchPage || !pager.canNewer;
    latest.disabled = busy || !current();
    searchScope.disabled = regex.disabled = busy || !current();
    previous.disabled = next.disabled = busy || !query.value || !current();
    stop.hidden = !finder.busy;
    stop.disabled = finder.stopping || !current();
    content.setAttribute("aria-busy", String(busy));
    const page = searchPage ?? pager.page;
    if (
      page &&
      (page !== renderedPage ||
        finder.matchedRow !== renderedStart ||
        finder.matchedEnd !== renderedEnd)
    ) {
      content.replaceChildren();
      for (const [index, row] of page.rows.entries()) {
        const node = doc.createElement(
          finder.matchedRow !== undefined &&
            page.start + index >= finder.matchedRow &&
            page.start + index < (finder.matchedEnd ?? 0)
            ? "mark"
            : "span",
        );
        node.textContent = row.text;
        content.append(node);
        if (index + 1 < page.rows.length)
          content.append(doc.createTextNode("\n"));
      }
      renderedPage = page;
      renderedStart = finder.matchedRow;
      renderedEnd = finder.matchedEnd;
    }
    uiText(
      status,
      finder.presentation ||
        pager.error ||
        (pager.busy
          ? "正在读取历史…"
          : !page?.totalRows
            ? "尚无保留历史；输入文字仍可查找主屏。"
            : searchPage
              ? message(
                  "查找片段：第 {0}–{1} 行（含主屏）；点“最新历史”返回翻页。",
                  page.start + 1,
                  page.end,
                )
              : message(
                  "第 {0}–{1} 行，共保留 {2} 行{3}",
                  page.start + 1,
                  page.end,
                  page.totalRows,
                  page.truncated ? message("；更早内容已按容量上限清理") : "",
                )),
    );
  };
  const pager = new HistoryPager(
    (before, revision) =>
      invoke("workspace_terminal_history_page", {
        nonce: options.nonce,
        session: options.session,
        before: before ?? null,
        revision: revision ?? null,
      }),
    current,
    render,
  );
  const finder = new HistoryFinder(
    (request) =>
      invoke("workspace_terminal_history_search", {
        nonce: options.nonce,
        session: options.session,
        request,
      }),
    current,
    (page) => {
      searchPage = page;
      render();
      Array.from(content.querySelectorAll("mark"))
        .at(-1)
        ?.scrollIntoView({ block: "nearest" });
    },
    render,
  );
  const find = (backwards: boolean) => {
    if (!pager.busy)
      void finder.find(
        query.value,
        matchCase.checked,
        backwards,
        searchScope.value === "alternate",
        regex.checked,
        wholeWord.checked,
      );
  };
  previous.addEventListener("click", () => find(true));
  next.addEventListener("click", () => find(false));
  stop.addEventListener("click", () => finder.stop());
  query.addEventListener("input", () => finder.clear());
  matchCase.addEventListener("change", () => finder.clear());
  regex.addEventListener("change", () => finder.clear());
  wholeWord.addEventListener("change", () => finder.clear());
  searchScope.addEventListener("change", () => {
    uiAttribute(
      query,
      "aria-label",
      searchScope.value === "alternate" ? "查找备用屏" : "查找历史与主屏",
    );
    searchPage = undefined;
    finder.clear();
  });
  const compositionLife = new AbortController();
  const composition = new CompositionGuard(search, compositionLife.signal);
  search.addEventListener("keydown", (event) => {
    if (composition.handles(event)) return;
    if (event.key === "Enter" && event.target === query) {
      event.preventDefault();
      find(event.shiftKey);
    }
  });
  dialog.addEventListener("keydown", (event) => {
    if (composition.handles(event)) return;
    if (
      event.key === "Escape" &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey &&
      !event.shiftKey
    ) {
      event.preventDefault();
      event.stopPropagation();
      dialog.close();
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      query.focus();
      query.select();
    }
  });
  older.addEventListener("click", () => {
    if (searchPage) return;
    finder.clear();
    void pager.older();
  });
  newer.addEventListener("click", () => {
    if (searchPage) return;
    finder.clear();
    void pager.newer();
  });
  latest.addEventListener("click", () => {
    finder.clear();
    void pager.latest().then(() => {
      if (!pager.error && !pager.expired) searchPage = undefined;
      render();
    });
  });
  done.addEventListener("click", () => dialog.close());
  dialog.addEventListener("cancel", (event) => {
    event.preventDefault();
    if (!composition.handles({ key: "Escape" })) dialog.close();
  });
  const abort = () => {
    if (dialog.open) dialog.close();
  };
  dialog.addEventListener(
    "close",
    () => {
      pager.dispose();
      finder.dispose();
      compositionLife.abort();
      options.signal.removeEventListener("abort", abort);
      dialog.remove();
      if (
        !options.signal.aborted &&
        options.current() &&
        options.opener.isConnected
      )
        options.opener.focus();
    },
    { once: true },
  );
  options.signal.addEventListener("abort", abort, { once: true });
  dialog.append(title, description, search, content, status, actions);
  doc.body.append(dialog);
  dialog.showModal();
  query.focus();
  void pager.latest();
}
