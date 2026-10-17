import { isWorkspaceNonce } from "./workspace-readiness";
import { decodeWorkspaceSessions } from "./workspace-sessions";

export interface CliSelectionRequest {
  readonly nonce: string;
  readonly ticket: string;
  readonly session: string;
}
export function decodeCliSelection(value: unknown): CliSelectionRequest {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("workspace.cli_selection_invalid");
  const body = value as Record<string, unknown>;
  if (
    Object.keys(body).length !== 4 ||
    body.version !== 1 ||
    typeof body.nonce !== "string" ||
    !isWorkspaceNonce(body.nonce) ||
    body.nonce !== body.nonce.toLowerCase() ||
    typeof body.ticket !== "string" ||
    !isWorkspaceNonce(body.ticket) ||
    body.ticket !== body.ticket.toLowerCase()
  )
    throw Error("workspace.cli_selection_invalid");
  const session = decodeWorkspaceSessions([body.session])[0];
  if (!session) throw Error("workspace.cli_selection_invalid");
  return Object.freeze({ nonce: body.nonce, ticket: body.ticket, session });
}
interface Options {
  signal: AbortSignal;
  current: () => string | undefined;
  available: () => boolean;
  admit: (request: CliSelectionRequest) => Promise<void>;
  select: (
    request: CliSelectionRequest,
    signal: AbortSignal,
  ) => Promise<boolean>;
  complete: (request: CliSelectionRequest, selected: boolean) => Promise<void>;
}
interface Pending {
  request: CliSelectionRequest;
  controller: AbortController;
  timer: ReturnType<typeof setTimeout>;
}
/** Display-only correlation. Only native one-use admission can precede selection. */
export class CliSelectionController {
  private pending: Pending | undefined;
  constructor(private readonly options: Options) {
    options.signal.addEventListener("abort", () => this.clear(), {
      once: true,
    });
  }
  private clear(pending = this.pending): void {
    if (!pending) return;
    if (this.pending === pending) this.pending = undefined;
    clearTimeout(pending.timer);
    pending.controller.abort();
  }
  private current(pending: Pending): boolean {
    return (
      this.pending === pending &&
      !pending.controller.signal.aborted &&
      !this.options.signal.aborted &&
      this.options.current() === pending.request.nonce
    );
  }
  async request(value: unknown): Promise<boolean> {
    let request: CliSelectionRequest;
    try {
      request = decodeCliSelection(value);
    } catch {
      return false;
    }
    const options = this.options;
    if (
      options.signal.aborted ||
      options.current() !== request.nonce ||
      this.pending
    )
      return false;
    if (!options.available()) {
      try {
        await options.complete(request, false);
      } catch {
        /* No retry. */
      }
      return false;
    }
    const pending: Pending = {
      request,
      controller: new AbortController(),
      timer: setTimeout(() => this.clear(pending), 5_000),
    };
    this.pending = pending;
    let admitted = false,
      answered = false;
    try {
      await options.admit(request);
      admitted = true;
      if (!this.current(pending)) return false;
      if (!options.available()) {
        answered = true;
        await options.complete(request, false);
        return false;
      }
      const selected = await options.select(request, pending.controller.signal);
      if (!this.current(pending)) return false;
      answered = true; // An uncertain completion is never retried or undone.
      await options.complete(request, selected);
      return selected && this.current(pending);
    } catch {
      if (admitted && !answered && this.current(pending)) {
        answered = true;
        try {
          await options.complete(request, false);
        } catch {
          /* No retry. */
        }
      }
      return false;
    } finally {
      this.clear(pending);
    }
  }
  cancel(value: unknown): void {
    if (!value || typeof value !== "object" || Array.isArray(value)) return;
    const body = value as Record<string, unknown>;
    if (
      Object.keys(body).length === 1 &&
      body.ticket === this.pending?.request.ticket
    )
      this.clear();
  }
}
export function installCliSelection(
  target: EventTarget,
  options: Options,
): CliSelectionController {
  const controller = new CliSelectionController(options);
  target.addEventListener(
    "pulseterm:cli-focus-tab",
    (event) => {
      void controller.request((event as CustomEvent<unknown>).detail);
    },
    { signal: options.signal },
  );
  target.addEventListener(
    "pulseterm:cli-cancel",
    (event) => {
      controller.cancel((event as CustomEvent<unknown>).detail);
    },
    { signal: options.signal },
  );
  return controller;
}
