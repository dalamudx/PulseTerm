import { invoke } from "@tauri-apps/api/core";
import {
  type WorkspaceCommandId,
  workspaceCommandIds,
  workspacePaletteOnlyCommandIds,
} from "./workspace-command-catalog";
// Drawing/selection metadata only. No backend invocation or permission receipt.
export interface WorkspaceCapability {
  readonly id: WorkspaceCommandId;
  readonly title: string;
  readonly risk: "safe" | "confirm";
}
type Rpc = (command: string, args: Record<string, unknown>) => Promise<unknown>;
function record(
  value: unknown,
  keys: readonly string[],
  required = keys,
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("workspace.cap_decode");
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null)
    throw new Error("workspace.cap_decode");
  const descriptors = Object.getOwnPropertyDescriptors(value);
  if (
    Reflect.ownKeys(descriptors).some(
      (key) => typeof key !== "string" || !keys.includes(key),
    ) ||
    required.some((key) => !Object.hasOwn(descriptors, key)) ||
    Object.values(descriptors).some((descriptor) => !("value" in descriptor))
  )
    throw new Error("workspace.cap_decode");
  return value as Record<string, unknown>;
}
function array(value: unknown, maximum: number): readonly unknown[] {
  if (
    !Array.isArray(value) ||
    Object.getPrototypeOf(value) !== Array.prototype ||
    value.length > maximum ||
    Reflect.ownKeys(value).some(
      (key) =>
        typeof key !== "string" ||
        (key !== "length" && !/^(0|[1-9][0-9]*)$/.test(key)),
    )
  )
    throw new Error("workspace.cap_decode");
  for (let index = 0; index < value.length; index++) {
    const descriptor = Object.getOwnPropertyDescriptor(value, `${index}`);
    if (!descriptor || !("value" in descriptor))
      throw new Error("workspace.cap_decode");
  }
  return value;
}
export function decodeWorkspaceCapabilities(
  value: unknown,
): readonly WorkspaceCapability[] {
  const body = record(value, ["version", "actions"]);
  if (body.version !== 1) throw new Error("workspace.cap_version");
  const seen = new Set<string>();
  const result = array(body.actions, workspaceCommandIds.length).map(
    (value) => {
      const action = record(value, [
        "id",
        "title",
        "scope",
        "risk",
        "surfaces",
        "args",
      ]);
      if (
        typeof action.id !== "string" ||
        !workspaceCommandIds.includes(action.id as WorkspaceCommandId) ||
        seen.has(action.id) ||
        action.title !== `workspace.commands.${action.id}` ||
        action.scope !== "window" ||
        action.risk !== (action.id === "close-session" ? "confirm" : "safe")
      )
        throw new Error("workspace.cap_decode");
      const surfaces = array(action.surfaces, 2);
      const paletteOnly = workspacePaletteOnlyCommandIds.some(
        (id) => id === action.id,
      );
      if (
        surfaces.length !== (paletteOnly ? 1 : 2) ||
        surfaces[0] !== "palette" ||
        (!paletteOnly && surfaces[1] !== "keybinding")
      )
        throw new Error("workspace.cap_decode");
      const args = record(
        action.args,
        ["$schema", "title", "type", "additionalProperties", "properties"],
        ["type", "additionalProperties"],
      );
      if (
        args.type !== "object" ||
        args.additionalProperties !== false ||
        (args.title !== undefined && args.title !== "EmptyArgs") ||
        (args.$schema !== undefined &&
          args.$schema !== "https://json-schema.org/draft/2020-12/schema")
      )
        throw new Error("workspace.cap_decode");
      if (args.properties !== undefined) record(args.properties, []);
      seen.add(action.id);
      return Object.freeze({
        id: action.id as WorkspaceCommandId,
        title: action.title as string,
        risk: action.risk as "safe" | "confirm",
      });
    },
  );
  // Only a fixed empty-object schema is accepted; no external refs, templates or schema execution.
  return Object.freeze(result);
}
export async function readWorkspaceCapabilities(
  nonce: string,
  signal: AbortSignal,
  currentNonce: () => string | undefined,
  rpc: Rpc = invoke,
): Promise<readonly WorkspaceCapability[]> {
  if (signal.aborted || currentNonce() !== nonce)
    throw new Error("workspace.cap_cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort: (() => void) | undefined;
  try {
    const value = await Promise.race([
      Promise.resolve().then(() => {
        if (signal.aborted || currentNonce() !== nonce)
          throw new Error("workspace.cap_cancelled");
        return rpc("workspace_cap_list", { nonce });
      }),
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("workspace.cap_timeout")),
          5000,
        );
        abort = () => reject(new Error("workspace.cap_cancelled"));
        signal.addEventListener("abort", abort, { once: true });
      }),
    ]);
    if (signal.aborted || currentNonce() !== nonce)
      throw new Error("workspace.cap_cancelled");
    return decodeWorkspaceCapabilities(value);
  } finally {
    if (timer) clearTimeout(timer);
    if (abort) signal.removeEventListener("abort", abort);
  }
}
