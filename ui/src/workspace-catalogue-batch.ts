import { message, uiAttribute, uiText } from "./workspace-language";
export function validateCatalogueIds(input: unknown): readonly string[] {
  if (
    !Array.isArray(input) ||
    !input.length ||
    input.length > 256 ||
    input.some(
      (id) =>
        typeof id !== "string" ||
        !/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
          id,
        ),
    ) ||
    new Set(input).size !== input.length
  )
    throw Error("catalogue.invalid_selection");
  return Object.freeze([...input]);
}
type Row = Readonly<{ id: string; label: string; description: string }>;
/** Bounded metadata projection, never secret content or startup permission. */
export class CatalogueBatch {
  private readonly root = document.createElement("section");
  private readonly list = document.createElement("div");
  private readonly selected = new Set<string>();
  private readonly all = document.createElement("input");
  private readonly remove = document.createElement("button");
  private readonly confirm = document.createElement("div");
  private readonly search = document.createElement("input");
  private busy = false;
  private rowsGeneration = 0;
  private confirmationGeneration = 0;
  private visible: readonly Row[];
  constructor(
    private readonly options: {
      container: HTMLElement;
      rows: readonly Row[];
      kind: string;
      consequence: string;
      current(): boolean;
      edit(id: string): void;
      remove(ids: readonly string[]): void;
      filter?(): void;
    },
  ) {
    this.visible = options.rows;
    this.root.className = "workspace-catalogue-batch";
    const summary = document.createElement("h3");
    uiText(summary, "搜索与批量管理");
    const label = document.createElement("label");
    uiText(label, message("搜索{0}", message(options.kind)));
    this.search.type = "search";
    this.search.maxLength = 256;
    uiAttribute(
      this.search,
      "aria-label",
      message("搜索{0}", message(options.kind)),
    );
    this.search.autocomplete = "off";
    this.search.addEventListener("input", () => {
      if (!this.valid()) return;
      options.filter?.();
      const query = this.search.value.toLocaleLowerCase();
      this.visible = options.rows.filter((row) =>
        `${row.label} ${row.description}`.toLocaleLowerCase().includes(query),
      );
      this.selected.clear();
      this.clearConfirmation();
      this.renderRows();
      this.refresh();
    });
    label.append(this.search);
    const allLabel = document.createElement("label");
    allLabel.className = "workspace-catalogue-choice";
    uiText(allLabel, "选择当前搜索结果");
    this.all.type = "checkbox";
    uiAttribute(this.all, "aria-label", "选择当前搜索结果");
    this.all.addEventListener("change", () => {
      if (!this.valid()) return;
      this.selected.clear();
      if (this.all.checked)
        for (const row of this.visible) this.selected.add(row.id);
      this.clearConfirmation();
      this.renderRows();
      this.refresh();
    });
    allLabel.append(this.all);
    this.remove.type = "button";
    this.remove.addEventListener("click", () => {
      if (this.valid() && this.selected.size) this.showConfirmation();
    });
    this.confirm.hidden = true;
    this.root.append(
      summary,
      label,
      allLabel,
      this.list,
      this.remove,
      this.confirm,
    );
    options.container.append(this.root);
    this.renderRows();
    this.refresh();
  }
  setBusy(busy: boolean): void {
    this.busy = busy;
    this.refresh();
  }
  private valid(): boolean {
    return !this.busy && this.options.current();
  }
  private clearConfirmation(): void {
    this.confirmationGeneration++;
    this.confirm.hidden = true;
    this.confirm.replaceChildren();
  }
  private renderRows(): void {
    const generation = ++this.rowsGeneration;
    this.list.replaceChildren();
    if (!this.visible.length) {
      const empty = document.createElement("p");
      uiText(empty, "没有匹配项");
      this.list.append(empty);
    }
    for (const row of this.visible) {
      const line = document.createElement("div"),
        label = document.createElement("label"),
        check = document.createElement("input"),
        edit = document.createElement("button");
      line.className = "workspace-catalogue-row";
      label.className = "workspace-catalogue-choice";
      label.textContent = `${row.label} · ${row.description}`;
      check.type = "checkbox";
      check.checked = this.selected.has(row.id);
      uiAttribute(
        check,
        "aria-label",
        message("选择 {0} · {1}", row.label, row.description),
      );
      check.addEventListener("change", () => {
        if (!this.valid() || generation !== this.rowsGeneration) return;
        if (check.checked) this.selected.add(row.id);
        else this.selected.delete(row.id);
        this.clearConfirmation();
        this.refresh();
      });
      label.append(check);
      edit.type = "button";
      uiText(edit, "编辑");
      uiAttribute(
        edit,
        "aria-label",
        message("编辑 {0} · {1}", row.label, row.description),
      );
      edit.addEventListener("click", () => {
        if (this.valid() && generation === this.rowsGeneration)
          this.options.edit(row.id);
      });
      line.append(label, edit);
      this.list.append(line);
    }
  }
  private refresh(): void {
    for (const control of this.root.querySelectorAll<
      HTMLInputElement | HTMLButtonElement
    >("input,button"))
      control.disabled = this.busy;
    this.all.disabled = this.busy || !this.visible.length;
    this.all.checked =
      !!this.visible.length &&
      this.visible.every((row) => this.selected.has(row.id));
    this.all.indeterminate = this.selected.size > 0 && !this.all.checked;
    uiText(this.remove, message("删除所选（{0}）", this.selected.size));
    this.remove.disabled = this.busy || !this.selected.size;
  }
  private showConfirmation(): void {
    const ids = validateCatalogueIds([...this.selected]);
    this.clearConfirmation();
    const generation = this.confirmationGeneration;
    this.confirm.hidden = false;
    const text = document.createElement("p"),
      names = document.createElement("ul"),
      approve = document.createElement("button"),
      cancel = document.createElement("button");
    uiText(
      text,
      message(
        "删除 {0} 个{1}？{2}",
        ids.length,
        message(this.options.kind),
        message(this.options.consequence),
      ),
    );
    for (const row of this.options.rows.filter((row) =>
      this.selected.has(row.id),
    )) {
      const name = document.createElement("li");
      name.textContent = `${row.label} · ${row.description}`;
      names.append(name);
    }
    approve.type = cancel.type = "button";
    uiText(approve, "确认删除所选");
    uiText(cancel, "取消批量删除");
    let submitted = false;
    approve.addEventListener("click", () => {
      if (
        this.valid() &&
        !submitted &&
        !this.confirm.hidden &&
        generation === this.confirmationGeneration
      ) {
        submitted = true;
        this.options.remove(ids);
      }
    });
    cancel.addEventListener("click", () => {
      if (this.valid() && generation === this.confirmationGeneration) {
        this.clearConfirmation();
        this.remove.focus();
      }
    });
    this.confirm.append(text, names, cancel, approve);
    cancel.focus();
  }
}
