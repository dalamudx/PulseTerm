type ImeEvent = Pick<
  Event,
  | "type"
  | "target"
  | "isTrusted"
  | "defaultPrevented"
  | "stopImmediatePropagation"
> & {
  keyCode?: number;
  inputType?: string;
};

/** WebKit may commit through key 229 without starting DOM composition. */
export class TerminalImeCommitGuard {
  private composing = false;
  private directKey = false;
  private directCommit = false;
  private target: EventTarget | null = null;
  private closed = false;
  private readonly events = [
    "keydown",
    "keyup",
    "input",
    "compositionstart",
    "compositionend",
    "blur",
  ];

  constructor(
    private readonly root: EventTarget,
    private readonly signal: AbortSignal,
    private readonly isCurrent: () => boolean,
  ) {
    if (signal.aborted) {
      this.closed = true;
      return;
    }
    for (const name of this.events)
      root.addEventListener(name, this.handle, true);
    signal.addEventListener("abort", this.dispose, { once: true });
  }

  readonly handle = (event: ImeEvent): void => {
    if (this.closed) return;
    if (!this.isCurrent()) {
      this.reset();
      return;
    }
    if (!event.isTrusted || !(event.target instanceof HTMLTextAreaElement))
      return;
    if (this.target !== event.target) this.reset();
    this.target = event.target;
    switch (event.type) {
      case "compositionstart":
        this.composing = true;
        this.directKey = this.directCommit = false;
        break;
      case "keydown":
        this.directKey =
          !this.composing && event.keyCode === 229 && !event.defaultPrevented;
        this.directCommit = false;
        break;
      case "input":
        this.directCommit =
          this.directKey &&
          !this.composing &&
          event.inputType === "insertFromComposition";
        break;
      case "compositionend":
        // The SDK's 229 textarea reader already owns this unpaired direct commit.
        if (!this.composing && this.directCommit)
          event.stopImmediatePropagation();
        this.reset();
        break;
      case "keyup":
        this.directKey = this.directCommit = false;
        break;
      case "blur":
        this.reset();
        break;
    }
  };

  private reset(): void {
    this.composing = this.directKey = this.directCommit = false;
    this.target = null;
  }

  readonly dispose = (): void => {
    if (this.closed) return;
    this.closed = true;
    this.reset();
    for (const name of this.events)
      this.root.removeEventListener(name, this.handle, true);
    this.signal.removeEventListener("abort", this.dispose);
  };
}
