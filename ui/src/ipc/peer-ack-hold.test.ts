import { expect, it } from "vitest";
import { PeerAckHold } from "./peer-ack-hold";

it("unheld ACK proceeds, armed ACK only rejects when old view retires, never returns credit", async () => {
  const life = new AbortController();
  const hold = new PeerAckHold(life.signal);
  await hold.wait();
  hold.arm();
  let released = false;
  const waiting = hold.wait().then(() => {
    released = true;
  });
  const rejected = expect(waiting).rejects.toThrow("peer.ack_retired");
  await Promise.resolve();
  expect(released).toBe(false);
  expect(() => hold.arm()).toThrow("peer.ack_hold_unavailable");
  life.abort();
  await rejected;
  expect(released).toBe(false);
  await expect(hold.wait()).rejects.toThrow("peer.ack_retired");
});
it("cancelled old view cannot arm or unblock a new generation's independent hold", async () => {
  const old = new AbortController(),
    fresh = new AbortController();
  const first = new PeerAckHold(old.signal),
    next = new PeerAckHold(fresh.signal);
  first.arm();
  const rejected = expect(first.wait()).rejects.toThrow("peer.ack_retired");
  old.abort();
  await rejected;
  expect(() => first.arm()).toThrow();
  await next.wait();
  next.arm();
  const closed = expect(next.wait()).rejects.toThrow("peer.ack_retired");
  fresh.abort();
  await closed;
});
