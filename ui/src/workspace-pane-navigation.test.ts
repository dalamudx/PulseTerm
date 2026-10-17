import { expect, it } from "vitest";
import { workspacePaneDirection } from "./workspace-pane-navigation";

const key = {
  key: "ArrowRight",
  ctrlKey: true,
  altKey: false,
  shiftKey: true,
  metaKey: false,
  isComposing: false,
  repeat: false,
  defaultPrevented: false,
};
it("explicit CtrlShift directions only", () => {
  for (const [name, direction] of [
    ["ArrowLeft", "left"],
    ["ArrowRight", "right"],
    ["ArrowUp", "up"],
    ["ArrowDown", "down"],
  ])
    expect(workspacePaneDirection({ ...key, key: name ?? "" })).toBe(direction);
  for (const value of ["x", "toString", "constructor", "__proto__"])
    expect(workspacePaneDirection({ ...key, key: value })).toBeUndefined();
});
it("IME/repeated/prevented/meta/alt or missing modifiers do not become pane actions", () => {
  for (const patch of [
    { isComposing: true },
    { repeat: true },
    { defaultPrevented: true },
    { metaKey: true },
    { altKey: true },
    { ctrlKey: false },
    { shiftKey: false },
  ])
    expect(workspacePaneDirection({ ...key, ...patch })).toBeUndefined();
});
