import { CatalogueBatch } from "./workspace-catalogue-batch";
import {
  type HostCatalogue,
  type HostChange,
  type HostFields,
  readHosts,
  saveHost,
  validateHostChange,
} from "./workspace-hosts";
import { uiAttribute, uiText } from "./workspace-language";
export class WorkspaceHostEditor {
  private batch: CatalogueBatch | undefined;
  private value: HostCatalogue | undefined;
  private epoch = 0;
  private renderGeneration = 0;
  private busy = false;
  private disposed = false;
  private operation: AbortController | undefined;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      done: HTMLButtonElement;
      fields: HTMLElement;
      status: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(busy: boolean): void;
      changed?(value: HostCatalogue): void;
      fingerprint?(host: string): void;
      fingerprintEnabled?(): boolean;
    },
    private readonly dependencies = { read: readHosts, save: saveHost },
  ) {
    options.signal.addEventListener("abort", this.abort, { once: true });
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      { signal: options.signal },
    );
  }
  async open(): Promise<void> {
    if (this.disposed || this.busy || this.options.dialog.open) return;
    this.options.dialog.showModal();
    await this.load();
  }
  close(): void {
    this.epoch++;
    this.operation?.abort();
    this.operation = undefined;
    this.value = undefined;
    this.busy = false;
    this.options.fields.replaceChildren();
    if (this.options.dialog.open) this.options.dialog.close();
    this.options.working(false);
  }
  private current(epoch: number): boolean {
    return (
      !this.disposed &&
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      epoch === this.epoch
    );
  }
  private lock(): void {
    for (const control of this.options.fields.querySelectorAll<
      HTMLInputElement | HTMLButtonElement
    >("input,button,select"))
      control.disabled = this.busy;
    this.batch?.setBusy(this.busy);
  }
  private async load(): Promise<void> {
    const nonce = this.options.nonce();
    if (!nonce) return;
    const epoch = ++this.epoch;
    const operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.options.fields.replaceChildren();
    uiText(this.options.status, "正在读取主机…");
    try {
      const value = await this.dependencies.read(nonce, operation.signal);
      if (!this.current(epoch)) return;
      this.value = value;
      this.options.changed?.(value);
      this.render();
      uiText(
        this.options.status,
        value.hosts.length
          ? "仅保存连接目标，不批准主机指纹或建立连接"
          : "尚未保存主机",
      );
    } catch {
      if (this.current(epoch))
        uiText(
          this.options.status,
          "主机列表不可用 · 未使用空列表替代数据库内容",
        );
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
        this.lock();
      }
    }
  }
  private render(selection?: string): void {
    const value = this.value;
    if (!value) return;
    const epoch = this.epoch;
    this.options.fields.replaceChildren();
    const generation = ++this.renderGeneration;
    const label = document.createElement("label"),
      select = document.createElement("select");
    uiText(label, "编辑 SSH 入口");
    uiAttribute(select, "aria-label", "选择 SSH 主机或添加主机");
    const create = document.createElement("option");
    create.value = "";
    uiText(create, "添加 SSH 入口");
    select.append(create);
    for (const host of value.hosts) {
      const option = document.createElement("option");
      option.value = host.id;
      option.textContent = host.label;
      select.append(option);
    }
    const selected = value.hosts.find(
      (host) => host.id === (selection ?? value.hosts[0]?.id),
    );
    select.value = selected?.id ?? "";
    select.addEventListener("change", () => {
      if (
        this.current(epoch) &&
        this.value === value &&
        !this.busy &&
        this.renderGeneration === generation
      )
        this.render(select.value);
    });
    label.append(select);
    this.options.fields.append(label);
    this.form(
      epoch,
      value,
      selected?.id,
      selected ?? { label: "", host: "", port: 22, username: "" },
    );
    if (selected?.machineId)
      this.machineForm(epoch, value, generation, selected.machineId);
    if (value.version === 2 && !value.migrated && value.hosts.length) {
      const migrate = document.createElement("button");
      migrate.type = "button";
      uiText(migrate, "启用多入口管理");
      migrate.addEventListener("click", () => {
        if (
          this.current(epoch) &&
          this.value === value &&
          !this.busy &&
          this.renderGeneration === generation
        )
          void this.save({ action: "migrate_catalogue" });
      });
      this.options.fields.append(migrate);
    }
    this.batch = new CatalogueBatch({
      container: this.options.fields,
      rows: value.hosts.map((row) => ({
        id: row.id,
        label: row.label,
        description: `${row.username}@${row.host}:${row.port}`,
      })),
      kind: "SSH 主机",
      consequence: "现有会话会继续运行。",
      current: () =>
        this.current(epoch) &&
        this.value === value &&
        !this.busy &&
        this.renderGeneration === generation,
      edit: (id) => {
        this.render(id);
        this.lock();
      },
      remove: (ids) => {
        void this.save({ action: "remove_many", ids });
      },
    });
  }
  private form(
    epoch: number,
    value: HostCatalogue,
    id: string | undefined,
    initial: HostFields,
  ): void {
    const generation = this.renderGeneration;
    const form = document.createElement("form");
    form.className = "workspace-host-form";
    const inputs = new Map<string, HTMLInputElement>();
    for (const [field, title] of [
      ["label", "显示名称"],
      ["host", "主机地址"],
      ["port", "端口"],
      ["username", "用户名"],
    ] as const) {
      const label = document.createElement("label"),
        input = document.createElement("input");
      uiText(label, title);
      input.name = field;
      input.type = field === "port" ? "number" : "text";
      input.required = true;
      input.value = String(initial[field]);
      input.autocomplete = "off";
      input.spellcheck = false;
      input.maxLength = field === "label" ? 256 : 255;
      if (field === "port") {
        input.min = "1";
        input.max = "65535";
        input.step = "1";
      }
      label.append(input);
      form.append(label);
      inputs.set(field, input);
    }
    let parent: HTMLSelectElement | undefined;
    if (!id && value.version === 2 && value.migrated) {
      const label = document.createElement("label");
      uiText(label, "归属机器");
      parent = document.createElement("select");
      uiAttribute(parent, "aria-label", "选择入口归属机器");
      const create = document.createElement("option");
      create.value = "";
      uiText(create, "新建机器");
      parent.append(create);
      for (const machine of value.machines ?? []) {
        const option = document.createElement("option");
        option.value = machine.id;
        option.textContent = machine.label;
        parent.append(option);
      }
      parent.value = "";
      label.append(parent);
      form.append(label);
    }
    const actions = document.createElement("div");
    actions.className = "terminal-paste-actions";
    const save = document.createElement("button");
    save.type = "submit";
    uiText(save, id ? "保存" : "添加");
    actions.append(save);
    const valid = () =>
      this.current(epoch) &&
      this.value === value &&
      !this.busy &&
      this.renderGeneration === generation;
    if (id && this.options.fingerprint && this.options.fingerprintEnabled?.()) {
      const fingerprint = document.createElement("button");
      fingerprint.type = "button";
      uiText(fingerprint, "核对指纹");
      fingerprint.addEventListener("click", () => {
        if (valid()) this.options.fingerprint?.(id);
      });
      actions.append(fingerprint);
    }
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (!valid()) return;
      const fields = {
        label: inputs.get("label")?.value ?? "",
        host: inputs.get("host")?.value ?? "",
        port: Number(inputs.get("port")?.value),
        username: inputs.get("username")?.value ?? "",
      };
      try {
        if (
          parent?.value &&
          !value.machines?.some((m) => m.id === parent.value)
        )
          throw Error("hosts.invalid_machine");
        const change = validateHostChange(
          id
            ? { action: "update", id, ...fields }
            : parent?.value
              ? { action: "attach", machineId: parent.value, ...fields }
              : { action: "create", ...fields },
        );
        void this.save(change);
      } catch {
        uiText(
          this.options.status,
          "请检查名称、地址、端口和用户名；不允许控制字符或隐形方向字符",
        );
      }
    });
    if (id) {
      const remove = document.createElement("button");
      remove.type = "button";
      uiText(remove, "删除");
      let confirmed = false;
      remove.addEventListener("click", () => {
        if (!valid()) return;
        if (!confirmed) {
          confirmed = true;
          uiText(remove, "确认删除主机");
          return;
        }
        void this.save({ action: "remove", id });
      });
      actions.append(remove);
    }
    form.append(actions);
    this.options.fields.append(form);
  }
  private machineForm(
    epoch: number,
    value: HostCatalogue,
    generation: number,
    id: string,
  ): void {
    const machine = value.machines?.find((m) => m.id === id);
    if (!machine) return;
    const form = document.createElement("form");
    form.className = "workspace-host-form";
    const label = document.createElement("label");
    uiText(label, "机器名称");
    const input = document.createElement("input");
    input.value = machine.label;
    input.maxLength = 256;
    uiAttribute(input, "aria-label", "机器名称");
    label.append(input);
    const save = document.createElement("button");
    save.type = "submit";
    uiText(save, "保存机器名称");
    form.append(label, save);
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (
        !this.current(epoch) ||
        this.value !== value ||
        this.busy ||
        this.renderGeneration !== generation
      )
        return;
      try {
        void this.save(
          validateHostChange({
            action: "rename_machine",
            id,
            label: input.value,
          }),
        );
      } catch {
        uiText(this.options.status, "请检查机器名称");
      }
    });
    this.options.fields.append(form);
  }
  private async save(change: HostChange): Promise<void> {
    const value = this.value,
      nonce = this.options.nonce();
    if (!value || !nonce || this.busy || this.disposed) return;
    const epoch = this.epoch;
    const operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.lock();
    uiText(this.options.status, "正在保存主机…");
    try {
      const next = await this.dependencies.save(
        nonce,
        value.token,
        change,
        operation.signal,
      );
      if (!this.current(epoch)) return;
      this.value = next;
      this.options.changed?.(next);
      const selected =
        change.action === "create" || change.action === "attach"
          ? next.hosts.find(
              (host) => !value.hosts.some((old) => old.id === host.id),
            )?.id
          : change.action === "update"
            ? change.id
            : next.hosts[0]?.id;
      this.render(selected ?? "");
      this.options.done.focus();
      uiText(this.options.status, "已保存 · 本地终端和主机指纹信任未改变");
    } catch {
      if (this.current(epoch)) {
        this.value = undefined;
        this.options.fields.replaceChildren();
        uiText(
          this.options.status,
          "保存未确认或版本冲突 · 不自动重试，关闭后重新打开以读取实际结果",
        );
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
        this.lock();
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
