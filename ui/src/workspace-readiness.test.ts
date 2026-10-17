import { describe, expect, it, vi } from "vitest";
import {
  confirmWorkspaceDocument,
  reportWorkspacePresentation,
} from "./workspace-readiness";

const valid = "eb8c9c2a-5ca5-42a7-91f3-2950e523719b";
describe("workspace document receipt", () => {
  it("rejects missing or non-v4 nonce without invoking native", async () => {
    const send = vi.fn(async () => undefined);
    expect(
      await confirmWorkspaceDocument("", send, new AbortController().signal),
    ).toBe("denied");
    expect(
      await confirmWorkspaceDocument(
        "00000000-0000-0000-0000-000000000000",
        send,
        new AbortController().signal,
      ),
    ).toBe("denied");
    expect(send).not.toHaveBeenCalled();
  });
  it("is only a display receipt and ignores late results after pagehide", async () => {
    const controller = new AbortController();
    let finish: (() => void) | undefined;
    const result = confirmWorkspaceDocument(
      valid,
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
      controller.signal,
    );
    controller.abort();
    finish?.();
    expect(await result).toBe("cancelled");
  });
  it("reports backend denial without reflecting error contents", async () => {
    expect(
      await confirmWorkspaceDocument(
        valid,
        async () => {
          throw new Error("secret");
        },
        new AbortController().signal,
      ),
    ).toBe("denied");
    expect(
      await confirmWorkspaceDocument(
        valid,
        async () => undefined,
        new AbortController().signal,
      ),
    ).toBe("ready");
  });
});

describe("workspace original-page presentation metadata", () => {
  it("reports once after initialization without granting action authority", async () => {
    const send = vi.fn(async () => null);
    expect(
      await reportWorkspacePresentation(
        valid,
        send,
        new AbortController().signal,
        () => valid,
      ),
    ).toBe("ready");
    expect(send).toHaveBeenCalledExactlyOnceWith(valid);
  });
  it("rejects invalid nonce without native IPC", async () => {
    const send = vi.fn(async () => null);
    expect(
      await reportWorkspacePresentation(
        "invalid",
        send,
        new AbortController().signal,
        () => "invalid",
      ),
    ).toBe("denied");
    expect(send).not.toHaveBeenCalled();
  });
  it("does not send from a retired or aborted initial page", async () => {
    const send = vi.fn(async () => null);
    const life = new AbortController();
    expect(
      await reportWorkspacePresentation(
        valid,
        send,
        life.signal,
        () => undefined,
      ),
    ).toBe("cancelled");
    life.abort();
    expect(
      await reportWorkspacePresentation(valid, send, life.signal, () => valid),
    ).toBe("cancelled");
    expect(send).not.toHaveBeenCalled();
  });
  it.each(["refresh", "pagehide"])(
    "ignores a late receipt after %s",
    async (mode) => {
      const life = new AbortController();
      let current: string | undefined = valid;
      let finish: (() => void) | undefined;
      const result = reportWorkspacePresentation(
        valid,
        () =>
          new Promise<void>((resolve) => {
            finish = resolve;
          }),
        life.signal,
        () => current,
      );
      if (mode === "pagehide") life.abort();
      else current = undefined;
      finish?.();
      expect(await result).toBe("cancelled");
    },
  );
  it("does not reflect a backend denial or retry it", async () => {
    const send = vi.fn(async () => {
      throw new Error("private");
    });
    expect(
      await reportWorkspacePresentation(
        valid,
        send,
        new AbortController().signal,
        () => valid,
      ),
    ).toBe("denied");
    expect(send).toHaveBeenCalledTimes(1);
  });
});
