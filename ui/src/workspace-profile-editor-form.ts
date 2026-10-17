import {
  type UiMessage,
  uiAttribute,
  message as uiMessage,
  uiNode,
  uiText,
} from "./workspace-language";
import { profileBehaviorFields } from "./workspace-profile-behavior-fields";
import {
  decodeProfileConfig,
  type ProfileConfig,
  ProfileEditor,
  type ProfileEditorNative,
  type ProfileEditorOffer,
} from "./workspace-profile-editor";
import type { ProfileKind } from "./workspace-profiles";
export type ProfileFormOffer = Pick<ProfileEditorOffer, "config"> & {
  shell?: ProfileKind;
  title?: UiMessage;
};
export interface ProfileEditorPort {
  read(shell: ProfileKind): Promise<ProfileFormOffer | null>;
  save(
    config: ProfileConfig,
  ): Promise<"saved" | "uncertain" | "cancelled" | "denied">;
  close(): void;
}
export class ProfileEditorForm {
  private editor: ProfileEditorPort | undefined;
  private version = 0;
  private events: AbortController | undefined;
  private expiry: ReturnType<typeof setTimeout> | undefined;
  private busy = false;
  constructor(
    private readonly options: {
      root: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      native?: ProfileEditorNative;
      editor?(nonce: string): ProfileEditorPort;
      fields?(form: HTMLFormElement): void;
      focus?(form: HTMLFormElement): void;
      validate?(): void;
      validationMessage?: UiMessage;
      engaged(value: boolean): void;
      changed(): void;
    },
  ) {
    options.signal.addEventListener("abort", this.close, { once: true });
  }
  close = (): void => {
    ++this.version;
    clearTimeout(this.expiry);
    this.events?.abort();
    this.events = undefined;
    this.editor?.close();
    this.editor = undefined;
    this.busy = false;
    this.options.root.replaceChildren();
  };
  async open(shell: ProfileKind): Promise<void> {
    this.close();
    const nonce = this.options.nonce();
    if (!nonce || this.options.signal.aborted) return;
    const version = this.version;
    this.options.engaged(true);
    const editor =
      this.options.editor?.(nonce) ??
      (this.options.native
        ? new ProfileEditor(nonce, this.options.signal, this.options.native)
        : undefined);
    if (!editor) {
      this.showResult("读取失败，请关闭编辑后重新打开。");
      return;
    }
    this.editor = editor;
    uiText(this.options.root, "正在读取 Profile 配置…");
    const offer = await editor.read(shell);
    if (
      version !== this.version ||
      this.options.signal.aborted ||
      nonce !== this.options.nonce()
    )
      return;
    if (!offer) {
      this.showResult("读取失败，请关闭编辑后重新打开。");
      return;
    }
    this.render(offer, version, nonce);
    this.expiry = setTimeout(() => {
      if (version === this.version) {
        editor.close();
        this.showResult("编辑已过期，请关闭后重新打开。");
      }
    }, 55000);
  }
  private finish = (): void => {
    this.close();
    this.options.engaged(false);
    this.options.changed();
  };
  private showResult(message: string): void {
    this.events?.abort();
    const root = this.options.root,
      doc = root.ownerDocument;
    const status = doc.createElement("p"),
      done = doc.createElement("button");
    status.setAttribute("role", "status");
    uiText(status, message);
    done.type = "button";
    uiText(done, "关闭编辑");
    done.addEventListener("click", this.finish, { once: true });
    root.replaceChildren(status, done);
  }
  private render(
    offer: ProfileFormOffer,
    version: number,
    nonce: string,
  ): void {
    const root = this.options.root,
      doc = root.ownerDocument;
    const events = new AbortController();
    this.events = events;
    const current = () =>
      version === this.version &&
      !events.signal.aborted &&
      !this.options.signal.aborted &&
      this.options.nonce() === nonce;
    const form = doc.createElement("form");
    form.className = "workspace-host-form workspace-profile-editor";
    const title = doc.createElement("h3");
    uiText(
      title,
      offer.title ??
        uiMessage(
          "编辑 {0}",
          offer.shell === "sh" ? "POSIX sh" : (offer.shell ?? ""),
        ),
    );
    const note = doc.createElement("p");
    uiText(
      note,
      "保存后用于新终端。参数保持原样；环境变量为普通配置，请勿填写密码或密钥。",
    );
    form.append(title, note);
    this.options.fields?.(form);
    const field = (labelText: string, value: string) => {
      const label = doc.createElement("label"),
        input = doc.createElement("input");
      uiText(label, labelText);
      input.value = value;
      input.autocomplete = "off";
      input.spellcheck = false;
      label.append(input);
      return { label, input };
    };
    const args = doc.createElement("fieldset"),
      legend = doc.createElement("legend"),
      argRows = doc.createElement("div"),
      addArg = doc.createElement("button");
    uiText(legend, "参数（按顺序；空字段作为空参数）");
    addArg.type = "button";
    uiText(addArg, "添加参数");
    const argInputs: HTMLInputElement[] = [];
    const addArgument = (value: string) => {
      const row = doc.createElement("div"),
        { label, input } = field("参数", value),
        remove = doc.createElement("button");
      row.className = "workspace-profile-option-row";
      input.maxLength = 16384;
      argInputs.push(input);
      argInputs.forEach((item, index) => {
        uiAttribute(item, "aria-label", uiMessage("参数 {0}", index + 1));
      });
      remove.type = "button";
      uiText(remove, "移除参数");
      remove.addEventListener(
        "click",
        () => {
          if (!current() || this.busy) return;
          argInputs.splice(argInputs.indexOf(input), 1);
          argInputs.forEach((item, index) => {
            uiAttribute(item, "aria-label", uiMessage("参数 {0}", index + 1));
          });
          row.remove();
          addArg.disabled = false;
        },
        { signal: events.signal },
      );
      row.append(label, remove);
      argRows.append(row);
      addArg.disabled = argInputs.length >= 64;
    };
    for (const arg of offer.config.args) addArgument(arg);
    addArg.addEventListener(
      "click",
      () => {
        if (current() && !this.busy && argInputs.length < 64) addArgument("");
      },
      { signal: events.signal },
    );
    args.append(legend, argRows, addArg);
    form.append(args);
    const cwd = field(
      "工作目录（留空继承；填写绝对路径）",
      offer.config.cwd ?? "",
    );
    cwd.input.maxLength = 4096;
    form.append(cwd.label);
    const env = doc.createElement("fieldset"),
      envLegend = doc.createElement("legend"),
      envRows = doc.createElement("div"),
      addEnv = doc.createElement("button");
    uiText(envLegend, "环境变量（显式 TERM 可覆盖全局默认值）");
    addEnv.type = "button";
    uiText(addEnv, "添加环境变量");
    const envInputs: {
      key: HTMLInputElement;
      value: HTMLInputElement;
      remove: HTMLInputElement;
    }[] = [];
    const labelEnvironment = () =>
      envInputs.forEach((item, index) => {
        uiAttribute(
          item.key,
          "aria-label",
          uiMessage("环境变量 {0} 名称", index + 1),
        );
        uiAttribute(
          item.value,
          "aria-label",
          uiMessage("环境变量 {0} 值", index + 1),
        );
        uiAttribute(
          item.remove,
          "aria-label",
          uiMessage("环境变量 {0} 移除继承值", index + 1),
        );
      });
    const addEnvironment = (name: string, value: string, removed: boolean) => {
      const row = doc.createElement("div"),
        key = field("名称", name),
        val = field("值", value),
        label = doc.createElement("label"),
        check = doc.createElement("input"),
        remove = doc.createElement("button");
      row.className = "workspace-profile-option-row";
      key.input.maxLength = 128;
      val.input.maxLength = 16384;
      check.type = "checkbox";
      check.checked = removed;
      val.input.disabled = removed;
      label.append(check, uiNode(doc, "移除继承值"));
      check.addEventListener(
        "change",
        () => {
          if (current() && !this.busy) val.input.disabled = check.checked;
        },
        { signal: events.signal },
      );
      const entry = { key: key.input, value: val.input, remove: check };
      envInputs.push(entry);
      labelEnvironment();
      remove.type = "button";
      uiText(remove, "移除条目");
      remove.addEventListener(
        "click",
        () => {
          if (!current() || this.busy) return;
          envInputs.splice(envInputs.indexOf(entry), 1);
          labelEnvironment();
          row.remove();
          addEnv.disabled = false;
        },
        { signal: events.signal },
      );
      row.append(key.label, val.label, label, remove);
      envRows.append(row);
      addEnv.disabled = envInputs.length >= 64;
    };
    for (const [key, value] of offer.config.env)
      addEnvironment(key, value, false);
    for (const key of offer.config.envRemove) addEnvironment(key, "", true);
    addEnv.addEventListener(
      "click",
      () => {
        if (current() && !this.busy && envInputs.length < 64)
          addEnvironment("", "", false);
      },
      { signal: events.signal },
    );
    env.append(envLegend, envRows, addEnv);
    form.append(env);
    const appearance = doc.createElement("fieldset"),
      appearanceLegend = doc.createElement("legend");
    uiText(appearanceLegend, "终端外观");
    const size = field(
      "字体大小（留空继承；6–72）",
      String(offer.config.appearance.fontSize ?? ""),
    );
    size.input.type = "number";
    size.input.min = "6";
    size.input.max = "72";
    size.input.step = "1";
    const family = field(
      "字体名称（留空继承）",
      offer.config.appearance.fontFamily ?? "",
    );
    family.input.maxLength = 256;
    const blinkLabel = doc.createElement("label"),
      blink = doc.createElement("select");
    uiText(blinkLabel, "光标闪烁");
    for (const [value, text] of [
      ["inherit", "继承设置"],
      ["on", "启用"],
      ["off", "停用"],
    ]) {
      const option = doc.createElement("option");
      option.value = value ?? "";
      uiText(option, text ?? "");
      blink.append(option);
    }
    blink.value =
      offer.config.appearance.cursorBlink === null
        ? "inherit"
        : offer.config.appearance.cursorBlink
          ? "on"
          : "off";
    blinkLabel.append(blink);
    appearance.append(appearanceLegend, size.label, family.label, blinkLabel);
    const behavior = profileBehaviorFields(doc, offer.config.appearance);
    form.append(appearance, behavior.root);
    const status = doc.createElement("p"),
      actions = doc.createElement("div"),
      save = doc.createElement("button"),
      cancel = doc.createElement("button");
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    actions.className = "terminal-paste-actions";
    save.type = "submit";
    uiText(save, "保存配置");
    cancel.type = "button";
    uiText(cancel, "取消编辑");
    cancel.addEventListener(
      "click",
      () => {
        if (current()) this.finish();
      },
      { signal: events.signal },
    );
    actions.append(cancel, save);
    form.append(status, actions);
    form.addEventListener(
      "submit",
      async (event) => {
        event.preventDefault();
        if (!current() || this.busy) return;
        let config: ProfileConfig;
        try {
          this.options.validate?.();
          config = decodeProfileConfig({
            args: argInputs.map((input) => input.value),
            cwd: cwd.input.value || null,
            env: envInputs
              .filter((e) => !e.remove.checked)
              .map((e) => [e.key.value, e.value.value]),
            envRemove: envInputs
              .filter((e) => e.remove.checked)
              .map((e) => e.key.value),
            appearance: {
              ...behavior.read(),
              fontSize: size.input.value ? Number(size.input.value) : null,
              fontFamily: family.input.value || null,
              cursorBlink:
                blink.value === "inherit" ? null : blink.value === "on",
            },
          });
        } catch {
          uiText(
            status,
            this.options.validationMessage ??
              "请检查参数总量、绝对路径、变量名称和外观值。",
          );
          return;
        }
        this.busy = true;
        for (const node of form.querySelectorAll<
          HTMLInputElement | HTMLButtonElement | HTMLSelectElement
        >("input,button,select"))
          node.disabled = true;
        cancel.disabled = false;
        uiText(status, "正在保存配置…");
        const result = await this.editor?.save(config);
        if (!current()) return;
        this.busy = false;
        clearTimeout(this.expiry);
        this.showResult(
          result === "saved"
            ? "Profile 配置已保存。新终端将使用此配置。"
            : "保存结果未确认，请关闭后重新读取检查。",
        );
      },
      { signal: events.signal },
    );
    root.replaceChildren(form);
    if (this.options.focus) this.options.focus(form);
    else cwd.input.focus();
  }
}
