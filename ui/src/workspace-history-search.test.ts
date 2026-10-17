import { expect, it, vi } from "vitest";
import { HistoryFinder } from "./workspace-history-search";

function result(
  row: number | null,
  cursor: number,
  complete = false,
  revision = "7",
  totalRows = 5000,
) {
  return {
    revision,
    totalRows,
    screenRows: 2,
    alternate: false,
    regex: false,
    wholeWord: false,
    matchedEnd: row === null ? null : row + 1,
    cursor,
    complete,
    matchedRow: row,
    page:
      row === null
        ? null
        : {
            revision,
            totalRows,
            coldRows: totalRows - 4,
            truncated: false,
            start: row,
            end: row + 1,
            rows: [{ text: "<script>中é", wrapped: false }],
          },
  };
}
function setup(
  read: (request: unknown) => Promise<unknown>,
  current = () => true,
) {
  const found = vi.fn();
  const finder = new HistoryFinder(read, current, found, () => {});
  return { finder, found };
}
it("continues bounded scans at the original revision and finds cold Unicode text", async () => {
  const read = vi
    .fn()
    .mockResolvedValueOnce(result(null, 2048))
    .mockResolvedValueOnce(result(3000, 3001));
  const { finder, found } = setup(read);
  await finder.find("中é", false, false);
  expect(read.mock.calls[1]?.[0]).toMatchObject({
    cursor: 2048,
    revision: "7",
    query: "中é",
  });
  expect(found).toHaveBeenCalledTimes(1);
  expect(finder.matchedRow).toBe(3000);
  read.mockResolvedValueOnce(result(3010, 3010));
  await finder.find("中é", false, true);
  expect(read.mock.calls[2]?.[0]).toMatchObject({
    cursor: 3000,
    revision: "7",
    backwards: true,
  });
  expect(finder.expired).toBe(true);
});
it("navigates previous/next matching rows and reaches an edge without wrapping", async () => {
  const read = vi
    .fn()
    .mockResolvedValueOnce(result(2, 3, false, "7", 10))
    .mockResolvedValueOnce(result(1, 1, false, "7", 10))
    .mockResolvedValueOnce(result(null, 0, true, "7", 10));
  const { finder } = setup(read);
  await finder.find("text", true, false);
  await finder.find("text", true, true);
  await finder.find("text", true, true);
  expect(read.mock.calls[1]?.[0]).toMatchObject({
    cursor: 2,
    revision: "7",
    matchCase: true,
  });
  expect(finder.matchedRow).toBe(1);
  expect(finder.message).toContain("没有更多");
});
it("retains the visible match on stale or busy results without retrying", async () => {
  for (const error of ["workspace.history_changed", "workspace.history_busy"]) {
    const read = vi
      .fn()
      .mockResolvedValueOnce(result(2, 3, false, "7", 10))
      .mockRejectedValueOnce(error);
    const { finder, found } = setup(read);
    await finder.find("text", false, false);
    await finder.find("text", false, false);
    expect(read).toHaveBeenCalledTimes(2);
    expect(found).toHaveBeenCalledTimes(1);
    expect(finder.matchedRow).toBe(2);
    expect(finder.expired).toBe(true);
  }
});
it("stops on invalid progress, mixed revisions, malformed pages and unknown fields", async () => {
  for (const bad of [
    result(null, 0),
    result(null, 4097),
    { ...result(3, 4), extra: true },
    { ...result(3, 4), page: { html: true } },
    result(null, 2048, false, "8"),
  ]) {
    const read = vi
      .fn()
      .mockResolvedValueOnce(result(null, 2048))
      .mockResolvedValueOnce(bad);
    const { finder, found } = setup(read);
    await finder.find("text", false, false);
    expect(finder.expired).toBe(true);
    expect(found).not.toHaveBeenCalled();
    expect(read.mock.calls.length).toBeLessThanOrEqual(2);
  }
});
it("discards delayed results after close, query edits or page revocation", async () => {
  for (const action of ["close", "edit", "page"]) {
    let resolve!: (value: unknown) => void;
    let current = true;
    const read = vi.fn(
      () =>
        new Promise((r) => {
          resolve = r;
        }),
    );
    const { finder, found } = setup(read, () => current);
    const running = finder.find("text", false, false);
    if (action === "close") finder.dispose();
    else if (action === "edit") finder.clear();
    else current = false;
    resolve(result(0, 1, false, "7", 10));
    await running;
    expect(found).not.toHaveBeenCalled();
    expect(read).toHaveBeenCalledTimes(1);
  }
});
it("rejects empty/control/oversized input before IPC and has one scan in flight", async () => {
  const read = vi.fn();
  const { finder } = setup(read);
  for (const text of ["", "line\n", "中".repeat(200)])
    await finder.find(text, false, false);
  expect(read).not.toHaveBeenCalled();
  let finish!: (value: unknown) => void;
  read.mockImplementation(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  const run = finder.find("text", false, false);
  await finder.find("again", false, false);
  expect(read).toHaveBeenCalledTimes(1);
  finish(result(null, 2, true, "7", 2));
  await run;
  expect(finder.message).toContain("没有匹配");
});

it("uses a wrapped match end for next navigation and names primary-screen rows", async () => {
  const wrapped = {
    ...result(8, 10, true, "7", 10),
    matchedEnd: 10,
    page: {
      ...result(8, 10, true, "7", 10).page,
      start: 8,
      end: 10,
      rows: [
        { text: "AB", wrapped: true },
        { text: "CD", wrapped: false },
      ],
    },
  };
  const read = vi
    .fn()
    .mockResolvedValueOnce(wrapped)
    .mockResolvedValueOnce(result(null, 10, true, "7", 10));
  const { finder, found } = setup(read);
  await finder.find("BC", true, false);
  expect(found).toHaveBeenCalledTimes(1);
  expect(finder.matchedEnd).toBe(10);
  expect(finder.message).toContain("主屏第 1 行至主屏第 2 行");
  await finder.find("BC", true, false);
  expect(read.mock.calls[1]?.[0]).toMatchObject({ cursor: 10, revision: "7" });
});

it("rejects invalid screen ranges but accepts bounded context for a long match", async () => {
  for (const bad of [
    { ...result(2, 3), screenRows: 257 },
    { ...result(2, 3), matchedEnd: 2 },
    { ...result(2, 3), matchedEnd: null },
    { ...result(2, 3), screenRows: 5001 },
  ]) {
    const { finder, found } = setup(vi.fn().mockResolvedValueOnce(bad));
    await finder.find("text", false, false);
    expect(finder.expired).toBe(true);
    expect(found).not.toHaveBeenCalled();
  }
  const long = {
    ...result(0, 300, false, "7", 500),
    matchedEnd: 300,
    page: {
      ...result(0, 300, false, "7", 500).page,
      start: 100,
      end: 300,
      rows: Array.from({ length: 200 }, () => ({ text: "a", wrapped: true })),
    },
  };
  const { finder, found } = setup(vi.fn().mockResolvedValueOnce(long));
  await finder.find("a".repeat(300), true, false);
  expect(found).toHaveBeenCalledTimes(1);
  expect(finder.message).toContain("仅显示末尾");
});

it("keeps alternate navigation independent and rejects mixed scope or history rows", async () => {
  const alternate = { ...result(0, 1, false, "7", 2), alternate: true };
  if (alternate.page) alternate.page.coldRows = 0;
  const read = vi
    .fn()
    .mockResolvedValueOnce(alternate)
    .mockResolvedValueOnce(result(3, 4, false, "7", 10));
  const { finder, found } = setup(read);
  await finder.find("text", false, false, true);
  expect(read.mock.calls[0]?.[0]).toMatchObject({
    alternate: true,
    cursor: null,
    revision: null,
  });
  expect(finder.message).toContain("备用屏第 1 行");
  await finder.find("text", false, false, false);
  expect(read.mock.calls[1]?.[0]).toMatchObject({
    alternate: false,
    regex: false,
    cursor: null,
    revision: null,
  });
  expect(found).toHaveBeenCalledTimes(2);
  for (const bad of [
    result(0, 1, false, "7", 2),
    { ...alternate, totalRows: 3 },
    { ...alternate, alternate: "true" },
    { ...alternate, page: { ...alternate.page, coldRows: 1 } },
  ]) {
    const check = setup(vi.fn().mockResolvedValueOnce(bad));
    await check.finder.find("text", false, false, true);
    expect(check.finder.expired).toBe(true);
    expect(check.found).not.toHaveBeenCalled();
  }
});
it("explains inactive alternate screens and discards a scope reply after clear", async () => {
  const { finder } = setup(
    vi.fn().mockRejectedValueOnce("workspace.history_unavailable"),
  );
  await finder.find("text", false, false, true);
  expect(finder.message).toContain("备用屏未启用");
  let deliver!: (value: unknown) => void;
  const late = setup(
    () =>
      new Promise((resolve) => {
        deliver = resolve;
      }),
  );
  const run = late.finder.find("text", false, false, true);
  late.finder.clear();
  deliver({ ...result(null, 2, true, "7", 2), alternate: true });
  await run;
  expect(late.found).not.toHaveBeenCalled();
  expect(late.finder.busy).toBe(false);
  expect(late.finder.message).toBe("");
});

it("resets cursors on regex mode changes and rejects mismatched replies", async () => {
  const read = vi
    .fn()
    .mockResolvedValueOnce(result(0, 1, false, "7", 10))
    .mockResolvedValueOnce({ ...result(2, 3, false, "7", 10), regex: true });
  const { finder, found } = setup(read);
  await finder.find("text", false, false);
  await finder.find("text", false, false, false, true);
  expect(read.mock.calls[1]?.[0]).toMatchObject({
    regex: true,
    cursor: null,
    revision: null,
  });
  expect(found).toHaveBeenCalledTimes(2);
  for (const bad of [result(0, 1), { ...result(0, 1), regex: "true" }]) {
    const check = setup(vi.fn().mockResolvedValueOnce(bad));
    await check.finder.find("^text$", true, false, false, true);
    expect(check.found).not.toHaveBeenCalled();
    expect(check.finder.expired).toBe(true);
  }
});
it("preserves content and explains invalid expressions and exhausted budgets without retry", async () => {
  for (const [error, message] of [
    ["workspace.history_pattern", "正则表达式无效"],
    ["workspace.history_too_large", "超过单次查找预算"],
  ]) {
    const read = vi
      .fn()
      .mockResolvedValueOnce(result(2, 3, false, "7", 10))
      .mockRejectedValueOnce(error);
    const { finder, found } = setup(read);
    await finder.find("text", true, false);
    await finder.find("[", true, false, false, true);
    expect(read).toHaveBeenCalledTimes(2);
    expect(found).toHaveBeenCalledTimes(1);
    expect(finder.matchedRow).toBe(2);
    expect(finder.message).toContain(message);
  }
});

it("resets cursors when whole-word mode changes and keeps navigation scoped", async () => {
  const read = vi
    .fn()
    .mockResolvedValueOnce(result(0, 1, false, "7", 10))
    .mockResolvedValueOnce({ ...result(2, 3, false, "7", 10), wholeWord: true })
    .mockResolvedValueOnce({ ...result(1, 1, false, "7", 10), wholeWord: true })
    .mockResolvedValueOnce(result(0, 1, false, "7", 10));
  const { finder, found } = setup(read);
  await finder.find("cat", false, false);
  await finder.find("cat", false, false, false, false, true);
  expect(read.mock.calls[1]?.[0]).toMatchObject({
    wholeWord: true,
    cursor: null,
    revision: null,
  });
  await finder.find("cat", false, true, false, false, true);
  expect(read.mock.calls[2]?.[0]).toMatchObject({
    wholeWord: true,
    cursor: 2,
    revision: "7",
  });
  await finder.find("cat", false, false);
  expect(read.mock.calls[3]?.[0]).toMatchObject({
    wholeWord: false,
    cursor: null,
    revision: null,
  });
  expect(found).toHaveBeenCalledTimes(4);
});
it("rejects missing, mistyped or mismatched whole-word modes", async () => {
  for (const wholeWord of [undefined, null, "true", 1, false]) {
    const { finder, found } = setup(
      vi.fn().mockResolvedValueOnce({ ...result(0, 1), wholeWord }),
    );
    await finder.find("cat", true, false, false, false, true);
    expect(found).not.toHaveBeenCalled();
    expect(finder.expired).toBe(true);
  }
});
it("drops an in-flight whole-word reply when controls invalidate it", async () => {
  let deliver!: (value: unknown) => void;
  const { finder, found } = setup(
    () =>
      new Promise((resolve) => {
        deliver = resolve;
      }),
  );
  const run = finder.find("cat", false, false, true, true, true);
  finder.clear();
  deliver({
    ...result(0, 1, false, "7", 2),
    alternate: true,
    regex: true,
    wholeWord: true,
  });
  await run;
  expect(found).not.toHaveBeenCalled();
  expect(finder.message).toBe("");
});
