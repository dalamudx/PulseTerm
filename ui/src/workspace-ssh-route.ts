import { invoke } from "@tauri-apps/api/core";
import { answerCliInteractive } from "./workspace-cli-interactive";
import {
  decodeInteractivePrompt,
  type InteractivePrompt,
} from "./workspace-ssh-interactive";
export type RouteDecision =
  | boolean
  | "interactive"
  | "key-interactive"
  | "agent-interactive"
  | "password-interactive";
type RouteAsk = (
  prompt: InteractivePrompt,
  signal: AbortSignal,
) => Promise<string[] | undefined>;

import type { HostRpc } from "./workspace-hosts";
import { isWorkspaceNonce } from "./workspace-readiness";

const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
export type RouteSelection = Readonly<{
  host: string;
  identity: string;
  profile: string;
  privateKey: boolean;
  agent?: boolean;
}>;
export type RoutePrompt = Readonly<{
  index: number;
  count: number;
  entrance: string;
  label: string;
  host: string;
  port: number;
  username: string;
  identity: string;
  identityLabel: string;
  algorithm: string;
  fingerprint: string;
  firstUse: boolean;
  interactiveAvailable?: boolean;
  keyInteractiveAvailable?: boolean;
  agentInteractiveAvailable?: boolean;
  passwordInteractiveAvailable?: boolean;
}>;
type Reply = Readonly<
  | { state: "connected"; session: string }
  | { state: "interactive"; prompt: InteractivePrompt }
  | { state: "confirm"; challenge: string; prompt: RoutePrompt }
