import { CompositionGuard } from "../../keybindings/composition";
import { message, uiAttribute, uiText } from "../../workspace-language";
import { reviewableFilePath } from "./terminal-file-links";
import { reviewableLink } from "./terminal-links";

export function createLinkReview(
  element: HTMLElement,
  signal: AbortSignal,
  isCurrent: () => boolean,
  focus: () => void,
  doc: Document = element.ownerDocument,
  openExternal?: (url: string) => Promise<void>,
) {
  let dialog: HTMLDialogElement | undefined;
  let generation = 0;
  let activeComposition: CompositionGuard | undefined;
  const valid = () => !signal.aborted && isCurrent() && element.isConnected;
  const dismiss = () => {
    generation++;
    activeComposition?.dispose();
    activeComposition = undefined;
    if (!dialog) return;
    const previous = dialog;
    dialog = undefined;
    previous.close();
    previous.remove();
    if (valid()) focus();
  };
  signal.addEventListener("abort", dismiss, { once: true });
  return {
    open(raw: string, fileSource = false) {
      if (!valid() || doc.hidden) return;
      dismiss();
      const current = ++generation;
      const target = fileSource ? undefined : reviewableLink(raw);
      const file = fileSource ? reviewableFilePath(raw) : undefined;
      const copyValue = file?.path ?? target?.href;
      const modal = doc.createElement("dialog");
      dialog = modal;
      modal.id = "terminal-link-review";
      modal.className = "terminal-paste-dialog terminal-link-review";
      uiAttribute(
        modal,
        "aria-label",
        file ? "检查终端文件路径" : "检查终端链接",
      );
      const heading = doc.createElement("h2");
      uiText(heading, file ? "检查终端文件路径" : "检查终端链接");
      const text = doc.createElement("p");
      uiText(
        text,
        file
          ? "路径来自终端输出，可能位于远程主机。包围路径的引号不计入复制内容；变量和反斜线按字面保留。请核对后复制，此操作不会读取本机文件或调用编辑器。"
          : "链接来自终端输出。IP:端口按 HTTP 网页地址解释，端口不代表服务一定支持 HTTP。请核对完整目标，再复制地址或交给系统默认浏览器处理。",
      );
      const address = doc.createElement("textarea");
      address.className = "terminal-link-target";
      address.hidden = !copyValue;
      address.readOnly = true;
      address.rows = 4;
      uiAttribute(
        address,
        "aria-label",
        file ? "实际文件路径" : "实际目标地址",
      );
      address.value = copyValue ?? "";
      const authority = doc.createElement("p");
      uiText(
        authority,
        file
          ? message(
              "{0}{1}{2}",
              message(
                file.style === "windows" ? "Windows 盘符路径" : "Unix 绝对路径",
              ),
              file.line === undefined ? "" : message(" · 行 {0}", file.line),
              file.column === undefined
                ? ""
                : message(" · 列 {0}", file.column),
            )
          : target
            ? message(
                "主机：{0} · 协议：{1}",
                target.host,
                target.protocol.slice(0, -1).toUpperCase(),
              )
            : "此目标不可用：仅接受不含凭据的 HTTP／HTTPS 地址。",
      );
      const status = doc.createElement("p");
      status.setAttribute("role", "status");
      const controls = doc.createElement("div");
      controls.className = "terminal-paste-actions";
      const copy = doc.createElement("button");
      copy.type = "button";
      uiText(copy, file ? "复制路径" : "复制地址");
      copy.disabled = !copyValue;
      const open = doc.createElement("button");
      open.type = "button";
      open.className = "terminal-paste-accept";
      uiText(open, "在系统浏览器打开");
      open.disabled = !target || !openExternal;
      open.hidden = !!file;
      if (file)
        uiText(
          status,
          "行列位置单独显示，复制内容仅含路径。尚不支持外部编辑器打开。",
        );
      else if (target && !openExternal)
        uiText(status, "此视图暂不支持系统打开，可复制地址。");
      let copying = false;
      let opening = false;
      let opened = false;
      const active = () =>
        valid() && generation === current && dialog === modal;
      copy.addEventListener("click", async (event) => {
        if (!event.isTrusted || !copyValue || copying || opening || !active())
          return;
        copying = true;
        copy.disabled = true;
        try {
          await doc.defaultView?.navigator.clipboard.writeText(copyValue);
          if (active()) uiText(status, file ? "路径已复制。" : "地址已复制。");
        } catch {
          if (active())
            uiText(status, "复制未完成，可选中地址后使用系统复制。");
        } finally {
          copying = false;
          if (active()) copy.disabled = false;
        }
      });
      open.addEventListener("click", async (event) => {
        if (
          !event.isTrusted ||
          !target ||
          !openExternal ||
          copying ||
          opening ||
          opened ||
          !active()
        )
          return;
        opening = true;
        open.disabled = true;
        copy.disabled = true;
        uiText(status, "正在请求系统浏览器…");
        try {
          await openExternal(target.href);
          opened = true;
          if (active()) uiText(status, "已交给系统默认浏览器处理。");
        } catch (error) {
          const code = error instanceof Error ? error.message : error;
          if (active())
            uiText(
              status,
              code === "workspace.link_unavailable"
                ? "当前平台尚不支持系统打开，可复制地址。"
                : "未能交给系统浏览器，可重试或复制地址。",
            );
        } finally {
          opening = false;
          if (active()) {
            open.disabled = opened;
            copy.disabled = false;
          }
        }
      });
      const close = doc.createElement("button");
      close.type = "button";
      uiText(close, "关闭");
      close.addEventListener("click", dismiss);
      const composition = new CompositionGuard(modal, signal);
      activeComposition = composition;
      modal.addEventListener("keydown", (event) => {
        if (composition.handles(event)) {
          event.stopPropagation();
          if (event.key === "Escape") event.preventDefault();
          return;
        }
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          dismiss();
        }
      });
      modal.addEventListener("cancel", (event) => {
        event.preventDefault();
        if (!composition.handles({ key: "Escape" })) dismiss();
      });
      modal.addEventListener("close", () => composition.dispose());
      controls.append(copy, open, close);
      modal.append(heading, text);
      if (copyValue) modal.append(address);
      modal.append(authority, status, controls);
      doc.body.append(modal);
      modal.showModal();
      address.focus();
      address.select();
    },
    dispose() {
      dismiss();
      signal.removeEventListener("abort", dismiss);
    },
  };
}
