import { Channel, invoke } from "@tauri-apps/api/core";
import type { HostRpc } from "./workspace-hosts";
import { isWorkspaceNonce } from "./workspace-readiness";
import { decodeWorkspaceSessions } from "./workspace-sessions";
import {
  decodeInteractivePrompt,
  type InteractivePrompt,
} from "./workspace-ssh-interactive";

const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
export type SavedSshSelection = Readonly<{ host: string; identity: string }>;
function decodeTicket(input: unknown): string {
  if (!input || typeof input !== "object" || Array.isArray(input))
    throw Error("ssh.session_protocol");
  const row = input as Record<string, unknown>;
  if (
    Object.keys(row).length !== 2 ||
    !Object.hasOwn(row, "version") ||
    !Object.hasOwn(row, "token") ||
    row.version !== 1 ||
    typeof row.token !== "string" ||
    !uuid4.test(row.token)
  )
    throw Error("ssh.session_protocol");
  return row.token;
}
/** One explicit saved-host/identity gesture. A result never grants terminal input/view permission.
 * Abort or stale replies revoke only the pending request, not a root-adopted session.
 * Never retries begin/start or tries another identity after an uncertain authentication result. */
export function connectSavedSsh(
  nonce: string,
  selection: SavedSshSelection,
  signal: AbortSignal,
  current: () => boolean,
  rpc: HostRpc = invoke,
): Promise<string> {
  return connectSaved(nonce, selection, signal, current, false, rpc);
}
export function connectSavedPrivateKeySsh(
  nonce: string,
  selection: SavedSshSelection,
  signal: AbortSignal,
  current: () => boolean,
  rpc: HostRpc = invoke,
): Promise<string> {
  return connectSaved(nonce, selection, signal, current, true, rpc);
}
/** Requires server-confirmed partial key success followed by MFA; never an authentication fallback. */
export function connectSavedPrivateKeyInteractiveSsh(
  nonce: string,
  selection: SavedSshSelection,
  signal: AbortSignal,
  current: () => boolean,
  questions: (
    prompt: InteractivePrompt,
    signal: AbortSignal,
  ) => Promise<string[] | undefined>,
  rpc: HostRpc = invoke,
): Promise<string> {
  return connectSaved(
    nonce,
    selection,
    signal,
    current,
    "key-interactive",
    rpc,
    questions,
  );
}
/** Explicit password partial success followed by MFA; never a retry or fallback. */
export function connectSavedPasswordInteractiveSsh(
  nonce: string,
  selection: SavedSshSelection,
  signal: AbortSignal,
  current: () => boolean,
  questions: (
    prompt: InteractivePrompt,
    signal: AbortSignal,
  ) => Promise<string[] | undefined>,
  rpc: HostRpc = invoke,
): Promise<string> {
  return connectSaved(
    nonce,
    selection,
    signal,
    current,
    "password-interactive",
    rpc,
    questions,
  );
}
async function connectSaved(
  nonce: string,
  selection: SavedSshSelection,
  signal: AbortSignal,
  current: () => boolean,
  privateKey:
    | boolean
    | "agent"
    | "interactive"
    | "key-interactive"
    | "password-interactive"
    | "agent-interactive",
  rpc: HostRpc = invoke,
  questions?: (
    prompt: InteractivePrompt,
    signal: AbortSignal,
  ) => Promise<string[] | undefined>,
): Promise<string> {
  if (
    !isWorkspaceNonce(nonce) ||
    !uuid7.test(selection.host) ||
    !uuid7.test(selection.identity) ||
    Object.keys(selection).length !== 2 ||
    signal.aborted ||
    !current()
  )
    throw Error("ssh.session_denied");
  const selected = Object.freeze({
    host: selection.host,
    identity: selection.identity,
  });
  const interactive =
    privateKey === "password-interactive" ||
    privateKey === "interactive" ||
    privateKey === "key-interactive" ||
    privateKey === "agent-interactive";
  const channel = interactive ? new Channel<unknown>() : undefined;
  const duration = interactive ? 300_000 : 5000;
  const deadline = performance.now() + duration;
  let token: string | undefined;
  let cancelled = false;
  let done = false;
  let timeout = false;
  const challengeAbort = new AbortController();
  const healthy = () =>
    !signal.aborted && current() && performance.now() < deadline && !cancelled;
  const cancel = () => {
    cancelled = true;
    if (token && !done && !cancelSent) {
      cancelSent = true;
      void rpc(
        privateKey === "agent"
          ? "workspace_ssh_agent_session_cancel"
          : "workspace_ssh_session_cancel",
        { nonce, token },
      ).catch(() => {});
    }
  };
  let cancelSent = false;
  let rejectStop: (error: Error) => void = () => {};
  const stopped = new Promise<never>((_, reject) => {
    rejectStop = reject;
  });
  const abort = () => {
    cancel();
    rejectStop(Error("ssh.session_cancelled"));
  };
  signal.addEventListener("abort", abort, { once: true });
  const timer = setTimeout(() => {
    timeout = true;
    cancel();
    rejectStop(Error("ssh.session_uncertain"));
  }, duration);
  const check = () => {
    if (!healthy()) {
      cancel();
      throw Error(
        timeout || performance.now() >= deadline
          ? "ssh.session_uncertain"
          : "ssh.session_cancelled",
      );
    }
  };
  let challengeCount = 0;
  let answering = false;
  let answerReceipt: Promise<unknown> | undefined;
  let waitingForReceipt = false;
  const seenRounds = new Set<string>();
  if (channel)
    channel.onmessage = async (input) => {
      let answers: string[] | undefined;
      let ownsPrompt = false;
      try {
        if (!healthy() || !token || !questions || ++challengeCount > 8)
          throw Error("ssh.session_protocol");
        const prompt = decodeInteractivePrompt(input);
        if (seenRounds.has(prompt.round)) throw Error("ssh.session_protocol");
        seenRounds.add(prompt.round);
        if (answering) {
          if (!answerReceipt || waitingForReceipt)
            throw Error("ssh.session_protocol");
          waitingForReceipt = true;
          try {
            await answerReceipt;
          } finally {
            waitingForReceipt = false;
          }
          if (!healthy() || done || challengeAbort.signal.aborted) return;
          if (answering) throw Error("ssh.session_protocol");
        }
        ownsPrompt = true;
        answering = true;
        answers = await questions(prompt, challengeAbort.signal);
        if (!answers || !healthy()) {
          abort();
          return;
        }
        answerReceipt = rpc("workspace_ssh_interactive_session_answer", {
          nonce,
          token,
          round: prompt.round,
          answers,
        });
        const receipt = await answerReceipt;
        if (receipt !== null && receipt !== undefined)
          throw Error("ssh.session_protocol");
      } catch {
        abort();
      } finally {
        if (ownsPrompt) {
          answering = false;
          answerReceipt = undefined;
        }
        answers?.fill("");
      }
    };
  try {
    check();
    const begin = rpc(
      privateKey === "password-interactive"
        ? "workspace_ssh_password_interactive_session_begin"
        : privateKey === "agent-interactive"
          ? "workspace_ssh_agent_interactive_session_begin"
          : privateKey === "agent"
            ? "workspace_ssh_agent_session_begin"
            : privateKey === "interactive"
              ? "workspace_ssh_interactive_session_begin"
              : privateKey === "key-interactive"
                ? "workspace_ssh_private_key_interactive_session_begin"
                : privateKey
                  ? "workspace_ssh_key_session_begin"
                  : "workspace_ssh_session_begin",
      {
        nonce,
        selection: selected,
        ...(channel ? { challenges: channel } : {}),
      },
    ).then((value) => {
      token = decodeTicket(value);
      if (!healthy()) cancel();
      return token;
    });
    await Promise.race([begin, stopped]);
    check();
    const prepared = await Promise.race([
      rpc(
        privateKey === "agent" || privateKey === "agent-interactive"
          ? "workspace_ssh_agent_session_prepare"
          : privateKey === "interactive"
            ? "workspace_ssh_interactive_session_prepare"
            : privateKey && privateKey !== "password-interactive"
              ? "workspace_ssh_key_session_prepare"
              : "workspace_ssh_session_prepare",
        { nonce, token },
      ),
      stopped,
    ]);
    check();
    if (prepared !== null && prepared !== undefined)
      throw Error("ssh.session_protocol");
    const result = await Promise.race([
      rpc(
        privateKey === "agent"
          ? "workspace_ssh_agent_session_start"
          : "workspace_ssh_session_start",
        { nonce, token },
      ),
      stopped,
    ]);
    check();
    const id = decodeWorkspaceSessions([result])[0];
    if (!id) throw Error("ssh.session_protocol");
    done = true;
    return id;
  } finally {
    clearTimeout(timer);
    challengeAbort.abort();
    if (channel) channel.onmessage = () => {};
    signal.removeEventListener("abort", abort);
    if (!done) cancel();
  }
}