>;
function row(value: unknown, keys: string[]): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("ssh.route_protocol");
  const r = value as Record<string, unknown>;
  if (
    Object.keys(r).length !== keys.length ||
    keys.some((k) => !Object.hasOwn(r, k))
  )
    throw Error("ssh.route_protocol");
  return r;
}
function text(value: unknown, limit: number): string {
  if (
    typeof value !== "string" ||
    !value ||
    new TextEncoder().encode(value).length > limit ||
    /[\p{Cc}\u00ad\u061c\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufeff]/u.test(
      value,
    )
  )
    throw Error("ssh.route_protocol");
  return value;
}
export function decodeRouteReply(value: unknown): Reply {
  const state =
    value && typeof value === "object"
      ? (value as Record<string, unknown>).state
      : undefined;
  if (state === "connected") {
    const r = row(value, ["state", "version", "session"]);
    if (
      r.version !== 1 ||
      typeof r.session !== "string" ||
      !/^[1-9][0-9]{0,19}$/.test(r.session) ||
      BigInt(r.session) > 18446744073709551615n
    )
      throw Error("ssh.route_protocol");
    return Object.freeze({ state, session: r.session });
  }
  if (state === "interactive") {
    const r = row(value, ["state", "version", "prompt"]);
    if (r.version !== 1) throw Error("ssh.route_protocol");
    return Object.freeze({ state, prompt: decodeInteractivePrompt(r.prompt) });
  }
  const r = row(value, ["state", "version", "challenge", "prompt"]);
  if (
    state !== "confirm" ||
    r.version !== 1 ||
    typeof r.challenge !== "string" ||
    !uuid4.test(r.challenge)
  )
    throw Error("ssh.route_protocol");
  const p = row(r.prompt, [
    "index",
    "count",
    "entrance",
    "label",
    "host",
    "port",
    "username",
    "identity",
    "identityLabel",
    "algorithm",
    "fingerprint",
    "firstUse",
    ...(r.prompt &&
    typeof r.prompt === "object" &&
    Object.hasOwn(r.prompt, "interactiveAvailable")
      ? ["interactiveAvailable"]
      : []),
    ...(r.prompt &&
    typeof r.prompt === "object" &&
    Object.hasOwn(r.prompt, "keyInteractiveAvailable")
      ? ["keyInteractiveAvailable"]
      : []),
    ...(r.prompt &&
    typeof r.prompt === "object" &&
    Object.hasOwn(r.prompt, "agentInteractiveAvailable")
      ? ["agentInteractiveAvailable"]
      : []),
    ...(r.prompt &&
    typeof r.prompt === "object" &&
    Object.hasOwn(r.prompt, "passwordInteractiveAvailable")
      ? ["passwordInteractiveAvailable"]
      : []),
  ]);
  if (
    !Number.isInteger(p.index) ||
    !Number.isInteger(p.count) ||
    (p.count as number) < 1 ||
    (p.count as number) > 9 ||
    (p.index as number) < 0 ||
    (p.index as number) >= (p.count as number) ||
    !Number.isInteger(p.port) ||
    (p.port as number) < 1 ||
    (p.port as number) > 65535 ||
    typeof p.firstUse !== "boolean" ||
    (Object.hasOwn(p, "interactiveAvailable") &&
      typeof p.interactiveAvailable !== "boolean") ||
    (Object.hasOwn(p, "keyInteractiveAvailable") &&
      typeof p.keyInteractiveAvailable !== "boolean") ||
    (Object.hasOwn(p, "agentInteractiveAvailable") &&
      typeof p.agentInteractiveAvailable !== "boolean") ||
    (Object.hasOwn(p, "passwordInteractiveAvailable") &&
      typeof p.passwordInteractiveAvailable !== "boolean") ||
    (p.passwordInteractiveAvailable === true &&
      p.interactiveAvailable !== true) ||
    [
      p.interactiveAvailable,
      p.keyInteractiveAvailable,
      p.agentInteractiveAvailable,
    ].filter((available) => available === true).length > 1 ||
    typeof p.entrance !== "string" ||
    !uuid7.test(p.entrance) ||
    typeof p.identity !== "string" ||
    !uuid7.test(p.identity)
  )
    throw Error("ssh.route_protocol");
  const fingerprint = text(p.fingerprint, 128);
  if (!/^SHA256:[A-Za-z0-9+/]{43}$/.test(fingerprint))
    throw Error("ssh.route_protocol");
  return Object.freeze({
    state,
    challenge: r.challenge,
    prompt: Object.freeze({
      index: p.index as number,
      count: p.count as number,
      entrance: p.entrance,
      label: text(p.label, 256),
      host: text(p.host, 253),
      port: p.port as number,
      username: text(p.username, 256),
      identity: p.identity,
      identityLabel: text(p.identityLabel, 256),
      algorithm: text(p.algorithm, 128),
      fingerprint,
      firstUse: p.firstUse,
      ...(Object.hasOwn(p, "interactiveAvailable")
        ? { interactiveAvailable: p.interactiveAvailable as boolean }
        : {}),
      ...(Object.hasOwn(p, "keyInteractiveAvailable")
        ? { keyInteractiveAvailable: p.keyInteractiveAvailable as boolean }
        : {}),
      ...(Object.hasOwn(p, "agentInteractiveAvailable")
        ? { agentInteractiveAvailable: p.agentInteractiveAvailable as boolean }
        : {}),
      ...(Object.hasOwn(p, "passwordInteractiveAvailable")
        ? {
            passwordInteractiveAvailable:
              p.passwordInteractiveAvailable as boolean,
          }
        : {}),
    }),
  });
}
export async function connectSshRoute(
  nonce: string,
  selection: RouteSelection,
  signal: AbortSignal,
  current: () => boolean,
  confirm: (prompt: RoutePrompt) => Promise<RouteDecision>,
  rpc: HostRpc = invoke,
  ask: RouteAsk = (prompt, signal) =>
    answerCliInteractive(document, prompt, signal),
): Promise<string> {
  const hasAgent =
    !!selection &&
    typeof selection === "object" &&
    Object.hasOwn(selection, "agent");
  const s = row(selection, [
    "host",
    "identity",
    "profile",
    "privateKey",
    ...(hasAgent ? ["agent"] : []),
  ]);
  if (
    !isWorkspaceNonce(nonce) ||
    ![s.host, s.identity, s.profile].every(
      (id) => typeof id === "string" && uuid7.test(id),
    ) ||
    typeof s.privateKey !== "boolean" ||
    (hasAgent && typeof s.agent !== "boolean") ||
    (s.agent === true && s.privateKey === true) ||
    signal.aborted ||
    !current()
  )
    throw Error("ssh.route_denied");
  const selected = Object.freeze({
    host: selection.host,
    identity: selection.identity,
    profile: selection.profile,
    privateKey: selection.privateKey,
    ...(hasAgent ? { agent: selection.agent } : {}),
  });
  return runSshRoute(
    nonce,
    () => rpc("workspace_ssh_route_begin", { nonce, selection: selected }),
    signal,
    current,
    confirm,
    rpc,
    ask,
  );
}

/** Continue a backend-issued original CLI capture. Never issues a new GUI begin or renews authority. */
export async function continueSshRoute(
  nonce: string,
  token: string,
  signal: AbortSignal,
  current: () => boolean,
  confirm: (prompt: RoutePrompt) => Promise<RouteDecision>,
  rpc: HostRpc = invoke,
  ask: RouteAsk = (prompt, signal) =>
    answerCliInteractive(document, prompt, signal),
): Promise<string> {
  if (
    !isWorkspaceNonce(nonce) ||
    typeof token !== "string" ||
    !uuid4.test(token) ||
    signal.aborted ||
    !current()
  ) {
    throw Error("ssh.route_denied");
  }
  return runSshRoute(
    nonce,
    async () => ({ version: 1, token }),
    signal,
    current,
    confirm,
    rpc,
    ask,
  );
}

