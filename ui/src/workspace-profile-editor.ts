import type { DisplayPreferences } from "./features/terminal/display-preferences";
import { displayPreferences } from "./features/terminal/display-preferences";
import { isProfileKind, type ProfileKind } from "./workspace-profiles";
import { isWorkspaceNonce } from "./workspace-readiness";
import {
  isTerminalPreset,
  type TerminalPreset,
} from "./workspace-terminal-preset";
export interface ProfileAppearance {
  readonly fontSize: number | null;
  readonly fontFamily: string | null;
  readonly cursorBlink: boolean | null;
  readonly macOptionIsMeta?: boolean | null;
  readonly backspaceSend?: "del" | "bs" | null;
  readonly deleteSend?: "csi" | "del" | null;
  readonly colorPreset?: TerminalPreset | null;
}
export interface ProfileConfig {
  readonly args: readonly string[];
  readonly cwd: string | null;
  readonly env: readonly (readonly [string, string])[];
  readonly envRemove: readonly string[];
  readonly appearance: ProfileAppearance;
}
export interface ProfileEditorNative {
  read(nonce: string, shell: ProfileKind): Promise<unknown>;
  save(nonce: string, ticket: string, config: ProfileConfig): Promise<unknown>;
  cancel(nonce: string, ticket: string): Promise<unknown>;
}
export interface ProfileEditorOffer {
  readonly ticket: string;
  readonly shell: ProfileKind;
  readonly config: ProfileConfig;
}
function record(value: unknown, keys: string[]): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("profile.invalid");
  const r = value as Record<string, unknown>;
  if (Object.keys(r).sort().join(",") !== keys.sort().join(","))
    throw Error("profile.invalid");
  return r;
}
export function decodeProfileAppearance(value: unknown): ProfileAppearance {
  const extras = [
    "macOptionIsMeta",
    "backspaceSend",
    "deleteSend",
    "colorPreset",
  ].filter(
    (key) =>
      value != null && typeof value === "object" && Object.hasOwn(value, key),
  );
  const r = record(value, ["fontSize", "fontFamily", "cursorBlink", ...extras]);
  if (Object.values(r).some((value) => value === undefined))
    throw Error("profile.invalid");
  if (
    Object.hasOwn(r, "colorPreset") &&
    r.colorPreset !== null &&
    !isTerminalPreset(r.colorPreset)
  )
    throw Error("profile.invalid");
  displayPreferences({
    fontSize: r.fontSize ?? 14,
    fontFamily: r.fontFamily ?? "Pulse Mono",
    cursorBlink: r.cursorBlink ?? false,
    ...(r.macOptionIsMeta != null
      ? { macOptionIsMeta: r.macOptionIsMeta }
      : {}),
    ...(r.backspaceSend != null ? { backspaceSend: r.backspaceSend } : {}),
    ...(r.deleteSend != null ? { deleteSend: r.deleteSend } : {}),
  });
  return Object.freeze({
    fontSize: r.fontSize as number | null,
    fontFamily: r.fontFamily as string | null,
    cursorBlink: r.cursorBlink as boolean | null,
    ...(Object.hasOwn(r, "macOptionIsMeta")
      ? { macOptionIsMeta: r.macOptionIsMeta as boolean | null }
      : {}),
    ...(Object.hasOwn(r, "backspaceSend")
      ? { backspaceSend: r.backspaceSend as "del" | "bs" | null }
      : {}),
    ...(Object.hasOwn(r, "deleteSend")
      ? { deleteSend: r.deleteSend as "csi" | "del" | null }
      : {}),
    ...(Object.hasOwn(r, "colorPreset")
      ? { colorPreset: r.colorPreset as TerminalPreset | null }
      : {}),
  });
}
export function profileDisplay(
  base: DisplayPreferences | undefined,
  appearance: ProfileAppearance,
): DisplayPreferences | undefined {
  if (
    !base &&
    appearance.fontSize === null &&
    appearance.fontFamily === null &&
    appearance.cursorBlink === null &&
    appearance.macOptionIsMeta == null &&
    appearance.backspaceSend == null &&
    appearance.deleteSend == null
  )
    return undefined;
  return displayPreferences({
    ...base,
    fontSize: appearance.fontSize ?? base?.fontSize ?? 14,
    fontFamily: appearance.fontFamily ?? base?.fontFamily ?? "Pulse Mono",
    cursorBlink: appearance.cursorBlink ?? base?.cursorBlink ?? false,
    ...(appearance.macOptionIsMeta != null ||
    base?.macOptionIsMeta !== undefined
      ? {
          macOptionIsMeta:
            appearance.macOptionIsMeta ?? base?.macOptionIsMeta ?? false,
        }
      : {}),
    ...(appearance.backspaceSend != null || base?.backspaceSend !== undefined
      ? {
          backspaceSend:
            appearance.backspaceSend ?? base?.backspaceSend ?? "del",
        }
      : {}),
    ...(appearance.deleteSend != null || base?.deleteSend !== undefined
      ? { deleteSend: appearance.deleteSend ?? base?.deleteSend ?? "csi" }
      : {}),
  });
}
export function decodeProfileConfig(value: unknown): ProfileConfig {
  const r = record(value, ["args", "cwd", "env", "envRemove", "appearance"]);
  let total = 0;
  const text = (s: unknown): string => {
    if (typeof s !== "string" || s.includes("\0"))
      throw Error("profile.invalid");
    total += new TextEncoder().encode(s).length + 1;
    if (total > 16384) throw Error("profile.invalid");
    return s;
  };
  if (
    !Array.isArray(r.args) ||
    r.args.length > 64 ||
    !Array.isArray(r.env) ||
    !Array.isArray(r.envRemove) ||
    r.env.length + r.envRemove.length > 64
  )
    throw Error("profile.invalid");
  const args = r.args.map(text);
  const cwd = r.cwd === null ? null : text(r.cwd);
  if (
    cwd !== null &&
    (!cwd.startsWith("/") || new TextEncoder().encode(cwd).length > 4096)
  )
    throw Error("profile.invalid");
  const keys = new Set<string>();
  const key = (value: unknown) => {
    const s = text(value),
      canonical = s.toUpperCase();
    if (
      !/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(s) ||
      canonical.startsWith("PULSE_") ||
      [
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "PULSE_PANE_ID",
        "PULSE_SOCKET",
      ].includes(canonical) ||
      keys.has(canonical)
    )
      throw Error("profile.invalid");
    keys.add(canonical);
    return s;
  };
  const env = r.env.map((entry: unknown) => {
    if (!Array.isArray(entry) || entry.length !== 2)
      throw Error("profile.invalid");
    return Object.freeze([key(entry[0]), text(entry[1])] as const);
  });
  const envRemove = r.envRemove.map(key);
  const appearance = decodeProfileAppearance(r.appearance);
  if (appearance.fontFamily !== null) text(appearance.fontFamily);
  return Object.freeze({
    args: Object.freeze(args),
    cwd,
    env: Object.freeze(env),
    envRemove: Object.freeze(envRemove),
    appearance,
  });
}
export function decodeProfileEditorOffer(value: unknown): ProfileEditorOffer {
  const r = record(value, ["ticket", "shell", "config"]);
  if (
    typeof r.ticket !== "string" ||
    !isWorkspaceNonce(r.ticket) ||
    !isProfileKind(r.shell)
  )
    throw Error("profile.invalid");
  return Object.freeze({
    ticket: r.ticket,
    shell: r.shell as ProfileKind,
    config: decodeProfileConfig(r.config),
  });
}
/** Original-page editor with single-use saves and cancellation of its own late receipts. */
export class ProfileEditor {
  private offer: ProfileEditorOffer | undefined;
  private operation: AbortController | undefined;
  private retired = false;
  private expires = 0;
  constructor(
    private readonly nonce: string,
    private readonly signal: AbortSignal,
    private readonly native: ProfileEditorNative,
  ) {
    if (!isWorkspaceNonce(nonce)) throw Error("workspace.denied");
    signal.addEventListener("abort", this.close, { once: true });
    if (signal.aborted) this.close();
  }
  private cancel(ticket: string): void {
    try {
      void this.native.cancel(this.nonce, ticket).catch(() => {});
    } catch {}
  }
  close = (): void => {
    this.retired = true;
    this.operation?.abort();
    if (this.offer) this.cancel(this.offer.ticket);
    this.offer = undefined;
    this.signal.removeEventListener("abort", this.close);
  };
  async read(shell: ProfileKind): Promise<ProfileEditorOffer | null> {
    if (this.retired) return null;
    this.operation?.abort();
    if (this.offer) this.cancel(this.offer.ticket);
    this.offer = undefined;
    const operation = new AbortController();
    this.operation = operation;
    return new Promise((resolve) => {
      let done = false;
      const finish = (offer: ProfileEditorOffer | null) => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        operation.signal.removeEventListener("abort", abort);
        if (offer) {
          this.offer = offer;
          this.expires = Date.now() + 55000;
        }
        resolve(offer);
      };
      const abort = () => finish(null);
      const timer = setTimeout(abort, 5000);
      operation.signal.addEventListener("abort", abort, { once: true });
      const accept = (value: unknown) => {
        let offer: ProfileEditorOffer;
        try {
          offer = decodeProfileEditorOffer(value);
        } catch {
          if (
            value &&
            typeof value === "object" &&
            "ticket" in value &&
            typeof value.ticket === "string" &&
            isWorkspaceNonce(value.ticket)
          )
            this.cancel(value.ticket);
          finish(null);
          return;
        }
        if (
          done ||
          this.retired ||
          operation.signal.aborted ||
          offer.shell !== shell
        ) {
          this.cancel(offer.ticket);
          finish(null);
          return;
        }
        finish(offer);
      };
      try {
        void this.native.read(this.nonce, shell).then(accept, abort);
      } catch {
        abort();
      }
    });
  }
  async save(
    config: ProfileConfig,
  ): Promise<"saved" | "uncertain" | "cancelled" | "denied"> {
    if (this.retired || !this.offer || Date.now() >= this.expires)
      return "denied";
    const validated = decodeProfileConfig(config),
      offer = this.offer;
    this.offer = undefined;
    this.operation?.abort();
    const operation = new AbortController();
    this.operation = operation;
    return new Promise((resolve) => {
      let done = false;
      const finish = (result: "saved" | "uncertain" | "cancelled") => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        operation.signal.removeEventListener("abort", abort);
        this.cancel(offer.ticket);
        resolve(result);
      };
      const abort = () => finish("cancelled");
      const timer = setTimeout(() => finish("uncertain"), 5000);
      operation.signal.addEventListener("abort", abort, { once: true });
      try {
        void this.native.save(this.nonce, offer.ticket, validated).then(
          (v) => finish(v === null || v === undefined ? "saved" : "uncertain"),
          () => finish("uncertain"),
        );
      } catch {
        finish("uncertain");
      }
    });
  }
}
