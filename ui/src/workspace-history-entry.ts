import { showWorkspaceHistory } from "./workspace-history";

export interface HistoryEntryTarget {
  readonly view: object;
  readonly session: string;
  readonly layout?: object | string | number;
}
export interface HistoryEntryOptions {
  readonly document: Document;
  readonly signal: AbortSignal;
  readonly fallbackOpener: HTMLElement;
  nonce(): string | undefined;
  available(): boolean;
  target(): HistoryEntryTarget | undefined;
}
/** UI target capture only. History IPC independently checks the original page and binding. */
export function createWorkspaceHistoryEntry(
  options: HistoryEntryOptions,
  show: typeof showWorkspaceHistory = showWorkspaceHistory,
): { target(): string; open(): void } {
  const views = new WeakMap<object, number>();
  let nextView = 0;
  return {
    target: () => {
      const target = options.target();
      if (!target) return "";
      if (!views.has(target.view)) views.set(target.view, ++nextView);
      let layout = target.layout;
      if (layout && typeof layout === "object") {
        if (!views.has(layout)) views.set(layout, ++nextView);
        layout = views.get(layout);
      }
      return JSON.stringify([
        views.get(target.view),
        target.session,
        typeof target.layout,
        layout ?? null,
      ]);
    },
    open: () => {
      const nonce = options.nonce();
      const target = options.target();
      if (!nonce || !target || options.signal.aborted || !options.available())
        return;
      const { view, session, layout } = target;
      const focused = options.document.activeElement as HTMLElement | null;
      const opener =
        focused?.isConnected && typeof focused.focus === "function"
          ? focused
          : options.fallbackOpener;
      show({
        document: options.document,
        nonce,
        session,
        signal: options.signal,
        opener,
        current: () => {
          const current = options.target();
          return (
            !options.signal.aborted &&
            options.nonce() === nonce &&
            options.available() &&
            current?.view === view &&
            current.session === session &&
            current.layout === layout
          );
        },
      });
    },
  };
}
