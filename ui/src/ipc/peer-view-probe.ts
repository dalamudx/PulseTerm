// Fixed, readonly refreshed-peer surface. Backend chooses the session, never JS.
import { Channel } from "@tauri-apps/api/core";
import { commands } from "./gen/commands";
import { PeerAckHold } from "./peer-ack-hold";
import { digest } from "./session-probe";
import { TransferViewProbe } from "./transfer-view-probe";

let started = false;
export async function verifyPeerView(
  nonce: string,
  pageNonce: string,
  previousPageNonce?: string,
  keepLive = false,
): Promise<void> {
  if (started) throw new Error("peer.view_duplicate");
  started = true;
  const life = new AbortController();
  let retiring = false;
  const abort = () => {
    retiring = true;
    life.abort();
  };
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
  let booted = false;
  const heldAck = new PeerAckHold(life.signal);
  const peerWindow = window as Window & {
    __PULSE_PEER_HOLD_ACK__?: (
      run: string,
      page: string,
      generation: string,
    ) => Promise<void>;
  };
  const pauseAck = async (run: string, page: string, generation: string) => {
    if (
      !keepLive ||
      previousPageNonce !== undefined ||
      !booted ||
      life.signal.aborted ||
      run !== nonce ||
      page !== pageNonce
    )
      throw new Error("peer.ack_hold_forbidden");
    heldAck.arm();
    await commands.probePeerHoldReady(nonce, pageNonce, generation);
  };
  if (keepLive) peerWindow.__PULSE_PEER_HOLD_ACK__ = pauseAck;
  let bootReady!: () => void;
  const bootstrap = new Promise<void>((resolve) => {
    bootReady = resolve;
  });
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
          if (previousPageNonce)
            await rejected(
              commands.probePeerAck(
                nonce,
                previousPageNonce,
                generation,
                sequence,
              ),
            );
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
        await heldAck.wait();
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
        if (previousPageNonce)
          await rejected(
            commands.probePeerConfirm(
              nonce,
              previousPageNonce,
              generation,
              rendered,
            ),
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
      live: keepLive
        ? {
            ready: async () => {
              await commands.probePeerDone(nonce, pageNonce);
              booted = true;
              bootReady();
            },
            marker: async (generation, boundary, value) => {
              await commands.probePeerConfirm(nonce, pageNonce, generation, {
                columns: boundary.columns,
                rows: boundary.rows,
                revision: boundary.revision.toString(),
                parsedBytes: boundary.parsedBytes.toString(),
                digest: value,
              });
            },
          }
        : undefined,
      digest,
      failed: (error) => {
        if (keepLive && retiring && life.signal.aborted) return;
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
    const native = commands.probePeerView(nonce, pageNonce, channel);
    if (keepLive) {
      let navigation!: () => void;
      const retired = new Promise<void>((resolve) => {
        navigation = resolve;
      });
      const onRetired = () => navigation();
      life.signal.addEventListener("abort", onRetired, { once: true });
      const closed = native.then(
        () => {
          if (!booted && !life.signal.aborted)
            failure ??= new Error("peer.live_closed_before_bootstrap");
          if (booted) retiring = true;
          life.abort();
        },
        (error) => {
          failure ??= error;
          life.abort();
          throw error;
        },
      );
      try {
        await Promise.race([
          bootstrap,
          cancelled,
          finite,
          closed.then(() => {
            throw failure ?? new Error("peer.live_closed_before_bootstrap");
          }),
        ]);
        await Promise.race([retired, closed, finite]);
        if (failure) throw failure;
      } finally {
        life.signal.removeEventListener("abort", onRetired);
      }
    } else {
      await Promise.race([native, cancelled, finite]);
      await view.finish();
      if (failure) throw failure;
      if (life.signal.aborted) throw new Error("peer.view_cancelled");
      await commands.probePeerDone(nonce, pageNonce);
    }
  } finally {
    if (peerWindow.__PULSE_PEER_HOLD_ACK__ === pauseAck)
      delete peerWindow.__PULSE_PEER_HOLD_ACK__;
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
