import { invoke } from "@tauri-apps/api/core";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const valid = (value: string) =>
  uuid.test(value) && value !== "00000000-0000-0000-0000-000000000000";
function metadata(value: unknown, keys: string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw Error("identity.key_protocol");
  const row = value as Record<string, unknown>;
  if (
    row.version !== 1 ||
    Object.keys(row).length !== keys.length ||
    keys.some((key) => !Object.hasOwn(row, key))
  )
    throw Error("identity.key_protocol");
  return row;
}
export async function keyImportAvailable(
  nonce: string,
  signal: AbortSignal,
): Promise<boolean> {
  if (!valid(nonce) || signal.aborted) throw Error("identity.key_cancelled");
  let abort = () => {};
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = performance.now() + 5000;
  const stopped = new Promise<never>((_, reject) => {
    abort = () => reject(Error("identity.key_cancelled"));
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => reject(Error("identity.key_uncertain")), 5000);
  });
  try {
    const row = metadata(
      await Promise.race([
        invoke<unknown>("workspace_identity_key_available", { nonce }),
        stopped,
      ]),
      ["version", "enabled"],
    );
    if (signal.aborted || performance.now() >= deadline)
      throw Error("identity.key_cancelled");
    if (typeof row.enabled !== "boolean") throw Error("identity.key_protocol");
    return row.enabled;
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
/** Owns byte buffers; clears reachable copies, but cannot wipe immutable JS strings or IPC copies. */
export async function saveIdentityKey(
  nonce: string,
  identity: string,
  bytes: Uint8Array,
  passphrase: Uint8Array | undefined,
  signal: AbortSignal,
): Promise<void> {
  const controller = new AbortController();
  let ticket: string | undefined;
  let keyWire: number[] = [],
    passWire: number[] | undefined;
  let uncertain = false,
    sentCancel = false;
  const deadline = performance.now() + 5000;
  const wipe = () => {
    bytes.fill(0);
    passphrase?.fill(0);
    keyWire.fill(0);
    passWire?.fill(0);
  };
  const cancel = () => {
    if (ticket && !sentCancel) {
      sentCancel = true;
      void invoke("workspace_identity_key_cancel", { nonce, ticket }).catch(
        () => {},
      );
    }
  };
  const relay = () => controller.abort();
  signal.addEventListener("abort", relay, { once: true });
  let timer: ReturnType<typeof setTimeout> | undefined;
  let rejectAbort = () => {};
  const current = () => {
    if (performance.now() >= deadline) {
      uncertain = true;
      controller.abort();
    }
    return !controller.signal.aborted;
  };
  try {
    if (
      !valid(nonce) ||
      !valid(identity) ||
      bytes.byteLength < 1 ||
      bytes.byteLength > 65536 ||
      (passphrase !== undefined &&
        (passphrase.byteLength < 1 || passphrase.byteLength > 4096)) ||
      signal.aborted
    )
      throw Error("identity.key_invalid");
    const aborted = new Promise<never>((_, reject) => {
      rejectAbort = () => {
        wipe();
        cancel();
        reject(
          Error(
            uncertain ? "identity.key_uncertain" : "identity.key_cancelled",
          ),
        );
      };
      controller.signal.addEventListener("abort", rejectAbort, { once: true });
    });
    timer = setTimeout(() => {
      uncertain = true;
      controller.abort();
    }, 5000);
    const work = (async () => {
      const row = metadata(
        await invoke<unknown>("workspace_identity_key_prepare", {
          nonce,
          identity,
        }),
        ["version", "ticket"],
      );
      if (typeof row.ticket !== "string" || !valid(row.ticket))
        throw Error("identity.key_protocol");
      ticket = row.ticket;
      if (!current()) {
        cancel();
        throw Error("identity.key_cancelled");
      }
      keyWire = Array.from(bytes);
      passWire = passphrase === undefined ? undefined : Array.from(passphrase);
      const reply = invoke<unknown>("workspace_identity_key_submit", {
        nonce,
        ticket,
        privateKey: keyWire,
        passphrase: passWire ?? null,
      });
      bytes.fill(0);
      passphrase?.fill(0);
      metadata(await reply, ["version"]);
      if (!current()) throw Error("identity.key_cancelled");
    })();
    await Promise.race([work, aborted]);
  } catch (error) {
    cancel();
    const allowed = new Set([
      "identity.key_protocol",
      "identity.key_invalid",
      "identity.key_cancelled",
      "identity.key_uncertain",
    ]);
    throw Error(
      error instanceof Error && allowed.has(error.message)
        ? error.message
        : "identity.key_failed",
    );
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    wipe();
    signal.removeEventListener("abort", relay);
    controller.signal.removeEventListener("abort", rejectAbort);
  }
}
