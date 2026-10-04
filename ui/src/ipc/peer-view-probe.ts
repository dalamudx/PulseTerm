// Fixed, readonly refreshed-peer surface. Backend chooses the session, never JS.
import { Channel } from "@tauri-apps/api/core";
import { commands } from "./gen/commands";
import { digest } from "./session-probe";
import { TransferViewProbe } from "./transfer-view-probe";

let started = false;
export async function verifyPeerView(
  nonce: string,
  pageNonce: string,
): Promise<void> {
  if (started) throw new Error("peer.view_duplicate");
  started = true;
  const life = new AbortController();
  const abort = () => life.abort();
  window.addEventListener("pagehide", abort, { once: true });
  const host = document.createElement("div");
  host.setAttribute("aria-label", "只读页面恢复验证");
  host.style.width = "480px";
  host.style.height = "208px";
  const app = document.getElementById("root");
  const wasHidden = app?.hidden;
  if (app) app.hidden = true;
  document.body.append(host);
  const channel = new Channel<ArrayBuffer>();
  let failure: unknown;
  let view: TransferViewProbe | undefined;
  let deadline: ReturnType<typeof setTimeout> | undefined;
  let stop: () => void = () => {};
  let challenged = false;
  const wrongPage = (pageNonce[0] === "0" ? "1" : "0") + pageNonce.slice(1);
  const rejected = async (job: Promise<unknown>) => {
    try {
      await job;
    } catch {
      return;
    }
    throw new Error("peer.invalid_control_admitted");
  };
  try {
    const version = await commands.probeSessionProtocol();
    if (version !== 1 && version !== 2) throw new Error("peer.view_protocol");
    view = new TransferViewProbe({
      version,
      host,
      signal: life.signal,
      ack: async (generation, sequence) => {
        if (!challenged) {
          challenged = true;
          await rejected(
            commands.probePeerAck(nonce, wrongPage, generation, sequence),
          );
          await rejected(
            commands.probePeerAck(
              nonce,
              pageNonce,
              (BigInt(generation) + 1n).toString(),
              sequence,
            ),
          );
        }
        await commands.probePeerAck(nonce, pageNonce, generation, sequence);
      },
      confirm: async (generation, boundary, value) => {
        const rendered = {
          columns: boundary.columns,
          rows: boundary.rows,
          revision: boundary.revision.toString(),
          parsedBytes: boundary.parsedBytes.toString(),
          digest: value,
        };
        await rejected(
          commands.probePeerConfirm(nonce, wrongPage, generation, rendered),
        );
        await commands.probePeerConfirm(nonce, pageNonce, generation, {
          columns: boundary.columns,
          rows: boundary.rows,
          revision: boundary.revision.toString(),
          parsedBytes: boundary.parsedBytes.toString(),
          digest: value,
        });
        await rejected(
          commands.probePeerConfirm(nonce, pageNonce, generation, rendered),
        );
      },
      digest,
      failed: (error) => {
        failure ??= error;
        life.abort();
      },
    });
    channel.onmessage = (data) => view?.receive(data);
    const cancelled = new Promise<never>((_resolve, reject) => {
      const stopped = () => reject(failure ?? new Error("peer.view_cancelled"));
      life.signal.addEventListener("abort", stopped, { once: true });
      stop = () => life.signal.removeEventListener("abort", stopped);
      if (life.signal.aborted) stopped();
    });
    const finite = new Promise<never>((_resolve, reject) => {
      deadline = setTimeout(
        () => reject(new Error("peer.view_deadline")),
        7000,
      );
    });
    await Promise.race([
      commands.probePeerView(nonce, pageNonce, channel),
      cancelled,
      finite,
    ]);
    await view.finish();
    if (failure) throw failure;
    if (life.signal.aborted) throw new Error("peer.view_cancelled");
    await commands.probePeerDone(nonce, pageNonce);
  } finally {
    clearTimeout(deadline);
    stop();
    channel.onmessage = () => {};
    life.abort();
    view?.dispose();
    host.remove();
    if (app) app.hidden = wasHidden ?? false;
    window.removeEventListener("pagehide", abort);
  }
}
