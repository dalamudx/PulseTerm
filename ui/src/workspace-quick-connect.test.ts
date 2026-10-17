import { expect, it } from "vitest";
import {
  matchingQuickHosts,
  parseQuickTarget,
} from "./workspace-quick-connect";

it("previews DNS, custom ports, usernames and bracketed IPv6 as metadata", () => {
  expect(parseQuickTarget("ssh://user@EXAMPLE.COM:2222")).toEqual({
    label: "example.com",
    host: "example.com",
    port: 2222,
    username: "user",
  });
  expect(parseQuickTarget("user@[2001:0DB8::1]", "IPv6")).toEqual({
    label: "IPv6",
    host: "2001:db8::1",
    port: 22,
    username: "user",
  });
  expect(parseQuickTarget("user@example.com@localhost").username).toBe(
    "user@example.com",
  );
});
it("rejects passwords, raw URLs, path/query syntax, ambiguous IPv6 and hidden controls", () => {
  for (const address of [
    "user:secret@host",
    "rdp://user@host",
    "user@host/path",
    "user@host?cmd=whoami",
    "user@host#fragment",
    "user@host:0",
    "user@host:65536",
    "user@host:",
    "user@2001:db8::1",
    "user@[fe80::1%eth0]",
    "user@host.",
    "user@-host",
    "user@a..b",
    "user@host\n",
    "user\u202e@host",
    "host",
    "user@",
    "user@[localhost]",
  ]) {
    expect(() => parseQuickTarget(address)).toThrow();
  }
});
it("matches exact saved users and ports including equivalent IPv6 without choosing duplicate IDs", () => {
  const target = parseQuickTarget("user@[::ffff:192.0.2.1]:2222");
  const hosts = [
    { ...target, id: "one", host: "::ffff:192.0.2.1" },
    { ...target, id: "two", username: "other" },
    { ...target, id: "three", port: 22 },
    { ...target, id: "four" },
  ];
  expect(matchingQuickHosts(hosts, target).map((h) => h.id)).toEqual([
    "one",
    "four",
  ]);
});
