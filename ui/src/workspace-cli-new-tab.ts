import { isCustomProfileId as isSavedProfile } from "./workspace-custom-profiles";
import type { DefaultProfileId as ProfileKind } from "./workspace-profile-default";
import { isWorkspaceNonce } from "./workspace-readiness";
import { decodeWorkspaceSessions } from "./workspace-sessions";
import {
  decodeInteractivePrompt,
  type InteractivePrompt,
} from "./workspace-ssh-interactive";

export type CliSshAuthentication = "interactive" | "key-interactive";
function isCliSshAuthentication(value: unknown): value is CliSshAuthentication {
  return value === "interactive" || value === "key-interactive";
}
export interface CliSshPrompt {
  hostId: string;
  identityId: string;
  hostLabel: string;
  address: string;
  port: number;
  identityLabel: string;
  username: string;
  authentication?: CliSshAuthentication;
}
export type CliSshIntent =
  | {
      hostId: string;
      identityId: string;
      alias?: never;
      authentication?: CliSshAuthentication;
    }
  | { alias: string; hostId?: never; identityId?: never };
type Request =
  | {
      ticket: string;
      newWindow: true;
      profile?: never;
      direction?: never;
      baseDirection?: never;
      targetSession?: never;
      ssh?: never;
      intent?: never;
    }
  | {
      ticket: string;
      profile: ProfileKind;
      direction?: "row" | "column";
      baseDirection?: "row" | "column";
      targetSession?: string;
      ssh?: never;
      intent?: never;
      newWindow?: never;
    }
  | {
      ticket: string;
      ssh: CliSshPrompt;
      profile?: never;
      direction?: never;
      baseDirection?: never;
      targetSession?: never;
      intent?: never;
      newWindow?: never;
    }
  | {
      ticket: string;
      intent: CliSshIntent;
      ssh?: never;
      profile?: never;
      direction?: never;
      baseDirection?: never;
      targetSession?: never;
      newWindow?: never;
    };
