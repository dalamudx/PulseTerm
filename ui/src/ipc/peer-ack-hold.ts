// Developer-only cooperative ACK pause. Pending ACK is NEVER resolved on teardown.
export class PeerAckHold {
  private held = false;
  constructor(private readonly signal: AbortSignal) {}
  arm() {
    if (this.signal.aborted || this.held)
      throw new Error("peer.ack_hold_unavailable");
    this.held = true;
  }
  async wait(): Promise<void> {
    if (this.signal.aborted) throw new Error("peer.ack_retired");
    if (!this.held) return;
    await new Promise<never>((_resolve, reject) => {
      const abort = () => {
        this.signal.removeEventListener("abort", abort);
        reject(new Error("peer.ack_retired"));
      };
      this.signal.addEventListener("abort", abort, { once: true });
      if (this.signal.aborted) abort();
    });
  }
}
