import { message as uiMessage, uiText } from "./workspace-language";
import {
  filterOrganization,
  groupPath,
  type Organization,
  type OrganizationChange,
  type OrganizationItem,
  requestOrganization,
} from "./workspace-organization";
import {
  type BehaviorApi,
  OrganizationBehaviorEditor,
} from "./workspace-organization-behavior";
export class WorkspaceOrganizationEditor {
  private value: Organization | undefined;
  private epoch = 0;
  private generation = 0;
  private busy = false;
  private disposed = false;
  private operation: AbortController | undefined;
  private selectedGroup = "";
  private selectedItem = "";
  private query = "";
  private filterGroup = "";
  private filterTag = "";
  private favorites = false;
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      fields: HTMLElement;
      status: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(busy: boolean): void;
      restoreFocus(): void;
      changed?(value: Organization): void;
    },
    private readonly request = requestOrganization,
    private readonly behaviorInvoke?: BehaviorApi,
  ) {
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      { signal: options.signal },
    );
    options.signal.addEventListener("abort", () => this.dispose(), {
      once: true,
    });
    if (options.signal.aborted) this.dispose();
  }
  async open(): Promise<void> {
    if (this.disposed || this.busy || this.options.dialog.open) return;
    this.options.dialog.showModal();
    await this.load();
  }
  close(): void {
    this.epoch++;
    this.operation?.abort();
    this.value = undefined;
    this.busy = false;
    this.options.fields.replaceChildren();
    this.options.working(false);
    if (this.options.dialog.open) this.options.dialog.close();
    if (!this.disposed) this.options.restoreFocus();
  }
  dispose(): void {
    this.disposed = true;
    this.close();
  }
  private current(epoch: number): boolean {
    return (
      !this.disposed &&
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      this.epoch === epoch
    );
  }
  private async load(change?: OrganizationChange): Promise<void> {
    const nonce = this.options.nonce();
    if (!nonce || this.busy) return;
    const old = this.value;
    if (change && !old) return;
    const epoch = ++this.epoch;
    const operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    for (const control of this.options.fields.querySelectorAll<HTMLInputElement>(
      "input,button,select,textarea",
    ))
      control.disabled = true;
    uiText(
      this.options.status,
      change ? "正在保存组织信息…" : "正在读取分组与标签…",
    );
    try {
      const value = await this.request(
        nonce,
        operation.signal,
        change && old ? { token: old.token, change } : undefined,
      );
      if (!this.current(epoch)) return;
      this.value = value;
      this.options.changed?.(value);
      this.render();
      uiText(
        this.options.status,
        change ? "组织信息已保存" : "仅管理分组、标签与收藏",
      );
    } catch {
      if (this.current(epoch)) {
        this.value = undefined;
        this.options.fields.replaceChildren();
        const retry = document.createElement("button");
        retry.type = "button";
        uiText(retry, "重新读取组织信息");
        retry.addEventListener("click", () => {
          if (this.current(epoch)) void this.load();
        });
        this.options.fields.append(retry);
        uiText(this.options.status, "保存或读取失败，请重新读取后再操作");
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
      }
    }
  }
  private behavior(kind: "group" | "host", id: string, name: string): void {
    if (this.busy || this.disposed || !this.options.dialog.open) return;
    ++this.epoch;
    this.operation?.abort();
    this.operation = new AbortController();
    this.value = undefined;
    this.options.fields.replaceChildren();
    uiText(this.options.status, "");
    new OrganizationBehaviorEditor({
      root: this.options.fields,
      invoke: this.behaviorInvoke,
      nonce: this.options.nonce,
      target: { kind, id },
      name,
      signal: this.operation.signal,
      working: (busy) => {
        this.busy = busy;
        this.options.working(busy);
      },
      back: () => void this.load(),
    });
  }
  private render(): void {
    const value = this.value;
    if (!value) return;
    const epoch = this.epoch,
      generation = ++this.generation;
    const current = () =>
      this.current(epoch) &&
      this.value === value &&
      !this.busy &&
      this.generation === generation;
    this.options.fields.replaceChildren();
    const field = (label: string, input: HTMLElement, root: HTMLElement) => {
      const wrapper = document.createElement("label");
      uiText(wrapper, label);
      wrapper.append(input);
      root.append(wrapper);
      return input;
    };
    const button = (text: string, action: () => void, root: HTMLElement) => {
      const b = document.createElement("button");
      b.type = "button";
      uiText(b, text);
      b.addEventListener("click", () => {
        if (current()) action();
      });
      root.append(b);
      return b;
    };
    const input = () => {
      const i = document.createElement("input");
      i.type = "text";
      i.autocomplete = "off";
      i.spellcheck = false;
      i.maxLength = 128;
      return i;
    };
    const groupSelect = (
      initial: string,
      label: string,
      root: HTMLElement,
      empty: string,
    ) => {
      const select = document.createElement("select");
      const option = document.createElement("option");
      option.value = "";
      uiText(option, empty);
      select.append(option);
      const groups = value.groups
        .filter((g) => !g.metadata.deleted)
        .map((g) => ({
          id: g.metadata.id,
          path: groupPath(value.groups, g.metadata.id),
        }))
        .sort((a, b) => a.path.localeCompare(b.path));
      for (const g of groups) {
        const o = document.createElement("option");
        o.value = g.id;
        o.textContent = g.path;
        select.append(o);
      }
      select.value = initial;
      if (select.selectedIndex < 0) select.value = "";
      field(label, select, root);
      return select;
    };
    const groupForm = document.createElement("form");
    groupForm.className = "workspace-host-form";
    const heading = document.createElement("h3");
    uiText(heading, "分组");
    groupForm.append(heading);
    const selected = groupSelect(
      this.selectedGroup,
      "编辑分组",
      groupForm,
      "新建分组",
    );
    this.selectedGroup = selected.value;
    selected.addEventListener("change", () => {
      if (current()) {
        this.selectedGroup = selected.value;
        this.render();
      }
    });
    const existing = value.groups.find(
      (g) => g.metadata.id === this.selectedGroup && !g.metadata.deleted,
    );
    const name = input();
    name.value = existing?.name ?? "";
    name.required = true;
    field("分组名称", name, groupForm);
    const parent = groupSelect(
      existing?.parent ?? "",
      "父分组",
      groupForm,
      "根分组",
    );
    for (const option of Array.from(parent.options)) {
      if (
        option.value &&
        existing &&
        (option.value === existing.metadata.id ||
          groupPath(value.groups, option.value).startsWith(
            `${groupPath(value.groups, existing.metadata.id)} / `,
          ))
      )
        option.disabled = true;
    }
    const saveGroup = document.createElement("button");
    saveGroup.type = "submit";
    uiText(saveGroup, existing ? "保存分组" : "创建分组");
    groupForm.append(saveGroup);
    if (existing)
      button(
        "终端配色与行为",
        () => {
          if (current())
            this.behavior("group", existing.metadata.id, existing.name);
        },
        groupForm,
      );
    groupForm.addEventListener("submit", (event) => {
      event.preventDefault();
      if (!current()) return;
      if (selected.value !== (existing?.metadata.id ?? "")) {
        this.selectedGroup = selected.value;
        this.render();
        return;
      }
      void this.load(
        existing
          ? {
              action: "update_group",
              id: existing.metadata.id,
              name: name.value,
              parent: parent.value || null,
            }
          : {
              action: "create_group",
              name: name.value,
              parent: parent.value || null,
            },
      );
    });
    if (existing)
      button(
        "删除分组",
        () => {
          const confirm = document.createElement("div");
          const note = document.createElement("p");
          uiText(
            note,
            uiMessage(
              "删除分组“{0}”？成员将移至未分组；含子分组时请先整理子分组。",
              groupPath(value.groups, existing.metadata.id),
            ),
          );
          confirm.append(note);
          let active = true;
          button(
            "确认删除分组",
            () => {
              if (active && selected.value === existing.metadata.id) {
                active = false;
                void this.load({
                  action: "remove_group",
                  id: existing.metadata.id,
                });
              }
            },
            confirm,
          );
          button(
            "取消删除分组",
            () => {
              active = false;
              confirm.remove();
            },
            confirm,
          );
          groupForm.append(confirm);
        },
        groupForm,
      );
    this.options.fields.append(groupForm);
    const list = document.createElement("section");
    list.className = "workspace-catalogue-batch";
    const title = document.createElement("h3");
    uiText(title, "主机与身份");
    list.append(title);
    const search = input();
    search.type = "search";
    search.value = this.query;
    field("搜索名称、用户名或标签", search, list);
    const filter = groupSelect(this.filterGroup, "筛选分组", list, "全部分组");
    const ungrouped = document.createElement("option");
    ungrouped.value = "ungrouped";
    uiText(ungrouped, "未分组");
    filter.append(ungrouped);
    filter.value = this.filterGroup;
    const tag = document.createElement("select");
    const all = document.createElement("option");
    all.value = "";
    uiText(all, "全部标签");
    tag.append(all);
    for (const row of value.tags.filter((r) => !r.metadata.deleted)) {
      const o = document.createElement("option");
      o.value = row.metadata.id;
      o.textContent = row.name;
      tag.append(o);
    }
    tag.value = this.filterTag;
    field("筛选标签", tag, list);
    const favorites = document.createElement("input");
    favorites.type = "checkbox";
    favorites.checked = this.favorites;
    field("仅显示收藏", favorites, list);
    const itemSelect = document.createElement("select");
    field("选择要整理的项目", itemSelect, list);
    const editor = document.createElement("div");
    list.append(editor);
    this.options.fields.append(list);
    let itemGeneration = 0;
    const showItem = () => {
      const item = value.items.find(
        (i) => `${i.kind}:${i.id}` === itemSelect.value,
      );
      editor.replaceChildren();
      const itemEpoch = ++itemGeneration;
      this.selectedItem = itemSelect.value;
      if (!item) return;
      const member = value.memberships.find(
        (m) =>
          m.kind === item.kind && m.item === item.id && !m.metadata.deleted,
      );
      const assignment = document.createElement("form");
      assignment.className = "workspace-host-form";
      const target = groupSelect(
        member?.group ?? "",
        "项目分组",
        assignment,
        "未分组",
      );
      const tags = document.createElement("textarea");
      tags.rows = 3;
      tags.maxLength = 2048;
      tags.value = (member?.tags ?? [])
        .map((id) => value.tags.find((t) => t.metadata.id === id)?.name ?? "")
        .join("\n");
      field("标签（每行一个，最多16个）", tags, assignment);
      const favorite = document.createElement("input");
      favorite.type = "checkbox";
      favorite.checked = member?.favorite ?? false;
      field("收藏此项目", favorite, assignment);
      const save = document.createElement("button");
      save.type = "submit";
      uiText(save, "保存项目组织");
      assignment.append(save);
      if (item.kind === "host")
        button(
          "终端配色与行为",
          () => {
            if (current() && itemEpoch === itemGeneration)
              this.behavior("host", item.id, item.label);
          },
          assignment,
        );
      assignment.addEventListener("submit", (event) => {
        event.preventDefault();
        if (!current() || itemGeneration !== itemEpoch) return;
        if (itemSelect.value !== `${item.kind}:${item.id}`) {
          showItem();
          return;
        }
        void this.load({
          action: "assign",
          kind: item.kind,
          item: item.id,
          group: target.value || null,
          tags: tags.value.split("\n").filter((t) => t !== ""),
          favorite: favorite.checked,
        });
      });
      editor.append(assignment);
    };
    const update = () => {
      const rows = filterOrganization(
        value,
        this.query,
        this.filterGroup,
        this.filterTag,
        this.favorites,
      );
      itemSelect.replaceChildren();
      for (const item of rows) {
        const option = document.createElement("option");
        option.value = `${item.kind}:${item.id}`;
        uiText(
          option,
          uiMessage(
            "{0} · {1} · {2}",
            uiMessage(this.kindLabel(item)),
            item.label,
            item.description,
          ),
        );
        itemSelect.append(option);
      }
      itemSelect.value = this.selectedItem;
      if (itemSelect.selectedIndex < 0 && rows.length)
        itemSelect.selectedIndex = 0;
      itemSelect.disabled = rows.length === 0;
      if (!rows.length) {
        const empty = document.createElement("p");
        uiText(empty, "没有匹配的项目");
        editor.replaceChildren(empty);
        itemGeneration++;
      } else showItem();
    };
    itemSelect.addEventListener("change", () => {
      if (current()) showItem();
    });
    search.addEventListener("input", () => {
      if (current()) {
        this.query = search.value;
        update();
      }
    });
    filter.addEventListener("change", () => {
      if (current()) {
        this.filterGroup = filter.value;
        update();
      }
    });
    tag.addEventListener("change", () => {
      if (current()) {
        this.filterTag = tag.value;
        update();
      }
    });
    favorites.addEventListener("change", () => {
      if (current()) {
        this.favorites = favorites.checked;
        update();
      }
    });
    update();
  }
  private kindLabel(item: OrganizationItem): string {
    return item.kind === "host"
      ? "SSH 主机"
      : item.kind === "password_identity"
        ? "密码身份"
        : "私钥身份";
  }
}
