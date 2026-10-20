import { expect, it, vi } from "vitest";
import { closeWorkspaceSession } from "./workspace-close";

it("cancel keeps current view and root native session, never sends close", async () => {
  const close = vi.fn();
  expect(
    await closeWorkspaceSession(
      "1",
      new AbortController().signal,
      () => true,
      async () => false,
      close,
    ),
  ).toBe("cancelled");
  expect(close).not.toHaveBeenCalled();
});
it("disabled confirmation resolves true directly and closes session", async () => {
  const close = vi.fn(async () => {});
  expect(
    await closeWorkspaceSession(
      "1",
      new AbortController().signal,
      () => true,
      async () => true,
      close,
    ),
  ).toBe("closed");
  expect(close).toHaveBeenCalledOnce();
});
it("explicit confirmation closes once, uncertain response is not retried", async () => {
  const close = vi.fn(async () => {});
  expect(
    await closeWorkspaceSession(
      "1",
      new AbortController().signal,
      () => true,
      async () => true,
      close,
    ),
  ).toBe("closed");
  expect(close).toHaveBeenCalledTimes(1);
  const uncertain = vi.fn(async () => {
    throw Error("uncertain");
  });
  await expect(
    closeWorkspaceSession(
      "1",
      new AbortController().signal,
      () => true,
      async () => true,
      uncertain,
    ),
  ).rejects.toThrow("uncertain");
  expect(uncertain).toHaveBeenCalledTimes(1);
});
it("page exit or changed target while confirmation waits cannot close replacement", async () => {
  for (const cause of ["abort", "target"]) {
    const controller = new AbortController();
    let current = true,
      accept!: (v: boolean) => void;
    const close = vi.fn(),
      job = closeWorkspaceSession(
        "1",
        controller.signal,
        () => current,
        () =>
          new Promise((resolve) => {
            accept = resolve;
          }),
        close,
      );
    if (cause === "abort") controller.abort();
    else current = false;
    accept(true);
    expect(await job).toBe("cancelled");
    expect(close).not.toHaveBeenCalled();
  }
});
it("invalid id and pre-retired scope never display confirmation or send close", async () => {
  const confirm = vi.fn(async () => true),
    close = vi.fn(),
    controller = new AbortController();
  await expect(
    closeWorkspaceSession("01", controller.signal, () => true, confirm, close),
  ).rejects.toThrow();
  controller.abort();
  expect(
    await closeWorkspaceSession(
      "1",
      controller.signal,
      () => true,
      confirm,
      close,
    ),
  ).toBe("cancelled");
  expect(confirm).not.toHaveBeenCalled();
  expect(close).not.toHaveBeenCalled();
});
