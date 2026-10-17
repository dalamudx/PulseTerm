import { invoke } from "@tauri-apps/api/core";
import type { HostRpc } from "./workspace-hosts";

const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
function bad(): never {
  throw Error("fingerprint.invalid");
}
function record(
  input: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!input || typeof input !== "object" || Array.isArray(input)) return bad();
  const r = input as Record<string, unknown>;
  if (
    Object.keys(r).length !== keys.length ||
    keys.some((key) => !Object.hasOwn(r, key))
  )
    return bad();
  return r;
}
function text(input: unknown, max: number): string {
  if (
    typeof input !== "string" ||
    input.length === 0 ||
    new TextEncoder().encode(input).length > max ||
    Array.from(input).some((char) => {
      const code = char.charCodeAt(0);
      return code < 32 || (code >= 127 && code <= 159);
    })
  )
    return bad();
  return input;
}
export type Fingerprint = Readonly<{
  version: 1;
  token: string | null;
  label: string;
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
  status: "unknown" | "already_pinned" | "changed";
  source: "application_only";
}>;
export function decodeFingerprint(
  input: unknown,
  expected: string,
): Fingerprint {
  if (!uuid4.test(expected)) return bad();
  const r = record(input, [
    "version",
    "token",
    "label",
    "host",
    "port",
    "algorithm",
    "fingerprint",
    "status",
    "source",
  ]);
  if (
    r.version !== 1 ||
    r.source !== "application_only" ||
    !Number.isInteger(r.port) ||
    typeof r.port !== "number" ||
    r.port < 1 ||
    r.port > 65535
  )
    return bad();
  if (
    r.status !== "unknown" &&
    r.status !== "already_pinned" &&
    r.status !== "changed"
  )
    return bad();
  if (r.status === "unknown" ? r.token !== expected : r.token !== null)
    return bad();
  const label = text(r.label, 256),
    host = text(r.host, 253),
    algorithm = text(r.algorithm, 128),
    fingerprint = text(r.fingerprint, 50);
  if (
    host !== host.toLowerCase() ||
    !/^[a-z0-9:.%_-]+$/.test(host) ||
    !/^[a-zA-Z0-9@._+-]+$/.test(algorithm) ||
    !/^SHA256:[A-Za-z0-9+/]{43}$/.test(fingerprint)
  )
    return bad();
  return Object.freeze({
    version: 1,
    token: r.status === "unknown" ? expected : null,
    label,
    host,
    port: r.port,
    algorithm,
    fingerprint,
    status: r.status,
    source: "application_only",
  });
}
function ticket(input: unknown): string {
  const r = record(input, ["version", "token"]);
  if (r.version !== 1 || typeof r.token !== "string" || !uuid4.test(r.token))
    return bad();
  return r.token;
}
function revision(input: unknown): string {
  const r = record(input, ["version", "revision"]);
  if (
    r.version !== 1 ||
    typeof r.revision !== "string" ||
    !/^[1-9][0-9]{0,18}$/.test(r.revision) ||
    BigInt(r.revision) > 9223372036854775807n
  )
    return bad();
  return r.revision;
}
async function bounded<T>(
  pending: Promise<T>,
  signal: AbortSignal,
  cancel: () => void,
  ms: number,
): Promise<T> {
  if (signal.aborted) {
    cancel();
    throw Error("fingerprint.cancelled");
  }
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  const stopped = new Promise<never>((_, reject) => {
    abort = () => {
      cancel();
      reject(Error("fingerprint.cancelled"));
    };
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => {
      cancel();
      reject(Error("fingerprint.timeout"));
    }, ms);
  });
  try {
    const value = await Promise.race([pending, stopped]);
    if (signal.aborted) {
      cancel();
      throw Error("fingerprint.cancelled");
    }
    return value;
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
/** Drawing metadata only. The backend independently verifies source, original page and action. */
export class FingerprintTicket {
  private stage: "ready" | "observing" | "approval" | "confirming" | "closed" =
    "ready";
  private cancelled = false;
  private readonly abort = () => this.cancel();
  private constructor(
    private readonly nonce: string,
    private readonly token: string,
    private readonly signal: AbortSignal,
    private readonly rpc: HostRpc,
  ) {
    signal.addEventListener("abort", this.abort, { once: true });
  }
  static async prepare(
    nonce: string,
    host: string,
    signal: AbortSignal,
    rpc: HostRpc = invoke,
  ): Promise<FingerprintTicket> {
    if (!uuid4.test(nonce) || !uuid7.test(host)) return bad();
    if (signal.aborted) throw Error("fingerprint.cancelled");
    let accepting = true,
      received: string | undefined,
      cancelled = false;
    const cleanup = () => {
      accepting = false;
      if (received && !cancelled) {
        cancelled = true;
        void rpc("workspace_ssh_fingerprint_cancel", {
          nonce,
          token: received,
        }).catch(() => {});
      }
    };
    const pending = rpc("workspace_ssh_fingerprint_prepare", {
      nonce,
      host,
    }).then((raw) => {
      if (
        raw &&
        typeof raw === "object" &&
        "token" in raw &&
        typeof raw.token === "string" &&
        uuid4.test(raw.token)
      )
        received = raw.token;
      if (!accepting || signal.aborted) cleanup();
      return ticket(raw);
    });
    try {
      const token = await bounded(pending, signal, cleanup, 5000);
      return new FingerprintTicket(nonce, token, signal, rpc);
    } catch (error) {
      cleanup();
      throw error;
    }
  }
  async observe(): Promise<Fingerprint> {
    if (this.stage !== "ready" || this.signal.aborted)
      throw Error("fingerprint.cancelled");
    this.stage = "observing";
    try {
      const raw = await bounded(
        this.rpc("workspace_ssh_fingerprint_observe", {
          nonce: this.nonce,
          token: this.token,
        }),
        this.signal,
        () => this.cancel(),
        12000,
      );
      if (this.isClosed()) throw Error("fingerprint.cancelled");
      const value = decodeFingerprint(raw, this.token);
      if (value.status === "unknown") this.stage = "approval";
      else this.finish();
      return value;
    } catch (error) {
      this.cancel();
      throw error;
    }
  }
  async confirm(): Promise<string> {
    if (this.stage !== "approval" || this.signal.aborted)
      throw Error("fingerprint.not_approved");
    this.stage = "confirming";
    try {
      const raw = await bounded(
        this.rpc("workspace_ssh_fingerprint_confirm", {
          nonce: this.nonce,
          token: this.token,
        }),
        this.signal,
        () => this.cancel(),
        5000,
      );
      if (this.isClosed()) throw Error("fingerprint.cancelled");
      const value = revision(raw);
      this.finish();
      return value;
    } catch (error) {
      this.cancel();
      throw error;
    }
  }
  cancel(): void {
    if (this.stage === "closed") return;
    this.finish();
    if (this.cancelled) return;
    this.cancelled = true;
    void this.rpc("workspace_ssh_fingerprint_cancel", {
      nonce: this.nonce,
      token: this.token,
    }).catch(() => {});
  }
  private isClosed(): boolean {
    return this.stage === "closed";
  }
  private finish(): void {
    this.stage = "closed";
    this.signal.removeEventListener("abort", this.abort);
  }
}