/** Explicit saved-Agent signature plus server-declared MFA. */
export function connectSavedAgentInteractiveSsh(
  nonce: string,
  selection: SavedSshSelection,
  signal: AbortSignal,
  current: () => boolean,
  questions: (
    prompt: InteractivePrompt,
    signal: AbortSignal,
  ) => Promise<string[] | undefined>,
  rpc: HostRpc = invoke,
): Promise<string> {
  return connectSaved(
    nonce,
    selection,
    signal,
    current,
    "agent-interactive",
    rpc,
    questions,
  );
}
export function connectSavedAgentSsh(
  nonce: string,
  selection: SavedSshSelection,
  signal: AbortSignal,
  current: () => boolean,
  rpc: HostRpc = invoke,
): Promise<string> {
  return connectSaved(nonce, selection, signal, current, "agent", rpc);
}

/** Explicit method selection only. No saved-password borrowing or automatic method fallback. */
export function connectSavedInteractiveSsh(
  nonce: string,
  selection: SavedSshSelection,
  signal: AbortSignal,
  current: () => boolean,
  questions: (
    prompt: InteractivePrompt,
    signal: AbortSignal,
  ) => Promise<string[] | undefined>,
  rpc: HostRpc = invoke,
): Promise<string> {
  return connectSaved(
    nonce,
    selection,
    signal,
    current,
    "interactive",
    rpc,
    questions,
  );
}
