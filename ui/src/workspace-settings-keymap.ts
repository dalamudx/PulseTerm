import type { WorkspaceKeymapEditor } from "./workspace-keymap-editor";
import type { WorkspaceSettingsEditor } from "./workspace-settings-editor";

/** UI navigation only; each editor keeps its own source and save authorization. */
export function settingsKeymapLink(options: {
  settings: Pick<WorkspaceSettingsEditor, "suspend">;
  keymap: Pick<WorkspaceKeymapEditor, "open">;
  dialog: HTMLDialogElement;
  signal: AbortSignal;
  failed(): void;
}): () => Promise<void> {
  let returnToSettings: (() => void) | undefined;
  options.dialog.addEventListener(
    "close",
    () => {
      const resume = returnToSettings;
      returnToSettings = undefined;
      resume?.();
    },
    { signal: options.signal },
  );
  options.signal.addEventListener(
    "abort",
    () => {
      returnToSettings = undefined;
    },
    { once: true },
  );
  return async () => {
    if (options.signal.aborted || returnToSettings) return;
    const resume = options.settings.suspend();
    if (!resume) return;
    returnToSettings = resume;
    try {
      await options.keymap.open();
    } catch {
      if (!options.signal.aborted) options.failed();
    } finally {
      if (!options.dialog.open && returnToSettings === resume) {
        returnToSettings = undefined;
        resume();
      }
    }
  };
}
