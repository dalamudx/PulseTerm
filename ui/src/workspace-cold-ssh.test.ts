import { expect, it, vi } from "vitest";
import { confirmColdSsh } from "./workspace-cold-ssh";
import { workspaceDecision } from "./workspace-decision";
import { message, renderUiMessage } from "./workspace-language";

vi.mock("./workspace-decision", () => ({ workspaceDecision: vi.fn() }));
const selection = {
  hostId: "018f5691-0059-7a69-a45a-7c4d6a1cfa51",
  identityId: "018f5691-0059-7a69-a45a-7c4d6a1cfa52",
};
const document = {} as Document;
it("unlock completion waits for a distinct target-view decision", async () => {
  const signal = new AbortController().signal;
  let accept: (value: boolean) => void = () => {};
  vi.mocked(workspaceDecision)
    .mockReset()
    .mockResolvedValueOnce(true)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          accept = resolve;
        }),
    );
  const read = vi
    .fn()
    .mockResolvedValueOnce({ version: 1, state: "locked" })
    .mockResolvedValueOnce({ version: 1, state: "available" });
  const unlock = vi.fn(async () => {});
  let done = false;
  const job = confirmColdSsh(document, selection, signal, {
    read,
    unlock,
  }).then((value) => {
    done = true;
    return value;
  });
  await vi.waitFor(() => expect(workspaceDecision).toHaveBeenCalledTimes(2));
  expect(done).toBe(false);
  expect(unlock).toHaveBeenCalledExactlyOnceWith(signal);
  expect(vi.mocked(workspaceDecision).mock.calls[1]?.[1]?.accept).toBe(
    "查看目标",
  );
  accept(false);
  expect(await job).toBe(false);
});
it("already unlocked still needs an explicit decision and never opens Vault", async () => {
  vi.mocked(workspaceDecision).mockReset().mockResolvedValueOnce(true);
  const signal = new AbortController().signal;
  const unlock = vi.fn();
  expect(
    await confirmColdSsh(document, selection, signal, {
      read: vi.fn().mockResolvedValue({ version: 1, state: "available" }),
      unlock,
    }),
  ).toBe(true);
  expect(workspaceDecision).toHaveBeenCalledTimes(1);
  expect(unlock).not.toHaveBeenCalled();
});
it("cancel, locked close and revocation during unlock never approve", async () => {
  for (const mode of ["cancel", "locked", "abort"]) {
    const life = new AbortController();
    vi.mocked(workspaceDecision)
      .mockReset()
      .mockResolvedValueOnce(mode !== "cancel");
    const unlock = vi.fn(async () => {
      if (mode === "abort") life.abort();
    });
    expect(
      await confirmColdSsh(document, selection, life.signal, {
        read: vi.fn().mockResolvedValue({ version: 1, state: "locked" }),
        unlock,
      }),
    ).toBe(false);
    expect(workspaceDecision).toHaveBeenCalledTimes(1);
    expect(unlock).toHaveBeenCalledTimes(mode === "cancel" ? 0 : 1);
  }
});

it("saved alias shows exact entry metadata and still requires an explicit decision", async () => {
  vi.mocked(workspaceDecision).mockReset().mockResolvedValueOnce(false);
  const unlock = vi.fn();
  expect(
    await confirmColdSsh(
      document,
      { alias: "开发入口 Alpha" },
      new AbortController().signal,
      {
        read: vi.fn().mockResolvedValue({ version: 1, state: "available" }),
        unlock,
      },
    ),
  ).toBe(false);
  const value = vi.mocked(workspaceDecision).mock.calls[0]?.[1].preview ?? "";
  const preview = typeof value === "string" ? message(value) : value;
  expect(renderUiMessage("en", preview)).toBe(
    "Saved SSH entry: 开发入口 Alpha",
  );
  expect(renderUiMessage("zh-CN", preview)).toBe(
    "保存的 SSH 入口：开发入口 Alpha",
  );
  expect(unlock).not.toHaveBeenCalled();
});
