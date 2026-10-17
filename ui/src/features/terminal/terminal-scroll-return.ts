import { uiText } from "../../workspace-language";
export function createScrollReturn(
  host: HTMLElement,
  current: () => boolean,
  buffer: () => { readonly viewportY: number; readonly baseY: number },
  scrollToBottom: () => void,
  focus: () => void,
) {
  const document = host.ownerDocument;
  const button = document.createElement("button");
  button.type = "button";
  button.className = "terminal-scroll-return";
  uiText(button, "回到最新输出");
  button.hidden = true;
  const hadClass = host.classList.contains("terminal-scroll-host");
  host.classList.add("terminal-scroll-host");
  host.append(button);
  let disposed = false;
  const refresh = () => {
    if (disposed || !current()) return;
    const { viewportY, baseY } = buffer();
    const hidden = viewportY >= baseY;
    if (button.hidden !== hidden) button.hidden = hidden;
  };
  const click = () => {
    if (disposed || !current()) return;
    const restoreFocus = document.activeElement === button;
    scrollToBottom();
    refresh();
    if (restoreFocus && current()) focus();
  };
  button.addEventListener("click", click);
  return {
    refresh,
    dispose() {
      if (disposed) return;
      disposed = true;
      button.removeEventListener("click", click);
      button.remove();
      if (!hadClass) host.classList.remove("terminal-scroll-host");
    },
  };
}
