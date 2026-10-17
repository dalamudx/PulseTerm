import type { ProfileKind } from "./workspace-profiles";
import { isWorkspaceNonce } from "./workspace-readiness";

const KINDS = new Set(["bash", "zsh", "fish", "sh", "pwsh"]);
export type LifecycleAction = "delete" | "restore";
export type LifecycleOffer = Readonly<{
  ticket: string;
  shell: ProfileKind;
  action: LifecycleAction;
  isDefault: boolean;
}>;
export interface ProfileLifecycleNative {
  deleted(nonce: string): Promise<unknown>;
  prepare(
    nonce: string,
    shell: ProfileKind,
    action: LifecycleAction,
  ): Promise<unknown>;
  commit(nonce: string, ticket: string): Promise<unknown>;
  cancel(nonce: string, ticket: string): Promise<unknown>;
}
export function decodeDeletedProfiles(value: unknown): readonly ProfileKind[] {
  if (
    !Array.isArray(value) ||
    value.length > 5 ||
    value.some((v) => typeof v !== "string" || !KINDS.has(v)) ||
    new Set(value).size !== value.length
  )
    throw Error("workspace.profiles_invalid");
  return Object.freeze([...value]) as readonly ProfileKind[];
}
export function decodeLifecycleOffer(value: unknown): LifecycleOffer {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("workspace.profiles_invalid");
  const v = value as Record<string, unknown>;
  if (
    Object.keys(v).sort().join(",") !== "action,isDefault,shell,ticket" ||
    typeof v.ticket !== "string" ||
    !isWorkspaceNonce(v.ticket) ||
    typeof v.shell !== "string" ||
    !KINDS.has(v.shell) ||
    (v.action !== "delete" && v.action !== "restore") ||
    typeof v.isDefault !== "boolean"
  )
    throw Error("workspace.profiles_invalid");
  return Object.freeze({
    ticket: v.ticket,
    shell: v.shell as ProfileKind,
    action: v.action,
    isDefault: v.isDefault,
  });
}
/** One fixed operation; late prepare receipts only cancel their own ticket. */
export class ProfileLifecycleEditor {
  #offer: LifecycleOffer | null = null;
  #operation: AbortController | null = null;
  #retired = false;
  #expires = 0;
  constructor(
    private readonly nonce: string,
    private readonly signal: AbortSignal,
    private readonly native: ProfileLifecycleNative,
  ) {
    if (!isWorkspaceNonce(nonce)) throw Error("workspace.denied");
    signal.addEventListener("abort", this.close, { once: true });
    if (signal.aborted) this.close();
  }
  get offer(): LifecycleOffer | null {
    return this.#retired || Date.now() >= this.#expires ? null : this.#offer;
  }
  #cancel(ticket: string): void {
    try {
      void this.native.cancel(this.nonce, ticket).catch(() => {});
    } catch {}
  }
  close = (): void => {
    this.#retired = true;
    this.#operation?.abort();
    if (this.#offer) this.#cancel(this.#offer.ticket);
    this.#offer = null;
    this.signal.removeEventListener("abort", this.close);
  };
  async prepare(
    shell: ProfileKind,
    action: LifecycleAction,
  ): Promise<LifecycleOffer | null> {
    if (this.#retired) return null;
    this.#operation?.abort();
    if (this.#offer) this.#cancel(this.#offer.ticket);
    this.#offer = null;
    const operation = new AbortController();
    this.#operation = operation;
    return new Promise((resolve) => {
      let done = false;
      const finish = (offer: LifecycleOffer | null) => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        operation.signal.removeEventListener("abort", abort);
        if (offer) {
          this.#offer = offer;
          this.#expires = Date.now() + 55000;
        }
        resolve(offer);
      };
      const abort = () => finish(null);
      const timer = setTimeout(abort, 5000);
      operation.signal.addEventListener("abort", abort, { once: true });
      const accept = (value: unknown) => {
        let offer: LifecycleOffer;
        try {
          offer = decodeLifecycleOffer(value);
        } catch {
          if (
            value &&
            typeof value === "object" &&
            "ticket" in value &&
            typeof value.ticket === "string" &&
            isWorkspaceNonce(value.ticket)
          )
            this.#cancel(value.ticket);
          finish(null);
          return;
        }
        if (
          done ||
          this.#retired ||
          operation.signal.aborted ||
          offer.shell !== shell ||
          offer.action !== action
        ) {
          this.#cancel(offer.ticket);
          finish(null);
          return;
        }
        finish(offer);
      };
      try {
        void this.native.prepare(this.nonce, shell, action).then(accept, abort);
      } catch {
        abort();
      }
    });
  }
  async commit(): Promise<"saved" | "uncertain" | "cancelled" | "denied"> {
    const offer = this.offer;
    if (!offer) return "denied";
    this.#offer = null;
    this.#operation?.abort();
    const operation = new AbortController();
    this.#operation = operation;
    return new Promise((resolve) => {
      let done = false;
      const finish = (status: "saved" | "uncertain" | "cancelled") => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        operation.signal.removeEventListener("abort", abort);
        this.#cancel(offer.ticket);
        resolve(status);
      };
      const abort = () => finish("cancelled");
      const timer = setTimeout(() => finish("uncertain"), 5000);
      operation.signal.addEventListener("abort", abort, { once: true });
      try {
        void this.native.commit(this.nonce, offer.ticket).then(
          (value) =>
            finish(
              value === null || value === undefined ? "saved" : "uncertain",
            ),
          () => finish("uncertain"),
        );
      } catch {
        finish("uncertain");
      }
    });
  }
}
