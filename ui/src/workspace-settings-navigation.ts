import {
  englishText,
  message,
  uiAttribute,
  uiText,
} from "./workspace-language";
import type { WorkspaceSettings } from "./workspace-settings";

export type SettingsField = keyof WorkspaceSettings["values"];
export type SettingsEntry = SettingsField | "keybindings";
const groups = {
  appearance: "外观与配色",
  font: "字体与排版",
  history: "历史与滚动",
  input: "键盘与剪贴板",
} as const;
type Group = keyof typeof groups;
// Index product labels and stable setting keys, never terminal or credential text.
const catalog = {
  ui_language: [
    "appearance",
    "界面语言 简体中文 英语 language locale English Chinese ui.language",
  ],
  ui_appearance: [
    "appearance",
    "界面外观 浅色 深色 跟随系统 theme light dark system",
  ],
  ui_accent: [
    "appearance",
    "界面强调色 青色 蓝色 绿色 琥珀色 红色 accent cyan blue green amber red",
  ],
  terminal_preset: [
    "appearance",
    "终端配色 主题 经典 午夜 纸白 PulseTerm One Dark Light Catppuccin Mocha Latte Dracula Solarized color palette theme",
  ],
  terminal_theme: [
    "appearance",
    "终端跟随系统 深色主题 浅色主题 theme system light dark palette terminal.theme",
  ],
  font_size: ["font", "字体大小 字号 font size"],
  font_family: ["font", "字体名称 预设 font family"],
  font_fallback: ["font", "后备字体 fallback"],
  font_ligatures: ["font", "运算符连字 ligatures"],
  line_height_percent: ["font", "行高 line height"],
  letter_spacing: ["font", "字距 letter spacing"],
  cursor_blink: ["appearance", "光标闪烁 cursor blink"],
  history_limit: ["history", "保留历史行数 容量 history limit scrollback"],
  scroll_on_input: ["history", "输入时回到底部 scroll input"],
  scroll_on_output: ["history", "新输出时回到底部 scroll output"],
  mac_option_as_meta: ["input", "macOS Option 作为 Meta 键 keyboard"],
  backspace_send: ["input", "Backspace 退格键 keyboard"],
  delete_send: ["input", "Delete 删除键 keyboard"],
  term_type: ["input", "TERM 终端类型 xterm vt100"],
  copy_on_select: ["input", "选中复制 clipboard copy selection"],
  right_click_paste: ["input", "右键粘贴 clipboard paste"],
  confirm_close: [
    "input",
    "关闭会话前确认 关闭确认 退出确认 confirm close session tab exit",
  ],
  keybindings: [
    "input",
    "快捷键 键位 动作 恢复内置 shortcuts keyboard keybindings keymap",
  ],
} as const satisfies Record<SettingsEntry, readonly [Group, string]>;

/** Local navigation only. Field controllers retain all read/write authority. */
export class WorkspaceSettingsNavigation {
  private readonly search: HTMLInputElement;
  private readonly category: HTMLSelectElement;
  private readonly clearButton: HTMLButtonElement;
  private readonly status: HTMLParagraphElement;
  private rows = new Map<SettingsEntry, HTMLElement>();
  private sections = new Map<Group, HTMLElement>();
  private busy = true;

  constructor(
    private readonly options: {
      root: HTMLElement;
      fields: HTMLElement;
      signal: AbortSignal;
    },
  ) {
    const { root, signal } = options;
    const doc = root.ownerDocument;
    root.className = "workspace-settings-navigation";
    root.setAttribute("role", "search");
    uiAttribute(root, "aria-label", "查找设置");
    const searchLabel = doc.createElement("label");
    uiText(searchLabel, "搜索设置");
    this.search = doc.createElement("input");
    this.search.type = "search";
    this.search.maxLength = 128;
    this.search.autocomplete = "off";
    uiAttribute(this.search, "placeholder", "名称或配置键，如 字体、font_size");
    this.search.setAttribute("aria-controls", options.fields.id);
    searchLabel.append(this.search);
    const categoryLabel = doc.createElement("label");
    uiText(categoryLabel, "分类");
    this.category = doc.createElement("select");
    this.category.setAttribute("aria-controls", options.fields.id);
    for (const [id, text] of [
      ["all", "全部设置"] as const,
      ...Object.entries(groups),
    ]) {
      const option = doc.createElement("option");
      option.value = id;
      uiText(option, text);
      this.category.append(option);
    }
    this.category.value = "all";
    categoryLabel.append(this.category);
    this.clearButton = doc.createElement("button");
    this.clearButton.type = "button";
    uiText(this.clearButton, "清除筛选");
    this.status = doc.createElement("p");
    this.status.setAttribute("role", "status");
    this.status.setAttribute("aria-live", "polite");
    this.status.setAttribute("aria-atomic", "true");
    root.replaceChildren(
      searchLabel,
      categoryLabel,
      this.clearButton,
      this.status,
    );
    this.search.addEventListener("input", () => this.filter(), { signal });
    this.category.addEventListener("change", () => this.filter(), { signal });
    this.clearButton.addEventListener(
      "click",
      () => {
        if (this.busy || signal.aborted) return;
        this.search.value = "";
        this.category.value = "all";
        this.filter();
        this.search.focus();
      },
      { signal },
    );
    this.setBusy(true);
    signal.addEventListener("abort", () => this.clear(), { once: true });
  }

  update(rows: ReadonlyMap<SettingsEntry, HTMLElement>): void {
    this.rows = new Map(rows);
    this.sections.clear();
    const doc = this.options.root.ownerDocument;
    for (const [group, title] of Object.entries(groups) as [Group, string][]) {
      const section = doc.createElement("section");
      section.className = "workspace-settings-section";
      const heading = doc.createElement("h3");
      heading.id = `${this.options.fields.id}-${group}`;
      uiText(heading, title);
      section.setAttribute("aria-labelledby", heading.id);
      section.append(heading);
      for (const [field, row] of rows) {
        if (catalog[field][0] === group) {
          row.setAttribute("data-setting", field);
          section.append(row);
        }
      }
      this.sections.set(group, section);
    }
    this.options.fields.replaceChildren(...this.sections.values());
    this.filter();
  }

  private filter(): void {
    if (this.options.signal.aborted) return;
    const terms = this.search.value
      .trim()
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean);
    const visibleGroups = new Set<Group>();
    let count = 0;
    for (const [field, row] of this.rows) {
      const [group, keywords] = catalog[field];
      const text =
        `${field} ${keywords} ${groups[group]} ${englishText(groups[group])}`.toLowerCase();
      row.hidden =
        (this.category.value !== "all" && this.category.value !== group) ||
        !terms.every((term) => text.includes(term));
      if (!row.hidden) {
        count++;
        visibleGroups.add(group);
      }
    }
    for (const [group, section] of this.sections)
      section.hidden = !visibleGroups.has(group);
    uiText(
      this.status,
      this.rows.size === 0
        ? "等待读取设置"
        : count === 0
          ? "没有匹配的设置。请更换关键词、分类，或清除筛选。"
          : message("显示 {0} / {1} 项设置", count, this.rows.size),
    );
  }

  setBusy(busy: boolean): void {
    this.busy = busy || this.options.signal.aborted || this.rows.size === 0;
    for (const control of [this.search, this.category, this.clearButton])
      control.disabled = this.busy;
  }

  focusSearch(): void {
    if (!this.busy) this.search.focus();
  }

  clear(): void {
    this.rows.clear();
    this.sections.clear();
    this.search.value = "";
    this.category.value = "all";
    this.setBusy(true);
    uiText(this.status, "等待读取设置");
  }
}
