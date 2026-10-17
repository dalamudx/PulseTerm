import { uiAttribute, uiText } from "../../workspace-language";
export interface TerminalBellNotice {
  ring(): void;
  dispose(): void;
}

/** One coalesced, local notice. Terminal content never becomes notification text. */
export function createTerminalBellNotice(
  host: HTMLElement,
  current: () => boolean,
  focus: () => void,
): TerminalBellNotice {
  const document = host.ownerDocument;
  const notice = document.createElement("div");
  notice.className = "terminal-bell";
  notice.setAttribute("role", "status");
  notice.setAttribute("aria-live", "polite");
  notice.hidden = true;
  const button = document.createElement("button");
  button.type = "button";
  uiText(button, "终端响铃 · 清除");
  uiAttribute(button, "aria-label", "清除终端响铃提示");
  notice.append(button);
  const hadClass = host.classList.contains("terminal-bell-host");
  host.classList.add("terminal-bell-host");
  host.append(notice);
  let disposed = false;
  const dismiss = () => {
    if (disposed || !current()) return;
    notice.hidden = true;
    if (document.activeElement === button) focus();
  };
  button.addEventListener("click", dismiss);
  return {
    ring() {
      if (!disposed && current()) notice.hidden = false;
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      button.removeEventListener("click", dismiss);
      notice.remove();
      if (!hadClass) host.classList.remove("terminal-bell-host");
    },
  };
}