async function runSshRoute(
  nonce: string,
  begin: () => Promise<unknown>,
  signal: AbortSignal,
  current: () => boolean,
  confirm: (prompt: RoutePrompt) => Promise<RouteDecision>,
  rpc: HostRpc,
  ask: RouteAsk,
): Promise<string> {
  const questions = new AbortController();
  let token: string | undefined,
    done = false,
    cancelled = false,
    cancelSent = false;
  const cancel = () => {
    cancelled = true;
    if (token && !done && !cancelSent) {
      cancelSent = true;
      void rpc("workspace_ssh_route_cancel", { nonce, token }).catch(() => {});
    }
  };
  let stop: (e: Error) => void = () => {};
  const stopped = new Promise<never>((_, reject) => {
    stop = reject;
  });
  const abort = () => {
    questions.abort();
    cancel();
    stop(Error("ssh.route_cancelled"));
  };
  signal.addEventListener("abort", abort, { once: true });
  const timer = setTimeout(() => {
    cancel();
    stop(Error("ssh.route_uncertain"));
  }, 300000);
  const check = () => {
    if (cancelled || signal.aborted || !current()) {
      cancel();
      throw Error("ssh.route_cancelled");
    }
  };
  try {
    const begun = begin().then((value) => {
      const r = row(value, ["version", "token"]);
      if (
        r.version !== 1 ||
        typeof r.token !== "string" ||
        !uuid4.test(r.token)
      )
        throw Error("ssh.route_protocol");
      token = r.token;
      if (cancelled) {
        cancelSent = false;
        cancel();
      }
      return r.token;
    });
    token = await Promise.race([begun, stopped]);
    check();
    let reply = decodeRouteReply(
      await Promise.race([
        rpc("workspace_ssh_route_wait", { nonce, token }),
        stopped,
      ]),
    );
    let count: number | undefined,
      index = 0,
      interactive = false,
      interactionRounds = 0;
    const rounds = new Set<string>();
    const challenges = new Set<string>(),
      entrances = new Set<string>();
    while (reply.state !== "connected") {
      check();
      if (reply.state === "interactive") {
        if (
          !interactive ||
          index === 0 ||
          interactionRounds >= 8 ||
          rounds.has(reply.prompt.round)
        )
          throw Error("ssh.route_protocol");
        interactionRounds++;
        rounds.add(reply.prompt.round);
        const answers = await Promise.race([
          ask(reply.prompt, questions.signal),
          stopped,
        ]);
        check();
        if (!answers) throw Error("ssh.route_cancelled");
        try {
          const response = rpc("workspace_ssh_route_answer", {
            nonce,
            token,
            round: reply.prompt.round,
            answers,
          });
          answers.fill("");
          answers.length = 0;
          reply = decodeRouteReply(await Promise.race([response, stopped]));
        } finally {
          answers.fill("");
          answers.length = 0;
        }
        continue;
      }
      const p = reply.prompt;
      if (
        p.index !== index ||
        (count !== undefined && p.count !== count) ||
        challenges.has(reply.challenge) ||
        entrances.has(p.entrance)
      )
        throw Error("ssh.route_protocol");
      count = p.count;
      challenges.add(reply.challenge);
      entrances.add(p.entrance);
      const approved = await Promise.race([confirm(p), stopped]);
      check();
      if (
        approved !== true &&
        approved !== "interactive" &&
        approved !== "key-interactive" &&
        approved !== "agent-interactive" &&
        approved !== "password-interactive"
      )
        throw Error("ssh.route_cancelled");
      if (approved === "interactive" && p.interactiveAvailable !== true)
        throw Error("ssh.route_protocol");
      if (approved === "key-interactive" && p.keyInteractiveAvailable !== true)
        throw Error("ssh.route_protocol");
      if (
        approved === "agent-interactive" &&
        p.agentInteractiveAvailable !== true
      )
        throw Error("ssh.route_protocol");
      if (
        approved === "password-interactive" &&
        p.passwordInteractiveAvailable !== true
      )
        throw Error("ssh.route_protocol");
      interactive =
        approved === "interactive" ||
        approved === "key-interactive" ||
        approved === "agent-interactive" ||
        approved === "password-interactive";
      interactionRounds = 0;
      reply = decodeRouteReply(
        await Promise.race([
          rpc("workspace_ssh_route_confirm", {
            nonce,
            token,
            challenge: reply.challenge,
            approved,
          }),
          stopped,
        ]),
      );
      index++;
    }
    check();
    if (count === undefined || index !== count)
      throw Error("ssh.route_protocol");
    done = true;
    return reply.session;
  } finally {
    questions.abort();
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
    if (!done) cancel();
  }
}
