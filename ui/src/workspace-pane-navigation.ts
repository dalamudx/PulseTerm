import type { PaneDirection } from "./workspace-layout";
export function workspacePaneDirection(
  event: Pick<
    KeyboardEvent,
    | "key"
    | "ctrlKey"
    | "altKey"
    | "metaKey"
    | "shiftKey"
    | "isComposing"
    | "repeat"
    | "defaultPrevented"
  >,
): PaneDirection | undefined {
  if (
    event.isComposing ||
    event.repeat ||
    event.defaultPrevented ||
    !event.ctrlKey ||
    !event.shiftKey ||
    event.metaKey ||
    event.altKey
  )
    return undefined;
  switch (event.key) {
    case "ArrowLeft":
      return "left";
    case "ArrowRight":
      return "right";
    case "ArrowUp":
      return "up";
    case "ArrowDown":
      return "down";
    default:
      return undefined;
  }
}
