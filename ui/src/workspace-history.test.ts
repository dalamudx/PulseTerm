import { expect, it, vi } from "vitest";
import {
  decodeHistoryPage,
  HistoryPager,
  historyViewAvailable,
} from "./workspace-history";

function page(end: number, total = 450, revision = "18446744073709551615") {
  const start = Math.max(0, end - 200);
  return {
    revision,
    totalRows: total,
    coldRows: Math.max(0, total - 64),
    truncated: false,
    start,
    end,
    rows: Array.from({ length: end - start }, (_, i) => ({
      text: `row ${start + i}`,
      wrapped: false,
    })),
  };
}
it("validates bounded text, exact row ranges and decimal revisions", () => {
  const valid = {
    ...page(1, 1),
    rows: [{ text: "中é👩‍💻<script>", wrapped: true }],
  };
  expect(decodeHistoryPage(valid)).toEqual(valid);
  for (const bad of [
    { ...valid, revision: "18446744073709551616" },
    { ...valid, revision: "01" },
    { ...valid, end: 2 },
    { ...valid, totalRows: 100001 },
    { ...valid, coldRows: 2 },
    { ...valid, coldRows: -1 },
    { ...valid, truncated: "true" },
    { ...valid, rows: [{ text: "中".repeat(50000), wrapped: false }] },
    { ...valid, rows: [{ text: "text", wrapped: false, html: true }] },
  ])
    expect(() => decodeHistoryPage(bad)).toThrow();
});
it("paginates both directions and keeps only one page plus cursors", async () => {
  const read = vi.fn(async (before?: number) => page(before ?? 450));
  const pager = new HistoryPager(
    read,
    () => true,
    () => {},
  );
  await pager.latest();
  await pager.older();
  await pager.older();
  expect(pager.page?.start).toBe(0);
  expect(pager.canOlder).toBe(false);
  await pager.newer();
  expect(pager.page?.end).toBe(250);
  await pager.newer();
  expect(pager.page?.end).toBe(450);
  expect(pager.canNewer).toBe(false);
  expect(read.mock.calls[1]?.[0]).toBe(250);
});
it("retains the current page on stale output and requires an explicit fresh read", async () => {
  const read = vi
    .fn()
    .mockResolvedValueOnce(page(450))
    .mockRejectedValueOnce("workspace.history_changed")
    .mockResolvedValueOnce(page(451, 451, "12"));
  const pager = new HistoryPager(
    read,
    () => true,
    () => {},
  );
  await pager.latest();
  const saved = pager.page;
  await pager.older();
  expect(pager.page).toBe(saved);
  expect(pager.expired).toBe(true);
  expect(pager.canOlder).toBe(false);
  expect(read).toHaveBeenCalledTimes(2);
  await pager.latest();
  expect(pager.page?.revision).toBe("12");
  expect(pager.expired).toBe(false);
});
it("discards a late reply after the source changes or the dialog closes", async () => {
  for (const dispose of [false, true]) {
    let deliver!: (value: unknown) => void;
    let current = true;
    const pager = new HistoryPager(
      () =>
        new Promise((resolve) => {
          deliver = resolve;
        }),
      () => current,
      () => {},
    );
    const waiting = pager.latest();
    if (dispose) pager.dispose();
    else current = false;
    deliver(page(450));
    await waiting;
    expect(pager.page).toBeUndefined();
  }
});
it("rejects a mixed revision response rather than replacing the visible page", async () => {
  const read = vi
    .fn()
    .mockResolvedValueOnce(page(450))
    .mockResolvedValueOnce(page(250, 450, "other"));
  const pager = new HistoryPager(
    read,
    () => true,
    () => {},
  );
  await pager.latest();
  const saved = pager.page;
  await pager.older();
  expect(pager.page).toBe(saved);
  expect(pager.expired).toBe(true);
});
it("has one request in flight and does not automatically retry a busy terminal", async () => {
  let reject!: (value: unknown) => void;
  const read = vi.fn(
    () =>
      new Promise((_, no) => {
        reject = no;
      }),
  );
  const pager = new HistoryPager(
    read,
    () => true,
    () => {},
  );
  const waiting = pager.latest();
  await pager.latest();
  expect(read).toHaveBeenCalledTimes(1);
  reject("workspace.history_busy");
  await waiting;
  expect(pager.busy).toBe(false);
  expect(read).toHaveBeenCalledTimes(1);
});

it("accepts cold pages up to 100000 rows and explicit retention truncation", () => {
  const value = { ...page(200, 100000), coldRows: 99936, truncated: true };
  expect(decodeHistoryPage(value)).toEqual(value);
});
it("keeps the visible page if retention metadata changes without a new revision", async () => {
  const read = vi
    .fn()
    .mockResolvedValueOnce(page(450))
    .mockResolvedValueOnce({ ...page(250), truncated: true });
  const pager = new HistoryPager(
    read,
    () => true,
    () => {},
  );
  await pager.latest();
  const saved = pager.page;
  await pager.older();
  expect(pager.page).toBe(saved);
  expect(pager.expired).toBe(true);
});

it("offers readonly history after natural end without enabling other view states", () => {
  expect(historyViewAvailable("ready")).toBe(true);
  expect(historyViewAvailable("ended")).toBe(true);
  for (const state of [undefined, "loading", "resizing", "paused", "failed"])
    expect(historyViewAvailable(state)).toBe(false);
});
