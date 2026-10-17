import { invoke } from "@tauri-apps/api/core";

const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const nil = "00000000-0000-0000-0000-000000000000";
const valid = (value: string) => uuid.test(value) && value !== nil;
function metadata(value: unknown, keys: string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw Error("identity.password_protocol");
  const row = value as Record<string, unknown>;
  if (
    row.version !== 1 ||
    Object.keys(row).length !== keys.length ||
    keys.some((key) => !Object.hasOwn(row, key))
  )
    throw Error("identity.password_protocol");
  return row;
}
/** Receive-owned bytes only; JS strings/upstream IPC copies cannot be completely wiped here. Never retries an uncertain write. */
export async function saveIdentityPassword(
  nonce: string,
  identity: string,
  bytes: Uint8Array,
  signal: AbortSignal,
): Promise<void> {
  const controller = new AbortController();
  let ticket: string | undefined;
  let wire: number[] = [];
  let uncertain = false;
  let sentCancel = false;
  const deadline = performance.now() + 5000;
  const wipe = () => {
    bytes.fill(0);
    wire.fill(0);
  };
  const cancel = () => {
    if (ticket && !sentCancel) {
      sentCancel = true;
      void invoke("workspace_identity_password_cancel", {
        nonce,
        ticket,
      }).catch(() => {});
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
      bytes.byteLength > 4096 ||
      signal.aborted
    )
      throw Error("identity.password_invalid");
    const aborted = new Promise<never>((_, reject) => {
      rejectAbort = () => {
        wipe();
        cancel();
        reject(
          Error(
            uncertain
              ? "identity.password_uncertain"
              : "identity.password_cancelled",
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
        await invoke<unknown>("workspace_identity_password_prepare", {
          nonce,
          identity,
        }),
        ["version", "ticket"],
      );
      if (typeof row.ticket !== "string" || !valid(row.ticket))
        throw Error("identity.password_protocol");
      ticket = row.ticket;
      if (!current()) {
        cancel();
        throw Error("identity.password_cancelled");
      }
      wire = Array.from(bytes);
      const reply = invoke<unknown>("workspace_identity_password_submit", {
        nonce,
        ticket,
        password: wire,
      });
      bytes.fill(0);
      metadata(await reply, ["version"]);
      if (!current()) throw Error("identity.password_cancelled");
    })();
    await Promise.race([work, aborted]);
  } catch (error) {
    cancel();
    const allowed = new Set([
      "identity.password_protocol",
      "identity.password_invalid",
      "identity.password_cancelled",
      "identity.password_uncertain",
    ]);
    throw Error(
      error instanceof Error && allowed.has(error.message)
        ? error.message
        : "identity.password_failed",
    );
  } finally {
    if (timer !== undefined) clearTimeout(timer);
    wipe();
    signal.removeEventListener("abort", relay);
    controller.signal.removeEventListener("abort", rejectAbort);
  }
}
