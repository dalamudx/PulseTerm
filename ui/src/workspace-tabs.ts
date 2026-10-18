import {
  message,
  type UiMessage,
  uiAttribute,
  uiText,
} from "./workspace-language";
import { decodeWorkspaceSessions } from "./workspace-sessions";
import { type TabLabel, tabLabel } from "./workspace-tab-labels";

const SELECTED = "pulseterm.workspace.selected.v1";
export function preferredWorkspaceSession(
  ids: readonly string[],
  storage?: Pick<Storage, "getItem">,
): string | undefined {
  const current = decodeWorkspaceSessions(ids);
  try {
    const selected = storage?.getItem(SELECTED);
    if (selected && current.includes(selected)) return selected;
  } catch {
    /* Display preference is optional, never authority. */
  }
  return current[0];
}
export function rememberWorkspaceSession(
  id: string,
  storage?: Pick<Storage, "setItem">,
): void {
  decodeWorkspaceSessions([id]);
  try {
    storage?.setItem(SELECTED, id);
  } catch {
    /* No startup or retry on storage failure. */
  }
}
/** Manual activation: arrows move focus, only an explicit click/Enter changes PTY view. */
export class WorkspaceTabs {
  private ids: readonly string[] = [];
  private active: string | undefined;
  private disabled = false;
  private revision = 0;
  constructor(
    private readonly element: HTMLElement,
    private readonly panel: HTMLElement,
    private readonly select: (id: string) => void,
  ) {}
  render(
    ids: readonly string[],
    active: string | undefined,
    disabled: boolean,
    labels: ReadonlyMap<string, TabLabel> = new Map(),
  ): void {
    this.ids = decodeWorkspaceSessions(ids);
    this.active = this.ids.includes(active ?? "") ? active : undefined;
    this.disabled = disabled;
    const revision = ++this.revision;
    const doc = this.element.ownerDocument;
    const candidate = this.element.contains(doc.activeElement)
      ? (doc.activeElement as HTMLElement).dataset.session
      : undefined;
    const focused = this.ids.includes(candidate ?? "") ? candidate : undefined;
    this.element.replaceChildren();
    this.element.setAttribute("aria-busy", String(disabled));
    for (const id of this.ids) {
      const button = doc.createElement("button");
      button.type = "button";
      button.className = "workspace-tab";
      button.id = `workspace-tab-${id}`;
      button.dataset.session = id;
      const value = labels.get(id);
      let title: string | UiMessage = message("会话 {0}", id);
      if (value) {
        const checked = tabLabel(value.title, value.color);
        title = checked.title || title;
        button.dataset.color = checked.color;
      }
      if (typeof title === "string") button.textContent = title;
      else uiText(button, title);
      uiAttribute(button, "aria-label", message("{0} · 会话 {1}", title, id));
      button.setAttribute("role", "tab");
      button.setAttribute("aria-controls", this.panel.id);
      button.setAttribute("aria-selected", String(id === this.active));
      button.disabled = disabled;
      button.tabIndex = id === (focused ?? this.active ?? this.ids[0]) ? 0 : -1;
      button.addEventListener("click", () => {
        if (
          revision === this.revision &&
          !this.disabled &&
          this.ids.includes(id)
        )
          this.select(id);
      });
      button.addEventListener("keydown", (event) => {
        if (revision === this.revision) this.key(event, id);
      });
      this.element.append(button);
    }
    this.panel.setAttribute(
      "aria-labelledby",
      this.active ? `workspace-tab-${this.active}` : "workspace-title",
    );
    const reveal = focused ?? this.active;
    if (reveal)
      this.element
        .querySelector<HTMLElement>(`#workspace-tab-${reveal}`)
        ?.scrollIntoView({ block: "nearest", inline: "nearest" });
    if (focused && !disabled)
      this.element
        .querySelector<HTMLButtonElement>(`#workspace-tab-${focused}`)
        ?.focus();
  }
  private key(event: KeyboardEvent, id: string): void {
    if (this.disabled || event.ctrlKey || event.metaKey || event.altKey) return;
    const at = this.ids.indexOf(id);
    if (at < 0) return;
    let next: number;
    switch (event.key) {
      case "ArrowRight":
        next = (at + 1) % this.ids.length;
        break;
      case "ArrowLeft":
        next = (at + this.ids.length - 1) % this.ids.length;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = this.ids.length - 1;
        break;
      default:
        return;
    }
    const target = this.ids[next];
    if (!target) return;
    event.preventDefault();
    for (const button of this.element.querySelectorAll<HTMLButtonElement>(
      "[role=tab]",
    ))
      button.tabIndex = button.dataset.session === target ? 0 : -1;
    this.element
      .querySelector<HTMLButtonElement>(`#workspace-tab-${target}`)
      ?.focus();
  }
}
