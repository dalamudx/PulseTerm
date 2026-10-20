import { type UiAccent, uiAccentField } from "./workspace-accent";
import { type UiAppearance, uiAppearanceField } from "./workspace-appearance";
import { backspaceField } from "./workspace-backspace-field";
import { bindFontPreset } from "./workspace-font-preset";
import {
  applyUiLanguage,
  message,
  uiAttribute,
  uiContent,
  uiText,
} from "./workspace-language";
import { languageField } from "./workspace-language-field";
import {
  readWorkspaceSettings,
  type SettingsChange,
  saveWorkspaceSettings,
  type WorkspaceSettings,
} from "./workspace-settings";
import {
  type SettingsEntry,
  WorkspaceSettingsNavigation,
} from "./workspace-settings-navigation";
import { terminalBehaviorField } from "./workspace-terminal-behavior-fields";
import { terminalPresetField } from "./workspace-terminal-preset";
import { terminalThemeField } from "./workspace-terminal-theme";
export class WorkspaceSettingsEditor {
  private projection: WorkspaceSettings | undefined;
  private busy = false;
  private disposed = false;
  private epoch = 0;
  private rendered: AbortController | undefined;
  private operation: AbortController | undefined;
  private readonly rows = new Map<SettingsEntry, HTMLElement>();
  private suspended: { nonce: string; epoch: number } | undefined;
  private readonly navigation: WorkspaceSettingsNavigation | undefined;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      fields: HTMLElement;
      navigation?: HTMLElement;
      openKeymap?(): void;
      status: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(busy: boolean): void;
      changed(value: WorkspaceSettings): boolean | undefined;
      applyAppearance?(value: UiAppearance): boolean;
      applyAccent?(value: UiAccent): boolean;
    },
    private readonly dependencies = {
      read: readWorkspaceSettings,
      save: saveWorkspaceSettings,
    },
  ) {
    this.navigation = options.navigation
      ? new WorkspaceSettingsNavigation({
          root: options.navigation,
          fields: options.fields,
          signal: options.signal,
        })
      : undefined;
    options.signal.addEventListener("abort", this.abort, { once: true });
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      { signal: options.signal },
    );
    if (options.signal.aborted) this.dispose();
  }
  async open(): Promise<void> {
    if (
      this.disposed ||
      this.busy ||
      this.suspended ||
      this.options.dialog.open
    )
      return;
    this.options.dialog.showModal();
    await this.load();
  }
  /** Keep local drafts and navigation while a child editor owns modal focus. */
  suspend(): (() => void) | undefined {
    const nonce = this.options.nonce();
    if (!nonce || this.busy || !this.projection || !this.current(this.epoch))
      return;
    const suspended = { nonce, epoch: this.epoch };
    const doc = this.options.dialog.ownerDocument;
    const focused = doc.activeElement as HTMLElement | null;
    this.suspended = suspended;
    this.options.dialog.close();
    return () => {
      if (this.suspended !== suspended) return;
      if (
        this.disposed ||
        this.options.signal.aborted ||
        suspended.epoch !== this.epoch ||
        suspended.nonce !== this.options.nonce() ||
        doc.querySelector("dialog[open]")
      ) {
        this.close();
        return;
      }
      this.suspended = undefined;
      this.options.dialog.showModal();
      if (focused?.isConnected) focused.focus({ preventScroll: true });
      else this.navigation?.focusSearch();
    };
  }
  close(): void {
    const wasBusy = this.busy;
    this.suspended = undefined;
    this.epoch++;
    this.rendered?.abort();
    this.rendered = undefined;
    this.operation?.abort();
    this.operation = undefined;
    this.busy = false;
    this.projection = undefined;
    this.navigation?.clear();
    if (wasBusy) this.options.working(false);
    if (this.options.dialog.open) this.options.dialog.close();
  }
  private async load(): Promise<void> {
    const nonce = this.options.nonce();
    if (!nonce) return;
    const epoch = ++this.epoch,
      operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.options.fields.replaceChildren();
    this.navigation?.setBusy(true);
    uiText(this.options.status, "正在读取已验证的设置…");
    try {
      const projection = await this.dependencies.read(nonce, operation.signal);
      if (!this.current(epoch)) return;
      this.projection = projection;
      if (projection.values.ui_language)
        applyUiLanguage(
          this.options.fields.ownerDocument,
          projection.values.ui_language.value,
          this.options.signal,
        );
      const applied = projection.values.ui_appearance
        ? this.options.applyAppearance?.(projection.values.ui_appearance.value)
        : true;
      this.options.applyAccent?.(projection.values.ui_accent?.value ?? "cyan");
      uiText(
        this.options.status,
        applied === false
          ? "无法读取系统外观 · 保留当前显示，可选择浅色或深色"
          : "界面外观保存后立即应用；配色用于新建本地或 SSH 终端，其他终端设置在下次显示时应用",
      );
      this.render();
    } catch {
      if (this.current(epoch))
        uiText(this.options.status, "设置不可用 · 未使用默认值覆盖文件");
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
        this.unlock();
        this.navigation?.focusSearch();
      }
    }
  }
  private current(epoch: number): boolean {
    return (
      !this.disposed &&
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      epoch === this.epoch
    );
  }
  private unlock(): void {
    this.navigation?.setBusy(this.busy || !this.projection);
    for (const control of this.options.fields.querySelectorAll<
      HTMLInputElement | HTMLSelectElement | HTMLButtonElement
    >("input,select,button"))
      control.disabled = this.busy;
  }
  private render(): void {
    const projection = this.projection;
    if (!projection) return;
    const doc = this.options.fields.ownerDocument,
      epoch = this.epoch;
    this.options.fields.replaceChildren();
    this.rows.clear();
    this.rendered?.abort();
    this.rendered = new AbortController();
    if (projection.values.ui_language) {
      this.append(
        "ui_language",
        languageField({
          document: doc,
          setting: projection.values.ui_language,
          signal: this.rendered.signal,
          current: () =>
            this.current(epoch) && this.projection === projection && !this.busy,
          save: (change) => {
            void this.save(change);
          },
        }),
      );
    }
    if (projection.values.ui_appearance) {
      this.append(
        "ui_appearance",
        uiAppearanceField({
          document: doc,
          setting: projection.values.ui_appearance,
          signal: this.rendered.signal,
          current: () =>
            this.current(epoch) && this.projection === projection && !this.busy,
          save: (change) => {
            void this.save(change);
          },
        }),
      );
    }
    if (projection.values.ui_accent) {
      this.append(
        "ui_accent",
        uiAccentField({
          document: doc,
          setting: projection.values.ui_accent,
          signal: this.rendered.signal,
          current: () => this.current(epoch) && this.projection === projection,
          save: (change) => {
            void this.save(change);
          },
        }),
      );
    }
    if (projection.values.terminal_preset && projection.terminalPresets) {
      this.append(
        "terminal_preset",
        terminalPresetField({
          document: doc,
          setting: projection.values.terminal_preset,
          presets: projection.terminalPresets,
          systemTheme:
            projection.values.terminal_theme?.value.mode === "system",
          signal: this.rendered.signal,
          current: () => this.current(epoch) && this.projection === projection,
          save: (change) => {
            void this.save(change);
          },
        }),
      );
    }
    if (projection.values.terminal_theme && projection.terminalPresets) {
      this.append(
        "terminal_theme",
        terminalThemeField({
          document: doc,
          setting: projection.values.terminal_theme,
          presets: projection.terminalPresets,
          signal: this.rendered.signal,
          current: () => this.current(epoch) && this.projection === projection,
          save: (change) => {
            void this.save(change);
          },
        }),
      );
    }
    if (projection.values.backspace_send) {
      this.append(
        "backspace_send",
        backspaceField({
          document: doc,
          setting: projection.values.backspace_send,
          current: () =>
            this.current(epoch) && this.projection === projection && !this.busy,
          save: (change) => {
            void this.save(change);
          },
        }),
      );
    }
    for (const field of ["delete_send", "term_type"] as const) {
      const setting = projection.values[field];
      if (setting)
        this.append(
          field,
          terminalBehaviorField({
            document: doc,
            field,
            setting,
            signal: this.rendered.signal,
            current: () =>
              this.current(epoch) &&
              this.projection === projection &&
              !this.busy,
            save: (change) => {
              void this.save(change);
            },
          }),
        );
    }
    for (const field of [
      "history_limit",
      "font_size",
      "font_family",
      "font_fallback",
      "font_ligatures",
      "line_height_percent",
      "letter_spacing",
      "cursor_blink",
      "scroll_on_input",
      "scroll_on_output",
      "mac_option_as_meta",
      "copy_on_select",
      "right_click_paste",
      "confirm_close",
    ] as const) {
      const value = projection.values[field],
        row = doc.createElement("form"),
        label = doc.createElement("label"),
        input = doc.createElement("input"),
        save = doc.createElement("button"),
        reset = doc.createElement("button"),
        source = doc.createElement("span");
      if (!value) continue;
      uiText(
        label,
        (
          {
            history_limit: "保留历史行数",
            font_size: "字体大小",
            font_family: "字体名称",
            font_fallback: "后备字体",
            font_ligatures: "运算符连字",
            line_height_percent: "行高（%）",
            letter_spacing: "字距（px）",
            cursor_blink: "光标闪烁",
            scroll_on_input: "输入时回到底部",
            scroll_on_output: "新输出时回到底部",
            mac_option_as_meta: "macOS Option 作为 Meta 键",
            copy_on_select: "选中复制",
            right_click_paste: "右键粘贴",
            confirm_close: "关闭会话前确认",
          } as const
        )[field],
      );
      input.type =
        field === "font_ligatures" ||
        field === "cursor_blink" ||
        field === "scroll_on_input" ||
        field === "scroll_on_output" ||
        field === "mac_option_as_meta" ||
        field === "copy_on_select" ||
        field === "right_click_paste" ||
        field === "confirm_close"
          ? "checkbox"
          : field === "history_limit" ||
              field === "font_size" ||
              field === "line_height_percent" ||
              field === "letter_spacing"
            ? "number"
            : "text";
      if (
        field === "font_ligatures" ||
        field === "cursor_blink" ||
        field === "scroll_on_input" ||
        field === "scroll_on_output" ||
        field === "mac_option_as_meta" ||
        field === "copy_on_select" ||
        field === "right_click_paste" ||
        field === "confirm_close"
      )
        input.checked = value.value === true;
      else input.value = String(value.value);
      if (field === "scroll_on_input") {
        uiAttribute(
          input,
          "title",
          "关闭后，输入时保留回看位置；下次显示或重建视图生效",
        );
      }
      if (field === "history_limit") {
        input.min = "1000";
        input.max = "100000";
        input.step = "1";
      }
      if (field === "font_size") {
        input.min = "6";
        input.max = "72";
        input.step = "1";
      }
      if (field === "line_height_percent" || field === "letter_spacing") {
        input.min = field === "line_height_percent" ? "100" : "0";
        input.max = field === "line_height_percent" ? "300" : "8";
        input.step = "1";
      }
      if (field === "font_family" || field === "font_fallback")
        input.maxLength = 128;
      label.append(input);
      uiText(
        source,
        message(
          "{0}{1}",
          message(value.source === "user" ? "用户覆盖" : "内置默认"),
          message(
            field === "scroll_on_output"
              ? " · 开启后回看时也跟随新输出，下次打开或刷新终端生效"
              : field === "mac_option_as_meta"
                ? " · 仅 macOS，下次打开或刷新终端生效"
                : field === "history_limit"
                  ? " · 新建终端生效，最多十万行；16MiB 冷区预算可能提前裁剪"
                  : "",
          ),
        ),
      );
      save.type = "submit";
      uiText(save, "保存");
      uiAttribute(save, "aria-label", message("保存{0}", uiContent(label)));
      reset.type = "button";
      uiText(reset, "恢复继承");
      uiAttribute(
        reset,
        "aria-label",
        message("{0}恢复继承", uiContent(label)),
      );
      row.className = "workspace-setting-row";
      row.append(label, source, save, reset);
      if (field === "font_family")
        bindFontPreset(
          doc,
          label,
          input,
          row,
          () =>
            this.current(epoch) && this.projection === projection && !this.busy,
        );
      row.addEventListener("submit", (event) => {
        event.preventDefault();
        if (!this.current(epoch) || this.projection !== projection || this.busy)
          return;
        const change: SettingsChange =
          field === "history_limit" ||
          field === "font_size" ||
          field === "line_height_percent" ||
          field === "letter_spacing"
            ? { field, value: Number(input.value) }
            : field === "font_family" || field === "font_fallback"
              ? { field, value: input.value }
              : { field, value: input.checked };
        void this.save(change);
      });
      reset.addEventListener("click", () => {
        if (this.current(epoch) && this.projection === projection && !this.busy)
          void this.save({ field: `reset_${field}` });
      });
      this.append(field, row);
    }
    if (this.options.openKeymap) {
      const row = doc.createElement("div");
      row.className = "workspace-setting-row";
      const button = doc.createElement("button");
      button.id = "workspace-keymap-open";
      button.type = "button";
      uiText(button, "编辑快捷键…");
      button.addEventListener(
        "click",
        () => {
          if (
            this.current(epoch) &&
            this.projection === projection &&
            !this.busy
          )
            this.options.openKeymap?.();
        },
        { signal: this.rendered.signal },
      );
      const hint = doc.createElement("span");
      uiText(hint, "修改动作键位、禁用快捷键或恢复内置");
      row.append(button, hint);
      this.append("keybindings", row);
    }
    this.navigation?.update(this.rows);
  }
  private append(field: SettingsEntry, row: HTMLElement): void {
    this.rows.set(field, row);
    this.options.fields.append(row);
  }
  private captureFocus(): (() => void) | undefined {
    const doc = this.options.fields.ownerDocument;
    const active = doc.activeElement;
    if (!active) return;
    for (const [field, row] of this.rows) {
      const controls = Array.from(
        row.querySelectorAll<HTMLElement>("input,select,button"),
      );
      const index = controls.indexOf(active as HTMLElement);
      if (index < 0) continue;
      return () => {
        // Do not steal focus if the user moved to another dialog control while saving.
        if (doc.activeElement !== doc.body && doc.activeElement !== active)
          return;
        const replacement = this.rows.get(field);
        if (replacement && !replacement.hidden) {
          replacement
            .querySelectorAll<HTMLElement>("input,select,button")
            [index]?.focus();
        }
      };
    }
  }
  private captureDrafts(): () => void {
    const scrollTop = this.options.fields.scrollTop;
    const drafts = [...this.rows]
      .filter(([key]) => key !== "ui_language")
      .map(([key, row]) => ({
        key,
        values: Array.from(
          row.querySelectorAll<HTMLInputElement | HTMLSelectElement>(
            "input,select",
          ),
        ).map((input) => ({
          value: input.value,
          checked: "checked" in input ? input.checked : undefined,
        })),
      }));
    return () => {
      for (const { key, values } of drafts) {
        const controls = this.rows
          .get(key)
          ?.querySelectorAll<HTMLInputElement | HTMLSelectElement>(
            "input,select",
          );
        if (!controls) continue;
        values.forEach((draft, index) => {
          const input = controls[index];
          if (!input) return;
          input.value = draft.value;
          if (draft.checked !== undefined && "checked" in input)
            input.checked = draft.checked;
        });
        if (
          [
            "ui_appearance",
            "ui_accent",
            "terminal_preset",
            "terminal_theme",
          ].includes(key)
        )
          controls.forEach((input) => {
            input.dispatchEvent?.(new Event("change"));
          });
      }
      this.options.fields.scrollTop = scrollTop;
    };
  }
  private async save(change: SettingsChange): Promise<void> {
    const projection = this.projection,
      nonce = this.options.nonce();
    if (!projection || !nonce || this.busy || this.disposed) return;
    const restoreFocus = this.captureFocus();
    const restoreDrafts =
      change.field === "ui_language" || change.field === "reset_ui_language"
        ? this.captureDrafts()
        : undefined;
    let saved = false;
    const epoch = this.epoch,
      operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.unlock();
    uiText(this.options.status, "正在保存单个字段…");
    try {
      const result = await this.dependencies.save(
        nonce,
        projection.token,
        change,
        operation.signal,
      );
      if (!this.current(epoch)) return;
      this.projection = result;
      const applied = this.options.changed(result);
      this.render();
      saved = true;
      uiText(
        this.options.status,
        change.field === "ui_language" || change.field === "reset_ui_language"
          ? "已保存 · 界面语言已应用"
          : change.field === "ui_appearance" ||
              change.field === "reset_ui_appearance" ||
              change.field === "ui_accent" ||
              change.field === "reset_ui_accent"
            ? applied === false
              ? "已保存 · 系统外观不可用，保留当前显示"
              : "已保存 · 界面外观已应用"
            : change.field === "terminal_preset" ||
                change.field === "terminal_theme" ||
                change.field === "reset_terminal_theme" ||
                change.field === "reset_terminal_preset"
              ? "已保存 · 用于新建本地或 SSH 终端；已有终端保持原配色"
              : "已保存 · 重新加载终端后应用；当前会话未关闭",
      );
    } catch {
      if (this.current(epoch)) {
        this.projection = undefined;
        this.rendered?.abort();
        this.options.fields.replaceChildren();
        this.rows.clear();
        this.navigation?.clear();
        uiText(
          this.options.status,
          "保存未确认 · 不自动重试，关闭并重新打开以读取实际结果",
        );
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
        this.unlock();
        if (saved) {
          restoreDrafts?.();
          restoreFocus?.();
        }
      }
    }
  }
  dispose(): void {
    if (this.disposed) return;
    this.close();
    this.disposed = true;
    this.options.signal.removeEventListener("abort", this.abort);
  }
}
