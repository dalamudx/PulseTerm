import { readFileSync } from "node:fs";
import { expect, it, vi } from "vitest";
import { installPeerBootstrap, type PeerBootstrapHost } from "./peer-bootstrap";

// Execute the actual trusted Rust eval fragment, not a second readiness implementation.
const source = readFileSync(
  new URL(
    "../../../apps/desktop/src-tauri/src/window_reload_probe.rs",
    import.meta.url,
  ),
  "utf8",
);
const script = source.match(/const WAIT_BOOTSTRAP: &str = "([^"]+)";/)?.[1];
if (!script) throw new Error("test.missing_actual_bootstrap_script");
const wait = new Function(
  "window",
  `return(async()=>{${script}return window.__PULSE_PEER_VIEW__;})()`,
) as (host: PeerBootstrapHost) => Promise<unknown>;
it("actual native Finished script waits for hook installation, publishes it before resolving and removes resolver", async () => {
  const host: PeerBootstrapHost = {};
  const bootstrap = vi.fn(async () => {});
  let completed = false;
  const pending = wait(host).then((value) => {
    completed = true;
    return value;
  });
  await Promise.resolve();
  expect(completed).toBe(false);
  expect(host.__PULSE_PEER_VIEW_READY__).toBeTypeOf("function");
  installPeerBootstrap(host, bootstrap);
  await expect(pending).resolves.toBe(bootstrap);
  expect(host.__PULSE_PEER_VIEW_READY__).toBeUndefined();
  expect(bootstrap).not.toHaveBeenCalled();
});
it("hook installed before native Finished is observed immediately without leaving a resolver", async () => {
  const host: PeerBootstrapHost = {};
  const bootstrap = vi.fn(async () => {});
  installPeerBootstrap(host, bootstrap);
  await expect(wait(host)).resolves.toBe(bootstrap);
  expect(host.__PULSE_PEER_VIEW_READY__).toBeUndefined();
});
it("another document cannot resolve a pending old hook and duplicate installation cannot replace it", async () => {
  const old: PeerBootstrapHost = {},
    fresh: PeerBootstrapHost = {};
  let oldDone = false;
  const pending = wait(old).then(() => {
    oldDone = true;
  });
  const first = vi.fn(async () => {}),
    second = vi.fn(async () => {});
  installPeerBootstrap(fresh, second);
  await Promise.resolve();
  expect(oldDone).toBe(false);
  installPeerBootstrap(old, first);
  await pending;
  expect(() => installPeerBootstrap(old, second)).toThrow(
    "peer.bootstrap_duplicate",
  );
  expect(old.__PULSE_PEER_VIEW__).toBe(first);
});