function savedId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      value,
    )
  );
}
function text(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 512 &&
    Array.from(value).every((character) => {
      const code = character.codePointAt(0) ?? 0;
      return code >= 32 && (code < 127 || code > 159);
    })
  );
}
function savedAlias(value: unknown): value is string {
  return (
    text(value) &&
    value.trim() === value &&
    new TextEncoder().encode(value).length <= 256 &&
    !/[\u200b-\u200f\u202a-\u202e\u2060-\u206f\ufeff]/u.test(value)
  );
}
export interface CliSplitTarget {
  session: string;
  identity: object;
}
export interface CliSplitLayout {
  direction: "row" | "column";
  target: CliSplitTarget;
}
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("workspace.cli_invalid");
  return value as Record<string, unknown>;
}
export function decodeCliNewTab(value: unknown): Request {
  const body = object(value);
  if (body.version === 5) {
    if (
      Object.keys(body).length !== 2 ||
      typeof body.ticket !== "string" ||
      !isWorkspaceNonce(body.ticket)
    )
      throw Error("workspace.cli_invalid");
    return { ticket: body.ticket, newWindow: true };
  }
  if (body.version === 4) {
    if (
      typeof body.ticket !== "string" ||
      !isWorkspaceNonce(body.ticket) ||
      typeof body.profile !== "string" ||
      (!["bash", "zsh", "fish", "sh", "pwsh"].includes(body.profile) &&
        !isSavedProfile(body.profile)) ||
      (body.direction !== "row" && body.direction !== "column")
    )
      throw new Error("workspace.cli_invalid");
    const profile = body.profile as ProfileKind;
    if (body.stage === "base" && Object.keys(body).length === 5) {
      return { ticket: body.ticket, profile, baseDirection: body.direction };
    }
    if (
      body.stage === "split" &&
      Object.keys(body).length === 6 &&
      typeof body.target === "string"
    ) {
      const target = decodeWorkspaceSessions([body.target])[0];
      if (target)
        return {
          ticket: body.ticket,
          profile,
          direction: body.direction,
          targetSession: target,
        };
    }
    throw new Error("workspace.cli_invalid");
  }
  if (body.version === 3) {
    const intent = object(body.intent);
    if (
      Object.keys(body).length !== 3 ||
      typeof body.ticket !== "string" ||
      !isWorkspaceNonce(body.ticket)
    ) {
      throw new Error("workspace.cli_invalid");
    }
    if (Object.hasOwn(intent, "alias")) {
      if (Object.keys(intent).length !== 1 || !savedAlias(intent.alias))
        throw new Error("workspace.cli_invalid");
      return { ticket: body.ticket, intent: { alias: intent.alias } };
    }
    if (
      Object.keys(intent).length !== ("authentication" in intent ? 3 : 2) ||
      ("authentication" in intent &&
        !isCliSshAuthentication(intent.authentication)) ||
      !savedId(intent.hostId) ||
      !savedId(intent.identityId)
    )
      throw new Error("workspace.cli_invalid");
    return {
      ticket: body.ticket,
      intent: {
        hostId: intent.hostId,
        identityId: intent.identityId,
        ...("authentication" in intent
          ? { authentication: intent.authentication as CliSshAuthentication }
          : {}),
      },
    };
  }
  if (body.version === 2) {
    const ssh = object(body.ssh);
    if (
      Object.keys(body).length !== 3 ||
      typeof body.ticket !== "string" ||
      !isWorkspaceNonce(body.ticket) ||
      Object.keys(ssh).length !== ("authentication" in ssh ? 8 : 7) ||
      ("authentication" in ssh &&
        !isCliSshAuthentication(ssh.authentication)) ||
      !savedId(ssh.hostId) ||
      !savedId(ssh.identityId) ||
      !text(ssh.hostLabel) ||
      !text(ssh.address) ||
      !text(ssh.identityLabel) ||
      !text(ssh.username) ||
      typeof ssh.port !== "number" ||
      !Number.isInteger(ssh.port) ||
      ssh.port < 1 ||
      ssh.port > 65535
    )
      throw new Error("workspace.cli_invalid");
    return {
      ticket: body.ticket,
      ssh: {
        hostId: ssh.hostId,
        identityId: ssh.identityId,
        hostLabel: ssh.hostLabel,
        address: ssh.address,
        port: ssh.port,
        identityLabel: ssh.identityLabel,
        username: ssh.username,
        ...("authentication" in ssh
          ? { authentication: ssh.authentication as CliSshAuthentication }
          : {}),
      },
    };
  }
  if (
    Object.keys(body).length !== (body.direction === undefined ? 3 : 4) ||
    (body.direction !== undefined &&
      body.direction !== "row" &&
      body.direction !== "column") ||
    body.version !== 1 ||
    typeof body.ticket !== "string" ||
    !isWorkspaceNonce(body.ticket) ||
    typeof body.profile !== "string" ||
    (!["bash", "zsh", "fish", "sh", "pwsh"].includes(body.profile) &&
      !isSavedProfile(body.profile))
  )
    throw new Error("workspace.cli_invalid");
  return {
    ticket: body.ticket,
    profile: body.profile as ProfileKind,
    ...(body.direction === undefined
      ? {}
      : { direction: body.direction as "row" | "column" }),
  };
}
export interface CliNewTabOptions {
  signal: AbortSignal;
  current: () => string | undefined;
  available: () => boolean;
  captureSplit?: () => CliSplitTarget | undefined;
  confirmWindow?: (signal: AbortSignal) => Promise<boolean>;
  confirmColdSsh?: (
    selection: CliSshIntent,
    signal: AbortSignal,
  ) => Promise<boolean>;
  confirmSsh?: (
    selection: CliSshPrompt,
    signal: AbortSignal,
  ) => Promise<boolean>;
  confirm: (
    profile: ProfileKind,
    signal: AbortSignal,
    direction?: "row" | "column",
    baseDirection?: "row" | "column",
  ) => Promise<boolean>;
  answer: (nonce: string, ticket: string, accept: boolean) => Promise<unknown>;
  opened: (
    nonce: string,
    session: string,
    layout?: CliSplitLayout,
  ) => Promise<void>;
  admitPresentation?: (
    nonce: string,
    ticket: string,
    session: string,
  ) => Promise<void>;
  completePresentation?: (
    nonce: string,
    ticket: string,
    session: string,
    selected: boolean,
  ) => Promise<void>;
  interactive?: (
    prompt: InteractivePrompt,
    signal: AbortSignal,
  ) => Promise<string[] | undefined>;
  subscribeInteractive?: (
    receive: (value: unknown) => void,
  ) => Promise<() => void>;
  respondInteractive?: (
    nonce: string,
    token: string,
    round: string,
    answers: string[],
  ) => Promise<void>;
  cancelInteractive?: (nonce: string, token: string) => Promise<void>;
  failed: () => void;
}
interface Pending {
  nonce: string;
  ticket: string;
  controller: AbortController;
  approving: boolean;
  interactive?: boolean;
  approved?: boolean;
  token?: string;
  round?: string;
  rounds?: Set<string>;
  presenting?: boolean;
  layout?: CliSplitLayout;
  timer: ReturnType<typeof setTimeout>;
}
/** Display-only correlation. Native original-page approval and startup remain authoritative. */
export class CliNewTabController {
  private pending: Pending | undefined;
  async challenge(value: unknown): Promise<void> {
    const pending = this.pending,
      options = this.options;
    if (
      !pending?.interactive ||
      !pending.approved ||
      pending.controller.signal.aborted ||
      options.signal.aborted ||
      options.current() !== pending.nonce
    )
      return;
    let answers: string[] | undefined;
    try {
      const body = object(value);
      if (body.ticket !== pending.ticket) return;
      if (
        body.version !== 1 ||
        Object.keys(body).length !== 4 ||
        typeof body.token !== "string" ||
        !isWorkspaceNonce(body.token) ||
        (pending.token !== undefined && body.token !== pending.token)
      )
        throw Error("workspace.cli_invalid");
      const prompt = decodeInteractivePrompt(body.prompt);
      if (
        pending.round ||
        pending.rounds?.has(prompt.round) ||
        (pending.rounds?.size ?? 0) >= 8 ||
        !options.interactive ||
        !options.respondInteractive
      )
        throw Error("workspace.cli_invalid");
      pending.token = body.token;
      pending.round = prompt.round;
      pending.rounds?.add(prompt.round);
      answers = await options.interactive(prompt, pending.controller.signal);
      if (
        this.pending !== pending ||
        options.current() !== pending.nonce ||
        pending.controller.signal.aborted
      )
        return;
      if (!answers) {
        this.clear();
        return;
      }
      // The next native round can arrive before the response IPC resolves.
      pending.round = undefined;
      await options.respondInteractive(
        pending.nonce,
        body.token,
        prompt.round,
        answers,
      );
    } catch {
      if (this.pending === pending) {
        this.clear();
        if (options.current() === pending.nonce) options.failed();
      }
    } finally {
      answers?.fill("");
    }
  }
  constructor(private readonly options: CliNewTabOptions) {
    options.signal.addEventListener("abort", () => this.clear(), {
      once: true,
    });
  }
  private clear(cancel = true): void {
    const pending = this.pending;
    this.pending = undefined;
    if (pending) {
      clearTimeout(pending.timer);
      pending.controller.abort();
      if (pending.token && cancel) {
        void this.options
          .cancelInteractive?.(pending.nonce, pending.token)
          .catch(() => {
            /* Original-page revocation may already have drained it. */
          });
      }
    }
  }
  private sameTarget(layout: CliSplitLayout | undefined): boolean {
    if (!layout) return true;
    const current = this.options.captureSplit?.();
    return (
      current?.session === layout.target.session &&
      current?.identity === layout.target.identity
    );
  }
  async request(value: unknown): Promise<void> {
    let request: Request;
    try {
      request = decodeCliNewTab(value);
    } catch {
      return;
    }
    const { options } = this;
    const nonce = options.current();
    if (options.signal.aborted || !nonce || !isWorkspaceNonce(nonce)) return;
    if (this.pending?.ticket === request.ticket) return;
    const target = request.direction ? options.captureSplit?.() : undefined;
    if (
      this.pending ||
      !options.available() ||
      (request.targetSession !== undefined &&
        target?.session !== request.targetSession) ||
      (request.direction && !target)
    ) {
      try {
        await options.answer(nonce, request.ticket, false);
      } catch {
        /* No launch and no retry. */
      }
      return;
    }
    const controller = new AbortController();
    const pending: Pending = {
      nonce,
      ticket: request.ticket,
      controller,
      approving: false,
      layout:
        request.direction && target
          ? { direction: request.direction, target: { ...target } }
          : undefined,
      interactive:
        request.ssh !== undefined && request.ssh.authentication !== undefined,
      rounds: new Set(),
      timer: setTimeout(() => {
        if (this.pending === pending) this.clear();
      }, 35_000),
    };
    this.pending = pending;
    try {
      const accept = request.newWindow
        ? await (options.confirmWindow?.(controller.signal) ??
            Promise.resolve(false))
        : request.intent
          ? await (options.confirmColdSsh?.(
              request.intent,
              controller.signal,
            ) ?? Promise.resolve(false))
          : request.ssh
            ? await (options.confirmSsh?.(request.ssh, controller.signal) ??
                Promise.resolve(false))
            : request.baseDirection
              ? await options.confirm(
                  request.profile,
                  controller.signal,
                  undefined,
                  request.baseDirection,
                )
              : await options.confirm(
                  request.profile,
                  controller.signal,
                  request.direction,
                );
      if (
        this.pending !== pending ||
        controller.signal.aborted ||
        options.signal.aborted ||
        options.current() !== nonce
      )
        return;
      pending.approved = accept;
      pending.approving =
        accept && options.available() && this.sameTarget(pending.layout);
      if (request.intent || request.newWindow) {
        // The metadata phase never waits for a session event. Clear before publishing the answer.
        this.clear();
        try {
          await options.answer(nonce, request.ticket, pending.approving);
        } catch {
          if (!options.signal.aborted && options.current() === nonce)
            options.failed();
        }
        return;
      }
      await options.answer(nonce, request.ticket, pending.approving);
      if (!pending.approving && this.pending === pending) this.clear();
    } catch {
      if (this.pending === pending) {
        this.clear();
        if (!options.signal.aborted && options.current() === nonce)
          options.failed();
      }
    }
  }
  cancel(value: unknown): void {
    try {
      const body = object(value);
      if (
        Object.keys(body).length === 1 &&
        body.ticket === this.pending?.ticket
      )
        this.clear();
    } catch {
      /* Invalid notices never affect another decision. */
    }
  }
  async opened(value: unknown): Promise<void> {
    const pending = this.pending;
    if (
      !pending?.approving ||
      pending.presenting ||
      this.options.signal.aborted ||
      this.options.current() !== pending.nonce
    )
      return;
    const options = this.options;
    const { admitPresentation, completePresentation } = options;
    let presentation: string | undefined;
    let session: string | undefined;
    let admitted = false,
      answered = false;
    const current = () =>
      !options.signal.aborted && options.current() === pending.nonce;
    try {
      const body = object(value);
      if (body.ticket !== pending.ticket) return;
      if (body.version === 5) {
        if (
          Object.keys(body).length !== 5 ||
          body.nonce !== pending.nonce ||
          typeof body.presentation !== "string" ||
          !isWorkspaceNonce(body.presentation) ||
          body.presentation !== body.presentation.toLowerCase() ||
          !options.admitPresentation ||
          !options.completePresentation
        )
          return;
        presentation = body.presentation;
      } else if (Object.keys(body).length !== 2) return;
      session = decodeWorkspaceSessions([body.session])[0];
      if (!session) return;
      if (
        !this.sameTarget(pending.layout) ||
        session === pending.layout?.target.session
      ) {
        this.clear();
        throw Error("workspace.cli_stale_target");
      }
      pending.presenting = true;
      if (presentation && admitPresentation) {
        await admitPresentation(pending.nonce, presentation, session);
        admitted = true;
        if (
          this.pending !== pending ||
          !current() ||
          pending.controller.signal.aborted ||
          !this.sameTarget(pending.layout)
        ) {
          throw Error("workspace.cli_presentation_revoked");
        }
      }
      this.clear(false);
      if (pending.layout)
        await options.opened(pending.nonce, session, pending.layout);
      else await options.opened(pending.nonce, session);
      if (presentation && completePresentation && current()) {
        answered = true;
        await completePresentation(pending.nonce, presentation, session, true);
      }
    } catch {
      if (this.pending === pending && pending.presenting) this.clear();
      if (
        presentation &&
        session &&
        admitted &&
        !answered &&
        completePresentation &&
        current()
      ) {
        answered = true;
        try {
          await completePresentation(
            pending.nonce,
            presentation,
            session,
            false,
          );
        } catch {
          /* No uncertain completion is retried. */
        }
      }
      if (current()) options.failed();
    }
  }
}

export function installCliNewTab(
  window: Window,
  options: CliNewTabOptions,
): void {
  const controller = new CliNewTabController(options);
  void options
    .subscribeInteractive?.((value) => {
      void controller.challenge(value);
    })
    .then((unlisten) => {
      if (options.signal.aborted) unlisten();
      else options.signal.addEventListener("abort", unlisten, { once: true });
    })
    .catch(() => {
      if (!options.signal.aborted) options.failed();
    });
  window.addEventListener(
    "pulseterm:cli-approval",
    (event) => {
      void controller.request((event as CustomEvent).detail);
    },
    { signal: options.signal },
  );
  window.addEventListener(
    "pulseterm:cli-cancel",
    (event) => controller.cancel((event as CustomEvent).detail),
    { signal: options.signal },
  );
  window.addEventListener(
    "pulseterm:cli-session",
    (event) => {
      void controller.opened((event as CustomEvent).detail);
    },
    { signal: options.signal },
  );
}
