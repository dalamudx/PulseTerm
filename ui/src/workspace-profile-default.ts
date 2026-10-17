import {
  type CustomProfileId,
  isCustomProfileId,
} from "./workspace-custom-profiles";
import type { ProfileKind } from "./workspace-profiles";
export type DefaultProfileId = ProfileKind | CustomProfileId;

const UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const KINDS = new Set(["bash", "zsh", "fish", "sh", "pwsh"]);
export type DefaultProfileOffer = Readonly<{
  ticket: string;
  selected: DefaultProfileId | null;
  available: boolean;
  offered: readonly DefaultProfileId[];
  names?: Readonly<Record<string, string>>;
}>;
export interface DefaultProfileNative {
  read(nonce: string): Promise<unknown>;
  save(
    nonce: string,
    ticket: string,
    selected: DefaultProfileId | null,
  ): Promise<unknown>;
  cancel(nonce: string, ticket: string): Promise<unknown>;
}
export function decodeDefaultProfile(value: unknown): DefaultProfileOffer {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("workspace.profiles_invalid");
  const v = value as Record<string, unknown>;
  if (
    Object.keys(v)
      .filter(
        (key) =>
          !["customOffered", "customSelected", "customName"].includes(key),
      )
      .sort()
      .join(",") !== "available,offered,selected,ticket" ||
    typeof v.ticket !== "string" ||
    !UUID.test(v.ticket) ||
    (v.selected !== null &&
      (typeof v.selected !== "string" || !KINDS.has(v.selected))) ||
    typeof v.available !== "boolean" ||
    !Array.isArray(v.offered) ||
    v.offered.length > 5 ||
    v.offered.some((kind) => typeof kind !== "string" || !KINDS.has(kind)) ||
    new Set(v.offered).size !== v.offered.length ||
    (v.customSelected === undefined &&
      v.available !== (v.selected !== null && v.offered.includes(v.selected)))
  )
    throw Error("workspace.profiles_invalid");
  const names: Record<string, string> = Object.create(null);
  const custom: CustomProfileId[] = [];
  const name = (value: unknown): value is string =>
    typeof value === "string" &&
    value.trim().length > 0 &&
    [...value].length <= 80 &&
    ![...value].some((c) => c.charCodeAt(0) < 32 || c.charCodeAt(0) === 127);
  if (v.customOffered !== undefined) {
    if (!Array.isArray(v.customOffered) || v.customOffered.length > 64)
      throw Error("workspace.profiles_invalid");
    for (const entry of v.customOffered) {
      if (
        !entry ||
        typeof entry !== "object" ||
        Array.isArray(entry) ||
        Object.keys(entry).sort().join(",") !== "id,name" ||
        typeof entry.id !== "string" ||
        !isCustomProfileId(entry.id) ||
        !name(entry.name) ||
        custom.includes(entry.id)
      )
        throw Error("workspace.profiles_invalid");
      custom.push(entry.id);
      names[entry.id] = entry.name;
    }
  }
  if (
    v.customSelected !== undefined &&
    (v.selected !== null ||
      typeof v.customSelected !== "string" ||
      !isCustomProfileId(v.customSelected) ||
      v.available !== custom.includes(v.customSelected))
  )
    throw Error("workspace.profiles_invalid");
  if (v.customName !== undefined) {
    if (
      typeof v.customSelected !== "string" ||
      !name(v.customName) ||
      (names[v.customSelected] !== undefined &&
        names[v.customSelected] !== v.customName)
    )
      throw Error("workspace.profiles_invalid");
    names[v.customSelected] = v.customName;
  }
  return Object.freeze({
    ticket: v.ticket,
    selected: (v.customSelected ?? v.selected) as DefaultProfileId | null,
    available: v.available,
    offered: Object.freeze([...(v.offered as ProfileKind[]), ...custom]),
    ...(Object.keys(names).length ? { names: Object.freeze(names) } : {}),
  });
}

/** One dialog lifetime; old receipts cannot republish a ticket or retry an uncertain save. */
export class DefaultProfileEditor {
  #offer: DefaultProfileOffer | null = null;
  #operation: AbortController | null = null;
  #retired = false;
  #expires = 0;
  constructor(
    private readonly nonce: string,
    private readonly signal: AbortSignal,
    private readonly native: DefaultProfileNative,
  ) {
    if (!UUID.test(nonce)) throw Error("workspace.denied");
    signal.addEventListener("abort", this.close, { once: true });
    if (signal.aborted) this.close();
  }
  get offer(): DefaultProfileOffer | null {
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
  async read(): Promise<DefaultProfileOffer | null> {
    if (this.#retired) return null;
    this.#operation?.abort();
    if (this.#offer) this.#cancel(this.#offer.ticket);
    this.#offer = null;
    const operation = new AbortController();
    this.#operation = operation;
    return new Promise((resolve) => {
      let done = false;
      const finish = (offer: DefaultProfileOffer | null) => {
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
        let offer: DefaultProfileOffer;
        try {
          offer = decodeDefaultProfile(value);
        } catch {
          if (
            value &&
            typeof value === "object" &&
            "ticket" in value &&
            typeof value.ticket === "string" &&
            UUID.test(value.ticket)
          )
            this.#cancel(value.ticket);
          finish(null);
          return;
        }
        if (done || this.#retired || operation.signal.aborted) {
          this.#cancel(offer.ticket);
          return;
        }
        finish(offer);
      };
      try {
        void this.native.read(this.nonce).then(accept, abort);
      } catch {
        abort();
      }
    });
  }
  async save(
    selected: DefaultProfileId | null,
  ): Promise<"saved" | "uncertain" | "cancelled" | "denied"> {
    const offer = this.offer;
    if (!offer || (selected !== null && !offer.offered.includes(selected)))
      return "denied";
    this.#offer = null; // one submit, even if the native result is lost
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
        void this.native.save(this.nonce, offer.ticket, selected).then(
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
