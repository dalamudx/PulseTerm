import {
  type UiMessage,
  uiAttribute,
  message as uiMessage,
  uiText,
} from "./workspace-language";
import type { DefaultProfileId } from "./workspace-profile-default";
import {
  DefaultProfileEditor,
  type DefaultProfileNative,
  type DefaultProfileOffer,
} from "./workspace-profile-default";

const label = (kind: DefaultProfileId, offer?: DefaultProfileOffer) =>
  offer?.names?.[kind] ?? (kind === "sh" ? "POSIX sh" : kind);
export class DefaultProfileForm {
  private editor: DefaultProfileEditor | undefined;
  private version = 0;
  private renderVersion = 0;
  private busy = false;
  private nonce: string | undefined;
  private expiry: ReturnType<typeof setTimeout> | undefined;
  constructor(
    private readonly options: {
      root: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      native: DefaultProfileNative;
      working(value: boolean): void;
      launch(nonce: string, kind: DefaultProfileId): void;
    },
  ) {
    options.signal.addEventListener("abort", this.close, { once: true });
  }
  close = (): void => {
    ++this.version;
    clearTimeout(this.expiry);
    this.editor?.close();
    this.editor = undefined;
    this.busy = false;
    this.options.working(false);
    this.options.root.replaceChildren();
  };
  async open(): Promise<void> {
    this.close();
    const nonce = this.options.nonce();
    if (!nonce || this.options.signal.aborted) return;
    this.nonce = nonce;
    const editor = new DefaultProfileEditor(
      nonce,
      this.options.signal,
      this.options.native,
    );
    this.editor = editor;
    const version = this.version;
    this.setBusy(true);
    this.render(null, "正在读取默认 Profile…");
    const offer = await editor.read();
    if (!this.current(version)) return;
    this.setBusy(false);
    this.show(offer);
  }
  private current(version: number): boolean {
    return (
      version === this.version &&
      !this.options.signal.aborted &&
      this.nonce === this.options.nonce()
    );
  }
  private setBusy(value: boolean): void {
    this.busy = value;
    this.options.working(value);
  }
  private show(
    offer: DefaultProfileOffer | null,
    status?: string | UiMessage,
  ): void {
    clearTimeout(this.expiry);
    this.render(
      offer,
      status ??
        (!offer
          ? "无法读取默认 Profile，请重新读取。"
          : !offer.selected
            ? "尚未设置默认 Profile。"
            : !offer.available
              ? "已保存的默认 Profile 不可用，请恢复显示或重新选择。"
              : uiMessage("当前默认：{0}", label(offer.selected, offer))),
    );
    if (offer) {
      const version = this.version;
      this.expiry = setTimeout(() => {
        if (this.current(version) && !this.busy)
          this.render(null, "读取已过期，请重新读取。");
      }, 55000);
    }
  }
  private render(
    offer: DefaultProfileOffer | null,
    message: string | UiMessage,
  ): void {
    const root = this.options.root,
      document = root.ownerDocument,
      version = this.version;
    const rendered = ++this.renderVersion;
    const current = () =>
      this.current(version) && rendered === this.renderVersion;
    const row = document.createElement("label"),
      text = document.createElement("span"),
      select = document.createElement("select");
    uiText(text, "默认 Profile");
    uiAttribute(select, "aria-label", "默认 Profile");
    const add = (
      value: string,
      title: string | UiMessage,
      disabled = false,
    ) => {
      const option = document.createElement("option");
      option.value = value;
      uiText(option, title);
      option.disabled = disabled;
      select.append(option);
    };
    add("", "未设置");
    if (offer?.selected && !offer.available)
      add(
        offer.selected,
        uiMessage("{0}（不可用）", label(offer.selected, offer)),
        true,
      );
    for (const kind of offer?.offered ?? [])
      add(kind, uiMessage("{0}", label(kind, offer ?? undefined)));
    select.value = offer?.selected ?? "";
    select.disabled = this.busy || !offer;
    row.append(text, select);
    const actions = document.createElement("div");
    actions.className = "terminal-paste-actions";
    const save = document.createElement("button"),
      start = document.createElement("button"),
      reload = document.createElement("button");
    save.type = start.type = reload.type = "button";
    uiText(save, "保存默认选择");
    uiText(start, "启动默认 Profile");
    uiText(reload, "重新读取");
    const update = () => {
      save.disabled =
        this.busy ||
        !offer ||
        select.value === (offer.selected ?? "") ||
        (select.value !== "" &&
          !offer.offered.includes(select.value as DefaultProfileId));
      start.disabled =
        this.busy || !offer?.available || select.value !== offer.selected;
      reload.disabled = this.busy;
    };
    update();
    select.addEventListener("change", update, { signal: this.options.signal });
    save.addEventListener(
      "click",
      () => {
        if (!current() || save.disabled) return;
        void this.save(
          select.value === "" ? null : (select.value as DefaultProfileId),
        );
      },
      { signal: this.options.signal },
    );
    start.addEventListener(
      "click",
      () => {
        if (current() && !start.disabled && offer?.selected)
          void this.start(offer.selected);
      },
      { signal: this.options.signal },
    );
    reload.addEventListener(
      "click",
      () => {
        if (current() && !reload.disabled) void this.open();
      },
      { signal: this.options.signal },
    );
    actions.append(save, start, reload);
    const status = document.createElement("p");
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    uiText(status, message);
    root.replaceChildren(row, actions, status);
  }
  private async save(selected: DefaultProfileId | null): Promise<void> {
    if (this.busy || !this.editor) return;
    const version = this.version,
      editor = this.editor;
    this.setBusy(true);
    this.render(editor.offer, "正在保存默认选择…");
    const result = await editor.save(selected);
    if (!this.current(version)) return;
    if (result !== "saved") {
      this.setBusy(false);
      this.show(null, "保存结果未确认，请重新读取检查。");
      return;
    }
    const offer = await editor.read();
    if (!this.current(version)) return;
    this.setBusy(false);
    this.show(
      offer,
      offer
        ? "默认选择已保存"
        : "保存已完成，但无法读取当前默认项，请重新读取。",
    );
  }
  private async start(expected: DefaultProfileId): Promise<void> {
    if (this.busy || !this.editor || !this.nonce) return;
    const version = this.version,
      nonce = this.nonce;
    this.setBusy(true);
    this.render(this.editor.offer, "正在确认默认 Profile…");
    const fresh = await this.editor.read();
    if (!this.current(version)) return;
    this.setBusy(false);
    if (!fresh?.available || fresh.selected !== expected) {
      this.show(fresh, "默认 Profile 已变化或不可用，请重新确认。");
      return;
    }
    this.close();
    this.options.launch(nonce, expected);
  }
}
