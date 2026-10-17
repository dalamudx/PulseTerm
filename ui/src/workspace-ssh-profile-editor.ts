import { message, uiAttribute, uiText } from "./workspace-language";
import {
  type ProfileCatalogue,
  type ProfileChange,
  readSshProfiles,
  saveSshProfile,
  validateProfileChange,
} from "./workspace-ssh-profiles";
export class WorkspaceSshProfileEditor {
  private epoch = 0;
  private context:
    | {
        nonce: string;
        signal: AbortController;
        view: ProfileCatalogue | undefined;
      }
    | undefined;
  private selected: string | undefined;
  private pending = false;
  private focus: HTMLElement | undefined;
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      fields: HTMLElement;
      status: HTMLElement;
      done: HTMLButtonElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      enabled(): boolean;
      working(value: boolean): void;
      changed(value: ProfileCatalogue["hostView"]): void;
    },
    private readonly dependencies = {
      read: readSshProfiles,
      save: saveSshProfile,
    },
  ) {
    options.done.addEventListener("click", () => this.close(), {
      signal: options.signal,
    });
    options.dialog.addEventListener(
      "cancel",
      (e) => {
        e.preventDefault();
        this.close();
      },
      { signal: options.signal },
    );
    options.signal.addEventListener("abort", () => this.close(), {
      once: true,
    });
  }
  private current(
    context: NonNullable<WorkspaceSshProfileEditor["context"]>,
  ): boolean {
    return (
      this.context === context &&
      !context.signal.signal.aborted &&
      !this.options.signal.aborted &&
      this.options.nonce() === context.nonce
    );
  }
  close(): void {
    const same = this.context?.nonce === this.options.nonce();
    this.context?.signal.abort();
    this.context = undefined;
    this.epoch++;
    this.pending = false;
    this.options.fields.replaceChildren();
    this.options.status.textContent = "";
    if (this.options.dialog.open) this.options.dialog.close();
    this.options.working(false);
    if (same && !this.options.signal.aborted && this.focus?.isConnected)
      this.focus.focus();
    this.focus = undefined;
  }
  async open(): Promise<void> {
    const nonce = this.options.nonce();
    if (!nonce || !this.options.enabled() || this.options.signal.aborted)
      return;
    this.focus =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : undefined;
    const context = {
      nonce,
      signal: new AbortController(),
      view: undefined as ProfileCatalogue | undefined,
    };
    this.context?.signal.abort();
    this.context = context;
    this.pending = true;
    this.selected = undefined;
    this.epoch++;
    this.options.fields.replaceChildren();
    uiText(this.options.status, "读取 SSH 配置…");
    this.options.dialog.showModal();
    this.options.working(true);
    try {
      const view = await this.dependencies.read(nonce, context.signal.signal);
      if (!this.current(context)) return;
      context.view = view;
      this.options.changed(view.hostView);
      uiText(
        this.options.status,
        !view.migrated
          ? "请先在主机管理中启用多入口管理。"
          : view.profiles.length
            ? "选择配置，修改名称、登录用户名和默认身份。"
            : "还没有 SSH 入口，请先添加主机。",
      );
    } catch {
      if (this.current(context))
        uiText(this.options.status, "配置读取未确认，请关闭后重新打开。");
    } finally {
      if (this.current(context)) {
        this.pending = false;
        this.options.working(false);
        this.render();
      }
    }
  }
  private render(): void {
    const context = this.context,
      view = context?.view;
    const epoch = ++this.epoch;
    this.options.fields.replaceChildren();
    if (!context || !view || !view.profiles.length) return;
    const doc = this.options.fields.ownerDocument;
    const picker = doc.createElement("select");
    picker.id = "workspace-ssh-profile-picker";
    uiAttribute(picker, "aria-label", "选择SSH配置");
    for (const profile of view.profiles) {
      const endpoint = view.hostView.hosts.find(
        (e) => e.id === profile.endpointId,
      );
      const option = doc.createElement("option");
      option.value = profile.id;
      option.textContent = `${profile.label} · ${endpoint?.label ?? ""}`;
      picker.append(option);
    }
    if (!view.profiles.some((p) => p.id === this.selected))
      this.selected = view.profiles[0]?.id;
    picker.value = this.selected ?? "";
    const pickerLabel = doc.createElement("label");
    pickerLabel.htmlFor = picker.id;
    uiText(pickerLabel, "SSH 配置");
    this.options.fields.append(pickerLabel, picker);
    picker.addEventListener(
      "change",
      () => {
        if (!this.pending && this.current(context)) {
          this.selected = picker.value;
          this.render();
        }
      },
      { signal: context.signal.signal },
    );
    const profile = view.profiles.find((p) => p.id === this.selected);
    if (!profile) return;
    const endpoint = view.hostView.hosts.find(
        (e) => e.id === profile.endpointId,
      ),
      machine = view.hostView.machines?.find(
        (m) => m.id === endpoint?.machineId,
      );
    const summary = doc.createElement("p");
    summary.textContent = `${machine?.label ?? ""} · ${endpoint?.label ?? ""} · ${endpoint?.host ?? ""}:${endpoint?.port ?? ""}`;
    this.options.fields.append(summary);
    const form = doc.createElement("form");
    form.className = "workspace-host-form";
    uiAttribute(form, "aria-label", "编辑SSH配置");
    const field = (name: string, value: string, max: number) => {
      const label = doc.createElement("label"),
        input = doc.createElement("input");
      input.id = `workspace-ssh-profile-${name === "配置名称" ? "label" : "username"}`;
      uiAttribute(input, "aria-label", name);
      input.value = value;
      input.maxLength = max;
      input.autocomplete = "off";
      input.spellcheck = false;
      input.required = true;
      label.htmlFor = input.id;
      uiText(label, name);
      form.append(label, input);
      return input;
    };
    const label = field("配置名称", profile.label, 256),
      username = field("配置登录用户名", profile.username, 255);
    const identity = doc.createElement("select");
    identity.id = "workspace-ssh-profile-identity";
    uiAttribute(identity, "aria-label", "配置默认身份");
    const none = doc.createElement("option");
    none.value = "";
    uiText(none, "每次连接时选择");
    identity.append(none);
    for (const kind of ["password", "key", "agent"] as const) {
      const group = doc.createElement("optgroup");
      uiAttribute(
        group,
        "label",
        kind === "agent"
          ? "Agent 身份"
          : kind === "key"
            ? "私钥身份"
            : "密码身份",
      );
      for (const item of view.identities.filter((i) => i.kind === kind)) {
        const option = doc.createElement("option");
        option.value = item.id;
        uiText(
          option,
          message(
            "{0} · {1}{2}",
            item.label,
            item.username,
            item.available ? "" : message("（未设置凭据）"),
          ),
        );
        group.append(option);
      }
      if (group.childElementCount) identity.append(group);
    }
    if (
      profile.identityId &&
      !view.identities.some((i) => i.id === profile.identityId)
    ) {
      const missing = doc.createElement("option");
      missing.value = profile.identityId;
      uiText(missing, "原默认身份已删除，请重新选择");
      identity.append(missing);
    }
    identity.value = profile.identityId ?? "";
    const identityLabel = doc.createElement("label");
    identityLabel.htmlFor = identity.id;
    uiText(identityLabel, "默认身份");
    form.append(identityLabel, identity);
    const help = doc.createElement("p");
    uiText(help, "默认身份用于下次连接时预选，连接前仍可更改。");
    form.append(help);
    const jump = doc.createElement("select");
    jump.id = "workspace-ssh-profile-jump";
    uiAttribute(jump, "aria-label", "配置SSH跳板");
    const direct = doc.createElement("option");
    direct.value = "";
    uiText(direct, "直接连接");
    jump.append(direct);
    for (const item of view.profiles.filter((p) => p.id !== profile.id)) {
      const target = view.hostView.hosts.find((e) => e.id === item.endpointId);
      const option = doc.createElement("option");
      option.value = item.id;
      option.textContent = `${item.label} · ${target?.host ?? ""}:${target?.port ?? ""}`;
      jump.append(option);
    }
    jump.value = profile.jumpProfileId ?? "";
    const jumpLabel = doc.createElement("label");
    jumpLabel.htmlFor = jump.id;
    uiText(jumpLabel, "SSH 跳板");
    const jumpHelp = doc.createElement("p");
    jumpHelp.id = "workspace-ssh-profile-jump-help";
    uiText(
      jumpHelp,
      "可引用其他 SSH 配置，最多八跳。连接时需逐一确认每跳目标、身份和实际指纹。",
    );
    jump.setAttribute("aria-describedby", jumpHelp.id);
    form.append(jumpLabel, jump, jumpHelp);
    const save = doc.createElement("button");
    save.type = "submit";
    uiText(save, "保存SSH配置");
    form.append(save);
    this.options.fields.append(form);
    for (const element of [picker, label, username, identity, jump, save])
      element.disabled = this.pending;
    form.addEventListener(
      "submit",
      (event) => {
        event.preventDefault();
        if (
          this.pending ||
          !this.current(context) ||
          this.epoch !== epoch ||
          picker.value !== profile.id
        )
          return;
        try {
          const change = validateProfileChange({
            version: 2,
            via: jump.value || null,
            id: profile.id,
            label: label.value,
            username: username.value,
            identityId: identity.value || null,
          });
          if (
            change.identityId &&
            !view.identities.some((i) => i.id === change.identityId)
          )
            throw Error("identity.deleted");
          if (
            change.via &&
            !view.profiles.some(
              (p) => p.id === change.via && p.id !== profile.id,
            )
          )
            throw Error("jump.invalid");
          void this.save(context, change);
        } catch {
          uiText(
            this.options.status,
            "请核对配置名称、登录用户名、默认身份和跳板配置。",
          );
        }
      },
      { signal: context.signal.signal },
    );
  }
  private async save(
    context: NonNullable<WorkspaceSshProfileEditor["context"]>,
    change: ProfileChange,
  ): Promise<void> {
    const view = context.view;
    if (!view || this.pending || !this.current(context)) return;
    this.pending = true;
    this.options.working(true);
    uiText(this.options.status, "保存 SSH 配置…");
    this.render();
    try {
      const saved = await this.dependencies.save(
        context.nonce,
        view.token,
        change,
        context.signal.signal,
      );
      if (!this.current(context)) return;
      context.view = saved;
      this.selected = change.id;
      this.options.changed(saved.hostView);
      uiText(this.options.status, "SSH 配置已保存。");
    } catch {
      if (this.current(context)) {
        context.view = undefined;
        uiText(
          this.options.status,
          "保存未确认，配置或身份可能已变更。请关闭后重新打开检查。",
        );
      }
    } finally {
      if (this.current(context)) {
        this.pending = false;
        this.options.working(false);
        this.render();
      }
    }
  }
}
