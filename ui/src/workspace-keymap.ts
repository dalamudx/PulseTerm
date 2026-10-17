import { invoke } from "@tauri-apps/api/core";
import { parseBindingCondition } from "./keybindings/conditions";
import {
  compileKeybindingDefinitions,
  type KeybindingAction,
} from "./keybindings/definitions";
import {
  decodeKeymapProjection,
  type KeymapProjection,
} from "./keybindings/projection";
import { workspaceKeybindingCommandIds } from "./workspace-command-catalog";

const uuid =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
export interface WorkspaceKeymap {
  readonly version: 1;
  readonly token: string;
  readonly confirmed: boolean;
  readonly values: KeymapProjection;
}
export type KeymapChange = Readonly<
  | {
      kind: "set";
      action: KeybindingAction;
      keys: readonly string[];
      when: string;
    }
  | { kind: "restore"; action: KeybindingAction }
>;
export type KeymapRpc = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;
function record(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("keymap.invalid");
  const prototype = Object.getPrototypeOf(value);
  if (
    (prototype !== Object.prototype && prototype !== null) ||
    Object.keys(value).length !== keys.length ||
    keys.some((key) => !Object.hasOwn(value, key)) ||
    Object.values(Object.getOwnPropertyDescriptors(value)).some(
      (descriptor) => !("value" in descriptor),
    )
  )
    throw Error("keymap.invalid");
  return value as Record<string, unknown>;
}
const decodedFrames = new WeakSet<WorkspaceKeymap>();
export function decodeWorkspaceKeymap(value: unknown): WorkspaceKeymap {
  if (
    value &&
    typeof value === "object" &&
    decodedFrames.has(value as WorkspaceKeymap)
  )
    return value as WorkspaceKeymap;
  const root = record(value, ["version", "token", "confirmed", "values"]);
  if (
    root.version !== 1 ||
    typeof root.token !== "string" ||
    !uuid.test(root.token) ||
    typeof root.confirmed !== "boolean"
  )
    throw Error("keymap.invalid");
  const decoded: WorkspaceKeymap = Object.freeze({
    version: 1,
    token: root.token,
    confirmed: root.confirmed,
    values: decodeKeymapProjection(root.values),
  });
  decodedFrames.add(decoded);
  return decoded;
}
export function validateKeymapChange(value: unknown): KeymapChange {
  if (!value || typeof value !== "object") throw Error("keymap.invalid");
  const descriptor = Object.getOwnPropertyDescriptor(value, "kind");
  const kind =
    descriptor && "value" in descriptor ? descriptor.value : undefined;
  const root = record(
    value,
    kind === "restore"
      ? ["kind", "action"]
      : ["kind", "action", "keys", "when"],
  );
  if (
    typeof root.action !== "string" ||
    ![...workspaceKeybindingCommandIds, "palette"].some(
      (action) => action === root.action,
    )
  )
    throw Error("keymap.invalid");
  const action = root.action as KeybindingAction;
  if (kind === "restore") return Object.freeze({ kind, action });
  if (
    kind !== "set" ||
    !Array.isArray(root.keys) ||
    root.keys.length > 4 ||
    typeof root.when !== "string"
  )
    throw Error("keymap.invalid");
  parseBindingCondition(root.when);
  const keys = root.keys.map((key) => {
    if (typeof key !== "string") throw Error("keymap.invalid");
    return key;
  });
  compileKeybindingDefinitions(
    keys.map((key) => ({ action, keys: key, when: root.when })),
  );
  return Object.freeze({
    kind,
    action,
    keys: Object.freeze(keys),
    when: root.when,
  });
}
interface CancelGate {
  pending: Promise<void> | undefined;
  failed: boolean;
}
const cancelGates = new WeakMap<KeymapRpc, CancelGate>();
function gateFor(rpc: KeymapRpc): CancelGate {
  let gate = cancelGates.get(rpc);
  if (!gate) {
    gate = { pending: undefined, failed: false };
    cancelGates.set(rpc, gate);
  }
  return gate;
}
async function request(
  command: string,
  args: Record<string, unknown>,
  signal: AbortSignal,
  rpc: KeymapRpc,
): Promise<WorkspaceKeymap> {
  if (signal.aborted) throw Error("keymap.cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  let cancelled = false;
  let dispatched = false;
  const gate = gateFor(rpc);
  const cancel = () => {
    if (cancelled || !dispatched) return;
    cancelled = true;
    try {
      void cancelWorkspaceKeymap(String(args.nonce), rpc).catch(() => {});
    } catch {
      /* Cancellation cannot mask the original uncertain result. */
    }
  };
  const ended = new Promise<never>((_, reject) => {
    abort = () => {
      cancel();
      reject(Error("keymap.cancelled"));
    };
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => {
      cancel();
      reject(Error("keymap.timeout"));
    }, 5000);
  });
  try {
    // A previous page-scoped cancel must be acknowledged before a new flow is admitted.
    // A failed/uncertain cancel requires page reload; no automatic replay or new admission.
    while (gate.pending) {
      await Promise.race([gate.pending, ended]);
    }
    if (gate.failed) throw Error("keymap.cancel_unconfirmed");
    if (signal.aborted) throw Error("keymap.cancelled");
    dispatched = true;
    const result = await Promise.race([rpc(command, args), ended]);
    if (signal.aborted) throw Error("keymap.cancelled");
    return decodeWorkspaceKeymap(result);
  } catch (error) {
    cancel();
    throw error;
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
function badPhase(nonce: string, rpc: KeymapRpc): never {
  try {
    void cancelWorkspaceKeymap(nonce, rpc).catch(() => {});
  } catch {
    /* Preserve the protocol error. */
  }
  throw Error("keymap.invalid_phase");
}
export async function readWorkspaceKeymap(
  nonce: string,
  signal: AbortSignal,
  rpc: KeymapRpc = invoke,
): Promise<WorkspaceKeymap> {
  if (!uuid.test(nonce)) throw Error("keymap.invalid");
  const result = await request("workspace_keymap_read", { nonce }, signal, rpc);
  if (result.confirmed) return badPhase(nonce, rpc);
  return result;
}
export async function confirmWorkspaceKeymap(
  nonce: string,
  value: WorkspaceKeymap,
  signal: AbortSignal,
  rpc: KeymapRpc = invoke,
): Promise<WorkspaceKeymap> {
  const candidate = decodeWorkspaceKeymap(value);
  if (!uuid.test(nonce) || candidate.confirmed)
    throw Error("keymap.invalid_phase");
  const result = await request(
    "workspace_keymap_confirm",
    { nonce, ticket: candidate.token },
    signal,
    rpc,
  );
  if (!result.confirmed || result.token === candidate.token)
    return badPhase(nonce, rpc);
  return result;
}
export async function saveWorkspaceKeymap(
  nonce: string,
  value: WorkspaceKeymap,
  change: KeymapChange,
  consent: boolean,
  signal: AbortSignal,
  rpc: KeymapRpc = invoke,
): Promise<WorkspaceKeymap> {
  const candidate = decodeWorkspaceKeymap(value);
  if (!uuid.test(nonce) || !candidate.confirmed || consent !== true)
    throw Error("keymap.invalid_phase");
  const result = await request(
    "workspace_keymap_save",
    {
      nonce,
      ticket: candidate.token,
      change: validateKeymapChange(change),
      consent: true,
    },
    signal,
    rpc,
  );
  if (result.confirmed || result.token === candidate.token)
    return badPhase(nonce, rpc);
  return result;
}
export function cancelWorkspaceKeymap(
  nonce: string,
  rpc: KeymapRpc = invoke,
): Promise<void> {
  if (!uuid.test(nonce)) return Promise.reject(Error("keymap.invalid"));
  const gate = gateFor(rpc);
  let sent: Promise<unknown>;
  try {
    sent = rpc("workspace_keymap_cancel", { nonce });
  } catch (error) {
    sent = Promise.reject(error);
  }
  const pending = Promise.all([gate.pending, sent]).then(() => {});
  gate.pending = pending;
  void pending.then(
    () => {
      if (gate.pending === pending) gate.pending = undefined;
    },
    () => {
      gate.failed = true;
    },
  );
  return pending;
}
