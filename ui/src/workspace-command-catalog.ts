import { matchesChord, parseChordSequence } from "./keybindings/chords";
import { englishText } from "./workspace-language";

const paletteChords = Object.freeze(
  ["ctrl+shift+p", "meta+shift+p", "ctrl+meta+shift+p"].flatMap((value) =>
    parseChordSequence(value),
  ),
);
// Drawing/selection metadata only; never backend permission or a retry receipt.
export const workspacePaletteOnlyCommandIds = [
  "keybindings",
  "organization",
  "private-keys",
  "agent-identities",
  "quick-connect",
  "ssh-config",
  "split-row",
  "split-column",
  "duplicate-session",
] as const;
export const workspaceKeybindingCommandIds = [
  "profiles",
  "settings",
  "find",
  "hosts",
  "identities",
  "vault",
  "ssh",
  "close-session",
] as const;
export type WorkspaceKeybindingCommandId =
  (typeof workspaceKeybindingCommandIds)[number];
export const workspaceCommandIds = [
  ...workspaceKeybindingCommandIds,
  ...workspacePaletteOnlyCommandIds,
] as const;
export type WorkspaceCommandId = (typeof workspaceCommandIds)[number];
export interface WorkspaceCommand {
  readonly id: WorkspaceCommandId;
  readonly title: string;
  readonly keywords?: string;
  available(): boolean;
  /** Display target identity, not authorization; changing targets invalidates an old choice. */
  target(): string;
  run(): void;
}
export interface WorkspaceCommandContext {
  readonly signal: AbortSignal;
  nonce(): string | undefined;
  available(): boolean;
}
export class WorkspaceCommandCapture {
  private consumed = false;
  private readonly targets = new Map<WorkspaceCommandId, string>();
  private readonly commands: readonly WorkspaceCommand[];
  constructor(
    private readonly context: WorkspaceCommandContext,
    private readonly nonce: string,
    commands: readonly WorkspaceCommand[],
  ) {
    if (commands.length > workspaceCommandIds.length)
      throw new Error("workspace.commands_limit");
    for (const command of commands) {
      if (
        !workspaceCommandIds.includes(command.id) ||
        this.targets.has(command.id)
      )
        throw new Error("workspace.commands_invalid");
      if (
        command.title.length === 0 ||
        command.title.length > 128 ||
        (command.keywords?.length ?? 0) > 256
      )
        throw new Error("workspace.command_text_limit");
      this.targets.set(command.id, command.target());
    }
    this.commands = Object.freeze(
      commands.map((command) => Object.freeze({ ...command })),
    );
  }
  private current(): boolean {
    return (
      !this.consumed &&
      !this.context.signal.aborted &&
      this.context.nonce() === this.nonce &&
      this.context.available()
    );
  }
  list(query: string): readonly Pick<WorkspaceCommand, "id" | "title">[] {
    if (query.length > 128 || !this.current()) return [];
    const wanted = query.trim().toLowerCase();
    return this.commands
      .filter(
        (command) =>
          command.available() &&
          command.target() === this.targets.get(command.id) &&
          `${command.title} ${englishText(command.title)} ${command.keywords ?? ""}`
            .toLowerCase()
            .includes(wanted),
      )
      .map(({ id, title }) => ({ id, title }));
  }
  /** True means the existing UI action was invoked, not that native work succeeded. */
  execute(id: WorkspaceCommandId): boolean {
    if (!this.current()) return false;
    const command = this.commands.find((command) => command.id === id);
    if (!command?.available() || command.target() !== this.targets.get(id))
      return false;
    this.consumed = true; // consume before callback, including reentrant/throwing callbacks
    command.run();
    return true;
  }
  close(): void {
    this.consumed = true;
  }
}
export function captureWorkspaceCommands(
  context: WorkspaceCommandContext,
  commands: readonly WorkspaceCommand[],
): WorkspaceCommandCapture | undefined {
  const nonce = context.nonce();
  if (!nonce || context.signal.aborted || !context.available())
    return undefined;
  return new WorkspaceCommandCapture(context, nonce, commands);
}
export function isCommandPaletteShortcut(
  event: Pick<
    KeyboardEvent,
    | "key"
    | "ctrlKey"
    | "metaKey"
    | "shiftKey"
    | "altKey"
    | "repeat"
    | "isComposing"
    | "defaultPrevented"
  >,
): boolean {
  return (
    !event.repeat && paletteChords.some((chord) => matchesChord(chord, event))
  );
}
