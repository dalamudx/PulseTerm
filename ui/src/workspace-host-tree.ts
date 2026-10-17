import type { HostCatalogue } from "./workspace-hosts";
import { message, uiAttribute, uiNode, uiText } from "./workspace-language";
import {
  filterOrganization,
  groupPath,
  type Organization,
} from "./workspace-organization";
import type { RecentHost } from "./workspace-recent-hosts";

/** Drawing metadata only; selecting an item opens the existing saved-target form. */
export class WorkspaceHostTree {
  private value: Organization | undefined;
  private generation = 0;
  private mode: "tree" | "list" | "recent" = "tree";
  private recent: readonly RecentHost[] | undefined;
  private recentFailed = false;
  private query = "";
  private favorites = false;
  private readonly collapsed = new Set<string>();
  constructor(
    private readonly options: {
      root: HTMLElement;
      signal: AbortSignal;
      select(id: string): void;
      enabled(): boolean;
    },
  ) {
    options.signal.addEventListener(
      "abort",
      () => {
        ++this.generation;
        options.root.replaceChildren();
      },
      { once: true },
    );
  }
  update(value: Organization): void {
    if (this.options.signal.aborted) return;
    this.value = value;
    this.render();
  }
  updateRecent(value: readonly RecentHost[] | undefined): void {
    if (this.options.signal.aborted) return;
    this.recent = value;
    this.recentFailed = value === undefined;
    this.render();
  }
  updateHosts(
    value: Pick<HostCatalogue, "hosts" | "machines" | "migrated">,
  ): void {
    if (!this.value) return;
    this.update({
      ...this.value,
      hostGraph: {
        version: 1,
        migrated: value.migrated === true,
        machines: value.machines ?? [],
        entrances: value.hosts.flatMap((h) =>
          h.machineId ? [{ id: h.id, machineId: h.machineId }] : [],
        ),
      },
      items: [
        ...this.value.items.filter((i) => i.kind !== "host"),
        ...value.hosts.map((h) => ({
          kind: "host" as const,
          id: h.id,
          label: h.label,
          description: `${h.username}@${h.host}:${h.port}`,
        })),
      ],
    });
  }
  updateEnabled(): void {
    for (const button of this.options.root.querySelectorAll<HTMLButtonElement>(
      ".workspace-host-tree-item",
    ))
      button.disabled = this.options.signal.aborted || !this.options.enabled();
  }
  failed(): void {
    this.value = undefined;
    ++this.generation;
    const failure = this.options.root.ownerDocument.createElement("p");
    uiText(failure, "主机目录读取失败，请重新加载工作区。");
    this.options.root.replaceChildren(failure);
  }
  private render(): void {
    const data = this.value;
    if (!data) return;
    const generation = ++this.generation;
    const current = () =>
      !this.options.signal.aborted && this.generation === generation;
    const doc = this.options.root.ownerDocument;
    this.options.root.replaceChildren();
    const toolbar = doc.createElement("div");
    toolbar.className = "workspace-host-tree-tools";
    const modes: HTMLButtonElement[] = [];
    for (const [mode, label, short] of [
      ["tree", "树视图", "分组"],
      ["list", "列表视图", "列表"],
      ["recent", "最近连接", "最近"],
    ] as const) {
      const button = doc.createElement("button");
      button.type = "button";
      uiText(button, short);
      uiAttribute(button, "aria-label", label);
      uiAttribute(button, "title", label);
      button.setAttribute("aria-pressed", String(this.mode === mode));
      button.addEventListener("click", () => {
        if (current()) {
          this.mode = mode;
          draw();
        }
      });
      toolbar.append(button);
      modes.push(button);
    }
    const search = doc.createElement("input");
    search.type = "search";
    search.value = this.query;
    uiAttribute(search, "placeholder", "搜索主机或标签");
    uiAttribute(search, "aria-label", "搜索主机树");
    const favoriteLabel = doc.createElement("label");
    const favorite = doc.createElement("input");
    favorite.type = "checkbox";
    favorite.checked = this.favorites;
    favoriteLabel.append(favorite, uiNode(doc, "仅收藏主机"));
    const result = doc.createElement("div");
    result.className = "workspace-host-tree-result";
    const showEmpty = (copy: string) => {
      const notice = doc.createElement("p");
      uiText(notice, copy);
      result.append(notice);
    };
    this.options.root.append(toolbar, search, favoriteLabel, result);
    const draw = () => {
      if (!current()) return;
      modes.forEach((button, index) => {
        button.setAttribute(
          "aria-pressed",
          String(this.mode === (["tree", "list", "recent"] as const)[index]),
        );
      });
      result.replaceChildren();
      if (this.mode === "recent" && !this.recent) {
        showEmpty(
          this.recentFailed
            ? "最近连接读取失败，请重新加载工作区。"
            : "正在读取最近连接…",
        );
        return;
      }
      let matches = filterOrganization(
        data,
        this.query,
        "",
        "",
        this.favorites,
      ).filter((i) => i.kind === "host");
      if (this.mode === "recent") {
        const byId = new Map(matches.map((host) => [host.id, host]));
        matches = (this.recent ?? []).flatMap((row) => {
          const host = byId.get(row.host);
          return host ? [host] : [];
        });
      }
      if (!matches.length) {
        showEmpty(
          this.mode === "recent"
            ? this.query || this.favorites
              ? "没有匹配的最近连接"
              : "尚无成功的 SSH 连接"
            : "没有匹配的主机",
        );
        return;
      }
      const memberships = new Map(
        data.memberships
          .filter((m) => m.kind === "host" && !m.metadata.deleted)
          .map((m) => [m.item, m]),
      );
      const membership = (id: string) => memberships.get(id);
      const addHost = (host: (typeof matches)[number], root: HTMLElement) => {
        const button = doc.createElement("button");
        button.type = "button";
        button.className = "workspace-host-tree-item";
        button.disabled = !this.options.enabled();
        uiAttribute(
          button,
          "aria-label",
          message("选择主机 {0} · {1}", host.label, host.description),
        );
        const title = doc.createElement("span");
        title.textContent = `${membership(host.id)?.favorite ? "★ " : ""}${host.label}`;
        const description = doc.createElement("small");
        description.textContent = host.description;
        button.append(title, description);
        if (this.mode === "recent") {
          const entry = this.recent?.find((row) => row.host === host.id);
          if (entry) {
            const time = doc.createElement("small");
            uiText(
              time,
              message(
                "最近连接：{0}",
                new Date(entry.connectedAt).toLocaleString(),
              ),
            );
            button.append(time);
          }
        }
        button.addEventListener("click", () => {
          if (current() && this.options.enabled()) this.options.select(host.id);
        });
        const machineId = data.hostGraph?.entrances.find(
          (e) => e.id === host.id,
        )?.machineId;
        const machine = data.hostGraph?.machines.find(
          (m) => m.id === machineId,
        );
        if (this.mode === "tree" && machine) {
          let folder = Array.from(root.children).find(
            (node) =>
              node instanceof HTMLDetailsElement &&
              node.dataset.machine === machine.id,
          ) as HTMLDetailsElement | undefined;
          if (!folder) {
            folder = doc.createElement("details");
            folder.dataset.machine = machine.id;
            const key = `machine:${machine.id}`;
            folder.open = !!this.query || !this.collapsed.has(key);
            const summary = doc.createElement("summary");
            summary.textContent = machine.label;
            folder.append(summary);
            const ownFolder = folder;
            folder.addEventListener("toggle", () => {
              if (current() && !this.query) {
                if (ownFolder.open) this.collapsed.delete(key);
                else this.collapsed.add(key);
              }
            });
            root.append(folder);
          }
          folder.append(button);
        } else root.append(button);
      };
      if (this.mode !== "tree") {
        for (const host of matches) addHost(host, result);
        return;
      }
      const groups = data.groups
        .filter((g) => !g.metadata.deleted)
        .sort((a, b) =>
          groupPath(data.groups, a.metadata.id).localeCompare(
            groupPath(data.groups, b.metadata.id),
          ),
        );
      const byId = new Map(groups.map((g) => [g.metadata.id, g]));
      const visible = new Set<string>();
      for (const host of matches) {
        let at = membership(host.id)?.group;
        for (let depth = 0; at && depth < 8; depth++) {
          visible.add(at);
          at = byId.get(at)?.parent;
        }
      }
      const addGroup = (parent: string | null, root: HTMLElement) => {
        for (const group of groups.filter(
          (g) => g.parent === parent && visible.has(g.metadata.id),
        )) {
          const folder = doc.createElement("details");
          folder.open = !!this.query || !this.collapsed.has(group.metadata.id);
          const summary = doc.createElement("summary");
          summary.textContent = group.name;
          folder.append(summary);
          folder.addEventListener("toggle", () => {
            if (current() && !this.query) {
              if (folder.open) this.collapsed.delete(group.metadata.id);
              else this.collapsed.add(group.metadata.id);
            }
          });
          for (const host of matches.filter(
            (h) => membership(h.id)?.group === group.metadata.id,
          ))
            addHost(host, folder);
          addGroup(group.metadata.id, folder);
          root.append(folder);
        }
      };
      addGroup(null, result);
      const loose = matches.filter((h) => !membership(h.id)?.group);
      if (loose.length) {
        const label = doc.createElement("p");
        uiText(label, "未分组");
        result.append(label);
        for (const host of loose) addHost(host, result);
      }
    };
    search.addEventListener("input", () => {
      if (current()) {
        this.query = search.value;
        draw();
      }
    });
    favorite.addEventListener("change", () => {
      if (current()) {
        this.favorites = favorite.checked;
        draw();
      }
    });
    draw();
  }
}
