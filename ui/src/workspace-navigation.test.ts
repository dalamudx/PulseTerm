import { expect, it, vi } from "vitest";
import { retireBeforeReload, workspaceReloadKey } from "./workspace-navigation";

it("revokes late-result publication before disposal or navigation and repeated intent is inert", () => {
  const life = new AbortController(),
    seen: string[] = [];
  life.signal.addEventListener("abort", () => seen.push("revoked"));
  const cleanup = vi.fn(() => {
    expect(life.signal.aborted).toBe(true);
    seen.push("disposed");
  });
  const navigate = vi.fn(() => {
    expect(life.signal.aborted).toBe(true);
    seen.push("navigated");
  });
  retireBeforeReload(life, cleanup, navigate);
  retireBeforeReload(life, cleanup, navigate);
  expect(seen).toEqual(["revoked", "disposed", "navigated"]);
  expect(navigate).toHaveBeenCalledOnce();
});
it("uncertain cleanup cannot claim a successful navigation", () => {
  const life = new AbortController(),
    navigate = vi.fn();
  expect(() =>
    retireBeforeReload(
      life,
      () => {
        throw Error("cleanup");
      },
      navigate,
    ),
  ).toThrow("cleanup");
  expect(life.signal.aborted).toBe(true);
  expect(navigate).not.toHaveBeenCalled();
});
it("does not steal shell Ctrl-R or alt/meta keyboard combinations", () => {
  const event = {
    ctrlKey: true,
    shiftKey: true,
    altKey: false,
    metaKey: false,
    key: "R",
  };
  expect(workspaceReloadKey(event)).toBe(true);
  for (const patch of [
    { shiftKey: false },
    { ctrlKey: false },
    { altKey: true },
    { metaKey: true },
    { key: "c" },
  ])
    expect(workspaceReloadKey({ ...event, ...patch })).toBe(false);
});
