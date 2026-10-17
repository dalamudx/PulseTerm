import {
  decodeProfileConfig,
  type ProfileConfig,
} from "./workspace-profile-editor";
import { isWorkspaceNonce } from "./workspace-readiness";
import { decodeWorkspaceSessions } from "./workspace-sessions";
export type CustomProfileId = string & {
  readonly customProfileId: unique symbol;
};
export type CustomProfile = Readonly<{
  id: string;
  name: string;
  executable: string;
  hidden: boolean;
  deleted: boolean;
  config: ProfileConfig;
}>;
export type CustomDraft = Omit<CustomProfile, "id" | "deleted">;
export type CustomOffer = Readonly<{
  ticket: string;
  profiles: readonly CustomProfile[];
}>;
export type CustomChange =
  | { kind: "create"; draft: CustomDraft }
  | { kind: "update"; id: string; draft: CustomDraft }
  | { kind: "delete" | "restore"; id: string };
export interface CustomNative {
  read(nonce: string): Promise<unknown>;
  save(nonce: string, ticket: string, change: CustomChange): Promise<unknown>;
  cancel(nonce: string, ticket: string): Promise<unknown>;
}
const UUID7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
export function isCustomProfileId(value: string): value is CustomProfileId {
  return UUID7.test(value);
}
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("profile.invalid");
  return value as Record<string, unknown>;
}
function containsControl(value: string): boolean {
  for (let index = 0; index < value.length; index++) {
    const code = value.charCodeAt(index);
    if (code < 32 || code === 127) return true;
  }
  return false;
}
export function decodeCustomDraft(value: unknown): CustomDraft {
  const body = object(value);
  if (
    typeof body.name !== "string" ||
    !body.name.trim() ||
    [...body.name].length > 80 ||
    containsControl(body.name) ||
    typeof body.executable !== "string" ||
    new TextEncoder().encode(body.executable).length > 4096 ||
    !/^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(body.executable) ||
    containsControl(body.executable) ||
    typeof body.hidden !== "boolean"
  )
    throw Error("profile.invalid");
  return Object.freeze({
    name: body.name,
    executable: body.executable,
    hidden: body.hidden,
    config: decodeProfileConfig(body.config),
  });
}
export function decodeCustomOffer(value: unknown): CustomOffer {
  const body = object(value);
  if (
    typeof body.ticket !== "string" ||
    !isWorkspaceNonce(body.ticket) ||
    !Array.isArray(body.profiles) ||
    body.profiles.length > 64
  )
    throw Error("profile.invalid");
  const seen = new Set<string>();
  const profiles = body.profiles.map((value) => {
    const profile = object(value);
    if (
      typeof profile.id !== "string" ||
      !isCustomProfileId(profile.id) ||
      seen.has(profile.id) ||
      typeof profile.deleted !== "boolean"
    )
      throw Error("profile.invalid");
    seen.add(profile.id);
    return Object.freeze({
      id: profile.id,
      deleted: profile.deleted,
      ...decodeCustomDraft(profile),
    });
  });
  return Object.freeze({
    ticket: body.ticket,
    profiles: Object.freeze(profiles),
  });
}
export const emptyCustomConfig = (): ProfileConfig =>
  decodeProfileConfig({
    args: [],
    cwd: null,
    env: [],
    envRemove: [],
    appearance: { fontSize: null, fontFamily: null, cursorBlink: null },
  });
export type CustomBrief = Readonly<
  Pick<CustomProfile, "id" | "name" | "hidden">
>;
export function decodeCustomBriefs(value: unknown): readonly CustomBrief[] {
  if (!Array.isArray(value) || value.length > 64)
    throw Error("profile.invalid");
  const seen = new Set<string>();
  return Object.freeze(
    value.map((item) => {
      const row = object(item);
      if (
        typeof row.id !== "string" ||
        !isCustomProfileId(row.id) ||
        seen.has(row.id) ||
        typeof row.name !== "string" ||
        !row.name.trim() ||
        [...row.name].length > 80 ||
        containsControl(row.name) ||
        typeof row.hidden !== "boolean"
      )
        throw Error("profile.invalid");
      seen.add(row.id);
      return Object.freeze({ id: row.id, name: row.name, hidden: row.hidden });
    }),
  );
}
export async function openCustomProfile(
  nonce: string,
  id: string,
  send: (nonce: string, id: string) => Promise<unknown>,
): Promise<string> {
  if (!isWorkspaceNonce(nonce) || !isCustomProfileId(id))
    throw Error("workspace.denied");
  const result = await send(nonce, id);
  const [session] = decodeWorkspaceSessions([result]);
  if (session === undefined) throw Error("workspace.denied");
  return session;
}
