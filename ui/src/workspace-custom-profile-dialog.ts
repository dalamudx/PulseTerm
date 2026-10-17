import {
  type CustomDraft,
  type CustomNative,
  type CustomProfile,
  decodeCustomDraft,
  decodeCustomOffer,
  emptyCustomConfig,
} from "./workspace-custom-profiles";
import { message, translate, uiAttribute, uiText } from "./workspace-language";
import { type ProfileConfig, ProfileEditor } from "./workspace-profile-editor";
import {
  ProfileEditorForm,
  type ProfileEditorPort,
} from "./workspace-profile-editor-form";
import { isWorkspaceNonce } from "./workspace-readiness";

type Mode = "create" | "copy" | "update" | "delete" | "restore";
export class WorkspaceCustomProfileDialog {
  private directoryRefresh: Promise<void> = Promise.resolve();
  private version = 0;
  private life = new AbortController();
  private session: ProfileEditorPort | undefined;
  private form: ProfileEditorForm | undefined;
  private metadata: CustomDraft | undefined;
  private fields:
    | {
        name: HTMLInputElement;
        executable: HTMLInputElement;
        visible: HTMLInputElement;
      }
    | undefined;
  private engaged = false;
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      list: HTMLElement;
      editor: HTMLElement;
      status: HTMLElement;
      done: HTMLButtonElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      native: CustomNative;
      working(value: boolean): void;
      changed(): void | Promise<void>;
      start(id: string): void;
      returned(): void;
    },
  ) {
    uiText(options.dialog.querySelector("h2") as HTMLElement, "自定义 Profile");
    uiText(options.done, "返回 Profile 管理");
    options.done.addEventListener("click", () => this.close(), {
      signal: options.signal,
    });
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      { signal: options.signal },
    );
    options.signal.addEventListener("abort", () => this.close(false), {
      once: true,
    });
  }
  open(): void {
    if (this.options.signal.aborted || this.options.dialog.open) return;
    this.life = new AbortController();
    this.options.dialog.showModal();
    this.form = new ProfileEditorForm({
      root: this.options.editor,
      signal: this.life.signal,
      nonce: this.options.nonce,
      editor: () => {
        if (!this.session) throw Error("profile.denied");
        return this.session;
      },
      fields: (form) => this.addFields(form),
      validate: () => {
        this.draft(emptyCustomConfig());
      },
      focus: () => this.fields?.name.focus(),
      validationMessage: message("请检查名称、程序绝对路径、参数和环境变量。"),
      engaged: (value) => this.working(value),
      changed: () => this.changed(),
    });
    void this.refresh();
  }
  close(returned = true): void {
    ++this.version;
    this.session?.close();
    this.session = undefined;
    this.form?.close();
    this.form = undefined;
    this.life.abort();
    this.fields = undefined;
    this.metadata = undefined;
    this.working(false);
    const opened = this.options.dialog.open;
    if (opened) this.options.dialog.close();
    this.options.list.replaceChildren();
    this.options.editor.replaceChildren();
    uiText(this.options.status, "");
    if (opened && returned && !this.options.signal.aborted)
      this.options.returned();
  }
  private working(value: boolean): void {
    this.engaged = value;
    for (const button of this.options.list.querySelectorAll<HTMLButtonElement>(
      "button",
    ))
      button.disabled = value;
    this.options.working(value);
  }
  private current(version: number, nonce: string): boolean {
    return (
      version === this.version &&
      this.options.dialog.open &&
      !this.life.signal.aborted &&
      !this.options.signal.aborted &&
      this.options.nonce() === nonce
    );
  }
  private cancelRaw(nonce: string, raw: unknown): void {
    if (
      !raw ||
      typeof raw !== "object" ||
      !("ticket" in raw) ||
      typeof raw.ticket !== "string" ||
      !isWorkspaceNonce(raw.ticket)
    )
      return;
    try {
      void this.options.native.cancel(nonce, raw.ticket).catch(() => {});
    } catch {}
  }
  private changed(): void {
    const signal = this.life.signal;
    const nonce = this.options.nonce();
    this.directoryRefresh = this.directoryRefresh
      .then(() => {
        if (
          signal.aborted ||
          this.options.signal.aborted ||
          this.options.nonce() !== nonce
        )
          return;
        return this.options.changed();
      })
      .catch(() => {});
    void this.refresh();
  }
  private async source(nonce: string, version: number) {
    let ended = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let abort = () => {};
    const signal = this.life.signal;
    try {
      return await Promise.race([
        this.directoryRefresh
          .then(() => {
            if (ended || !this.current(version, nonce))
              throw Error("profile.cancelled");
            return this.options.native.read(nonce);
          })
          .then((raw) => {
            if (ended || !this.current(version, nonce)) {
              this.cancelRaw(nonce, raw);
              throw Error("profile.cancelled");
            }
            try {
              return decodeCustomOffer(raw);
            } catch (error) {
              this.cancelRaw(nonce, raw);
              throw error;
            }
          }),
        new Promise<never>((_, reject) => {
          abort = () => reject(Error("profile.cancelled"));
          signal.addEventListener("abort", abort, { once: true });
          timer = setTimeout(abort, 5000);
        }),
      ]);
    } finally {
      ended = true;
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
    }
  }
  async refresh(): Promise<void> {
    const nonce = this.options.nonce();
    if (!nonce || !this.options.dialog.open || this.life.signal.aborted) return;
    const version = ++this.version;
    this.session?.close();
    this.session = undefined;
    this.form?.close();
    this.working(true);
    uiText(this.options.status, "正在读取自定义 Profile…");
    try {
      const offer = await this.source(nonce, version);
      this.cancelRaw(nonce, offer);
      if (!this.current(version, nonce)) return;
      this.render(offer.profiles);
      uiText(this.options.status, "");
    } catch {
      if (this.current(version, nonce)) {
        uiText(this.options.status, "自定义 Profile 读取失败，请重新读取。");
        this.options.list.replaceChildren(
          this.button("重新读取", () => {
            void this.refresh();
          }),
        );
      }
    } finally {
      if (this.current(version, nonce)) this.working(false);
    }
  }
  private button(label: string, action: () => void): HTMLButtonElement {
    const button = this.options.list.ownerDocument.createElement("button");
    button.type = "button";
    uiText(button, label);
    button.addEventListener(
      "click",
      () => {
        if (!this.engaged && this.options.dialog.open) action();
      },
      { signal: this.life.signal },
    );
    return button;
  }
  private render(profiles: readonly CustomProfile[]): void {
    const root = this.options.list,
      doc = root.ownerDocument;
    root.replaceChildren(
      this.button("新建 Profile", () => this.begin("create")),
    );
    for (const profile of profiles) {
      const row = doc.createElement("div"),
        name = doc.createElement("span"),
        actions = doc.createElement("div");
      row.className = "workspace-custom-profile-row";
      actions.className = "terminal-paste-actions";
      name.textContent = profile.name;
      if (profile.deleted) {
        const deleted = doc.createElement("span");
        uiText(deleted, "已删除");
        row.append(deleted);
        actions.append(
          this.button("恢复", () => this.begin("restore", profile)),
        );
      } else {
        actions.append(
          this.button("编辑", () => this.begin("update", profile)),
          this.button("复制", () => this.begin("copy", profile)),
          this.button("删除", () => this.begin("delete", profile)),
          this.button("打开终端", () => {
            this.close(false);
            this.options.start(profile.id);
          }),
        );
      }
      row.prepend(name);
      row.append(actions);
      root.append(row);
    }
  }
  private begin(mode: Mode, selected?: CustomProfile): void {
    if (this.engaged) return;
    const nonce = this.options.nonce();
    if (!nonce) return;
    const version = ++this.version;
    this.form?.close();
    this.session?.close();
    this.fields = undefined;
    const delegate = new ProfileEditor(nonce, this.life.signal, {
      read: async (nonce, correlation) => {
        const source = await this.source(nonce, version);
        const profile = selected
          ? source.profiles.find((item) => item.id === selected.id)
          : undefined;
        if (
          selected &&
          (!profile || profile.deleted !== (mode === "restore"))
        ) {
          this.cancelRaw(nonce, source);
          throw Error("profile.conflict");
        }
        const initial = profile ?? {
          name: "",
          executable: "",
          hidden: false,
          config: emptyCustomConfig(),
        };
        this.metadata = {
          ...initial,
          name:
            mode === "copy"
              ? translate(
                  this.options.dialog.ownerDocument,
                  "{0}（副本）",
                  initial.name,
                )
              : initial.name,
        };
        // This correlation key stays inside the bounded editor; native APIs receive the saved UUID.
        return {
          ticket: source.ticket,
          shell: correlation,
          config: initial.config,
        };
      },
      save: (nonce, ticket, config) => {
        if (mode === "delete" || mode === "restore") {
          if (!selected) throw Error("profile.invalid");
          return this.options.native.save(nonce, ticket, {
            kind: mode,
            id: selected.id,
          });
        }
        const draft = this.draft(config);
        return this.options.native.save(
          nonce,
          ticket,
          mode === "update" && selected
            ? { kind: "update", id: selected.id, draft }
            : { kind: "create", draft },
        );
      },
      cancel: (nonce, ticket) => this.options.native.cancel(nonce, ticket),
    });
    this.session = {
      read: async (correlation) => {
        const offer = await delegate.read(correlation);
        return offer
          ? {
              config: offer.config,
              title:
                mode === "create"
                  ? message("新建 Profile")
                  : message(
                      mode === "copy" ? "复制 {0}" : "编辑 {0}",
                      this.metadata?.name ?? "",
                    ),
            }
          : null;
      },
      save: (config) => delegate.save(config),
      close: () => delegate.close(),
    };
    if (mode === "delete" || mode === "restore")
      void this.confirm(mode, selected, nonce, version, this.session);
    else void this.form?.open("sh");
  }
  private addFields(form: HTMLFormElement): void {
    const metadata = this.metadata;
    if (!metadata) throw Error("profile.invalid");
    const doc = form.ownerDocument;
    const field = (label: string, value: string, max: number) => {
      const node = doc.createElement("label"),
        input = doc.createElement("input");
      uiText(node, label);
      input.value = value;
      input.maxLength = max;
      input.autocomplete = "off";
      input.spellcheck = false;
      uiAttribute(input, "aria-label", label);
      node.append(input);
      form.append(node);
      return input;
    };
    const name = field("Profile 名称", metadata.name, 80),
      executable = field("程序绝对路径", metadata.executable, 4096);
    const label = doc.createElement("label"),
      visible = doc.createElement("input"),
      note = doc.createElement("p");
    visible.type = "checkbox";
    visible.checked = !metadata.hidden;
    uiAttribute(visible, "aria-label", "显示在侧栏");
    uiText(label, "显示在侧栏");
    label.className = "workspace-profile-toggle";
    label.prepend(visible);
    uiText(note, "程序按字面参数启动，不自动添加登录参数。保存不会启动终端。");
    form.append(label, note);
    this.fields = { name, executable, visible };
  }
  private draft(config: ProfileConfig): CustomDraft {
    if (!this.fields) throw Error("profile.invalid");
    return decodeCustomDraft({
      name: this.fields.name.value,
      executable: this.fields.executable.value,
      hidden: !this.fields.visible.checked,
      config,
    });
  }
  private async confirm(
    mode: "delete" | "restore",
    selected: CustomProfile | undefined,
    nonce: string,
    version: number,
    session: ProfileEditorPort,
  ): Promise<void> {
    this.working(true);
    uiText(this.options.status, "正在读取自定义 Profile…");
    const offer = await session.read("sh");
    if (!this.current(version, nonce)) return;
    if (!offer || !selected) {
      this.working(false);
      uiText(this.options.status, "自定义 Profile 读取失败，请重新读取。");
      return;
    }
    uiText(this.options.status, "");
    const root = this.options.editor,
      doc = root.ownerDocument,
      note = doc.createElement("p");
    uiText(
      note,
      message(
        mode === "delete"
          ? "确认删除 {0}？已有终端保持运行，可稍后恢复。"
          : "确认恢复 {0}？恢复后仍需明确打开终端。",
        this.metadata?.name ?? selected.name,
      ),
    );
    const cancel = doc.createElement("button"),
      commit = doc.createElement("button"),
      actions = doc.createElement("div");
    cancel.type = commit.type = "button";
    actions.className = "terminal-paste-actions";
    uiText(cancel, "取消");
    uiText(commit, mode === "delete" ? "确认删除" : "确认恢复");
    cancel.addEventListener(
      "click",
      () => {
        void this.refresh();
      },
      { signal: this.life.signal },
    );
    commit.addEventListener(
      "click",
      async () => {
        if (!this.current(version, nonce) || commit.disabled) return;
        commit.disabled = true;
        const result = await session.save(offer.config);
        if (!this.current(version, nonce)) return;
        if (result === "saved") {
          this.changed();
        } else {
          uiText(note, "保存结果未确认，请重新读取；不会自动重试。");
        }
      },
      { signal: this.life.signal },
    );
    actions.append(cancel, commit);
    root.replaceChildren(note, actions);
    cancel.focus();
  }
}
