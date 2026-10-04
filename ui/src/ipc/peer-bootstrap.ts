// Fixed local peer probe only; readiness never grants a nonce or terminal authority.
export type PeerBootstrap = (
  nonce: string,
  pageNonce: string,
  previousPageNonce?: string,
  initialLive?: boolean,
) => Promise<void>;
export interface PeerBootstrapHost {
  __PULSE_PEER_VIEW__?: PeerBootstrap;
  __PULSE_PEER_VIEW_READY__?: () => void;
}
export function installPeerBootstrap(
  host: object,
  bootstrap: PeerBootstrap,
): void {
  const target = host as PeerBootstrapHost;
  if (target.__PULSE_PEER_VIEW__) throw new Error("peer.bootstrap_duplicate");
  target.__PULSE_PEER_VIEW__ = bootstrap;
  const ready = target.__PULSE_PEER_VIEW_READY__;
  delete target.__PULSE_PEER_VIEW_READY__;
  ready?.();
}
