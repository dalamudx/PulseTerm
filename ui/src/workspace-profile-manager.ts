import {
  uiAttribute,
  message as uiMessage,
  uiText,
} from "./workspace-language";
import type {
  DefaultProfileId,
  DefaultProfileNative,
} from "./workspace-profile-default";
import { DefaultProfileForm } from "./workspace-profile-default-form";
import type { ProfileEditorNative } from "./workspace-profile-editor";
import { ProfileEditorForm } from "./workspace-profile-editor-form";
import type { ProfileLifecycleNative } from "./workspace-profile-lifecycle";
import { ProfileLifecycleForm } from "./workspace-profile-lifecycle-form";
import { changeProfileVisibility } from "./workspace-profile-preferences";
import {
  decodeWorkspaceProfiles,
  type ProfileKind,
  type WorkspaceProfile,
} from "./workspace-profiles";
export class WorkspaceProfileManager {
  private profiles: readonly WorkspaceProfile[] = [];
  private busy = false;
  private disposed = false;
  private revision = 0;
  private formVersion = 0;
  private readonly abort = () => this.dispose();
  private defaultBusy = false;
  private defaultForm: DefaultProfileForm | undefined;
  private lifecycleForm: ProfileLifecycleForm | undefined;
  private editorForm: ProfileEditorForm | undefined;
  private lifecycleEngaged = false;
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      list: HTMLElement;
      status: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      save(nonce: string, kind: ProfileKind, hidden: boolean): Promise<unknown>;
      refresh(): Promise<readonly WorkspaceProfile[]>;
      working(busy: boolean): void;
      defaults?: {
        root: HTMLElement;
        native: DefaultProfileNative;
        launch(nonce: string, kind: DefaultProfileId): void;
      };
      lifecycle?: { root: HTMLElement; native: ProfileLifecycleNative };
      editor?: { root: HTMLElement; native: ProfileEditorNative };
    },
  ) {
    options.signal.addEventListener("abort", this.abort, { once: true });
    options.dialog.addEventListener("cancel", this.cancel);
    if (options.defaults)
      this.defaultForm = new DefaultProfileForm({
        ...options.defaults,
        signal: options.signal,
        nonce: options.nonce,
        working: (value) => {
          this.defaultBusy = value;
          this.render();
          if (!this.disposed && !options.signal.aborted)
            options.working(this.busy || value || this.lifecycleEngaged);
        },
        launch: (nonce, kind) => {
          this.close();
          options.defaults?.launch(nonce, kind);
        },
      });
    if (options.lifecycle)
      this.lifecycleForm = new ProfileLifecycleForm({
        ...options.lifecycle,
        signal: options.signal,
        nonce: options.nonce,
        engaged: (value) => {
          if (this.disposed || options.signal.aborted || !options.dialog.open)
            return;
          this.lifecycleEngaged = value;
          if (value) {
            ++this.formVersion;
            this.defaultForm?.close();
          }
          this.render();
          options.working(this.busy || this.defaultBusy || value);
          if (!value) void this.defaultForm?.open();
        },
        changed: async () => {
          const version = this.formVersion;
          const profiles = decodeWorkspaceProfiles(await options.refresh());
          if (
            version === this.formVersion &&
            !this.disposed &&
            !options.signal.aborted &&
            options.dialog.open
          )
            this.update(profiles);
        },
      });
  }
  private beginEdit(kind: ProfileKind): void {
    if (
      this.busy ||
      this.defaultBusy ||
      this.lifecycleEngaged ||
      !this.options.dialog.open ||
      !this.options.editor
    )
      return;
    if (!this.editorForm)
      this.editorForm = new ProfileEditorForm({
        ...this.options.editor,
        signal: this.options.signal,
        nonce: this.options.nonce,
        engaged: (value) => {
          if (
            this.disposed ||
            this.options.signal.aborted ||
            !this.options.dialog.open
          )
            return;
          this.lifecycleEngaged = value;
          if (value) {
            ++this.formVersion;
            this.defaultForm?.close();
            this.lifecycleForm?.close();
          }
          this.render();
          this.options.working(this.busy || this.defaultBusy || value);
        },
        changed: () => {
          if (!this.disposed && this.options.dialog.open) void this.openForms();
        },
      });
    void this.editorForm.open(kind);
  }
  private readonly cancel = (event: Event) => {
    event.preventDefault();
    this.close();
  };
  update(profiles: readonly WorkspaceProfile[]): void {
    this.profiles = decodeWorkspaceProfiles(profiles);
    this.render();
  }
  open(): void {
    if (
      this.disposed ||
      this.busy ||
      this.options.signal.aborted ||
      this.options.dialog.ownerDocument.querySelector("dialog[open]")
    )
      return;
    this.render();
    try {
      this.options.dialog.showModal();
      this.options.dialog.querySelector<HTMLElement>("button")?.focus();
      void this.openForms();
    } catch {
      uiText(this.options.status, "Profile 管理不可用");
    }
  }
  private async openForms(): Promise<void> {
    const version = ++this.formVersion;
    await this.defaultForm?.open();
    if (
      version === this.formVersion &&
      !this.disposed &&
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      !this.lifecycleEngaged
    )
      await this.lifecycleForm?.open();
  }
  private beginDelete(kind: ProfileKind): void {
    if (
      this.busy ||
      this.defaultBusy ||
      this.lifecycleEngaged ||
      !this.options.dialog.open
    )
      return;
    void this.lifecycleForm?.prepare(kind, "delete");
  }
  close(): void {
    ++this.formVersion;
    this.editorForm?.close();
    this.lifecycleForm?.close();
    this.lifecycleEngaged = false;
    if (!this.disposed && !this.options.signal.aborted)
      this.options.working(this.busy);
    this.defaultForm?.close();
    if (this.options.dialog.open) this.options.dialog.close();
  }
  private render(): void {
    if (this.disposed) return;
    const list = this.options.list;
    const revision = ++this.revision;
    list.replaceChildren();
    for (const profile of this.profiles) {
      const label = list.ownerDocument.createElement("label"),
        input = list.ownerDocument.createElement("input"),
        text = list.ownerDocument.createElement("span");
      input.type = "checkbox";
      input.checked = !profile.hidden;
      input.disabled = this.busy || this.defaultBusy || this.lifecycleEngaged;
      input.dataset.profile = profile.kind;
      uiAttribute(
        input,
        "aria-label",
        uiMessage(
          "显示 {0}",
          profile.kind === "sh" ? "POSIX sh" : profile.kind,
        ),
      );
      text.textContent = profile.kind === "sh" ? "POSIX sh" : profile.kind;
      label.append(input, text);
      input.addEventListener(
        "change",
        () => {
          if (revision === this.revision && this.options.dialog.open)
            void this.change(profile.kind, !input.checked);
        },
        { signal: this.options.signal },
      );
      if (this.lifecycleForm) {
        const row = list.ownerDocument.createElement("div"),
          remove = list.ownerDocument.createElement("button");
        row.className = "workspace-profile-row";
        remove.type = "button";
        uiText(
          remove,
          uiMessage(
            "删除 {0}",
            profile.kind === "sh" ? "POSIX sh" : profile.kind,
          ),
        );
        remove.disabled = input.disabled;
        remove.addEventListener(
          "click",
          () => {
            if (
              revision === this.revision &&
              this.options.dialog.open &&
              !remove.disabled
            )
              this.beginDelete(profile.kind);
          },
          { signal: this.options.signal },
        );
        row.append(label, remove);
        if (this.options.editor) {
          const edit = list.ownerDocument.createElement("button");
          edit.type = "button";
          uiText(
            edit,
            uiMessage(
              "编辑 {0}",
              profile.kind === "sh" ? "POSIX sh" : profile.kind,
            ),
          );
          edit.disabled = input.disabled;
          edit.addEventListener(
            "click",
            () => {
              if (
                revision === this.revision &&
                this.options.dialog.open &&
                !edit.disabled
              )
                this.beginEdit(profile.kind);
            },
            { signal: this.options.signal },
          );
          row.append(edit);
        }
        list.append(row);
      } else list.append(label);
    }
  }
  private async change(kind: ProfileKind, hidden: boolean): Promise<void> {
    const nonce = this.options.nonce();
    if (
      this.disposed ||
      this.busy ||
      this.defaultBusy ||
      !nonce ||
      this.lifecycleEngaged ||
      this.options.signal.aborted
    )
      return;
    this.busy = true;
    this.defaultForm?.close();
    this.lifecycleForm?.close();
    this.options.working(true);
    this.render();
    uiText(this.options.status, "正在保存 Profile 显示偏好…");
    try {
      const result = await changeProfileVisibility(
        nonce,
        kind,
        hidden,
        this.profiles,
        this.options.signal,
        this.options.save,
      );
      if (this.disposed || this.options.signal.aborted) return;
      if (result !== "saved") {
        uiText(
          this.options.status,
          "保存结果未确认 · 请重新加载检查，不自动重试",
        );
        this.close();
        return;
      }
      this.profiles = decodeWorkspaceProfiles(await this.options.refresh());
      if (!this.disposed) uiText(this.options.status, "Profile 显示偏好已保存");
    } catch {
      if (!this.disposed) {
        uiText(this.options.status, "无法读取已保存偏好 · 未回退默认 Shell");
        this.close();
      }
    } finally {
      this.busy = false;
      if (!this.disposed && !this.options.signal.aborted) {
        this.options.working(false);
        if (this.options.dialog.open) void this.openForms();
      }
      this.render();
    }
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.close();
    this.options.signal.removeEventListener("abort", this.abort);
    this.options.dialog.removeEventListener("cancel", this.cancel);
    this.options.list.replaceChildren();
  }
}
