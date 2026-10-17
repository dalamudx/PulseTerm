import { Terminal } from "@xterm/headless";
import type { ILink } from "@xterm/xterm";
import { afterEach, expect, it, vi } from "vitest";
import {
  endpointLink,
  reviewableLink,
  textLinkProvider,
} from "./terminal-links";

const terminals: Terminal[] = [];
afterEach(() => {
  for (const terminal of terminals.splice(0)) terminal.dispose();
});
async function terminal(text: string, cols = 64, rows = 4) {
  const t = new Terminal({
    cols,
    rows,
    scrollback: 100,
    allowProposedApi: true,
  });
  terminals.push(t);
  await new Promise<void>((resolve) => t.write(text, resolve));
  return t;
}
function links(
  t: Terminal,
  row: number,
  current = () => true,
  activate = vi.fn(),
) {
  let result: ILink[] | undefined;
  textLinkProvider(t, current, activate).provideLinks(row, (value) => {
    result = value;
  });
  return result;
}
it("projects an absolute IDN destination to an ASCII hostname and escaped path", () => {
  const url = reviewableLink("https://例子.测试/路径?q=1#part");
  expect(url?.hostname).toBe("xn--fsqu00a.xn--0zwm56d");
  expect(url?.pathname).toBe("/%E8%B7%AF%E5%BE%84");
});
it.each([
  "javascript:alert(1)",
  "file:///etc/passwd",
  "data:text/html,x",
  "ssh://host",
  "//example.test/path",
  "https://user:pass@example.test/",
  "https://good.test\\@evil.test/",
  "https://example.test/with space",
  `https://example.test/${String.fromCharCode(1)}`,
  `https://example.test/${String.fromCharCode(0x202e)}`,
  "https://e.test/" + "a".repeat(4096),
])("rejects unsupported, ambiguous or oversized target %s", (raw) => {
  expect(reviewableLink(raw)).toBeUndefined();
});
it("bounds the canonical UTF-8 target when the Unicode source itself fits", () => {
  expect(
    reviewableLink(`https://example.test/${"中".repeat(1000)}`),
  ).toBeUndefined();
});
it("maps wide and combining prefix cells exactly", async () => {
  const url = "https://e.test/a",
    t = await terminal("中e\u0301 " + url);
  expect(links(t, 1)?.map((l) => ({ text: l.text, range: l.range }))).toEqual([
    {
      text: url,
      range: { start: { x: 5, y: 1 }, end: { x: 4 + url.length, y: 1 } },
    },
  ]);
});
it("recognizes the complete target on any row of a soft-wrapped link", async () => {
  const url = "https://example.test/long/path",
    t = await terminal(url, 12, 4);
  for (const row of [1, 2, 3]) {
    const match = links(t, row)?.[0];
    expect(match?.text).toBe(url);
    expect(match?.range.start).toEqual({ x: 1, y: 1 });
    expect(match?.range.end.y).toBe(3);
  }
});
it("does not merge hard lines or include sentence punctuation", async () => {
  const t = await terminal("(https://one.test/a).\r\nhttps://two.test/(b)");
  expect(links(t, 1)?.[0]?.text).toBe("https://one.test/a");
  expect(links(t, 2)?.[0]?.text).toBe("https://two.test/(b)");
  const longTail = await terminal(
    `https://one.test/a${")".repeat(900)}`,
    1024,
    4,
  );
  const match = links(longTail, 1)?.[0];
  expect(match?.text).toBe("https://one.test/a");
  expect(match?.range.end.x).toBe("https://one.test/a".length);
});
it("refuses an over-budget wrapped chain instead of linking a prefix", async () => {
  const t = await terminal("https://e.test/" + "a".repeat(300), 8, 48);
  expect(links(t, 2)).toBeUndefined();
});
it("retired providers and previously returned links cannot activate", async () => {
  const t = await terminal("https://one.test"),
    activate = vi.fn();
  let current = true;
  const provider = textLinkProvider(t, () => current, activate);
  let result: ILink[] | undefined;
  provider.provideLinks(1, (value) => {
    result = value;
  });
  current = false;
  result?.[0]?.activate(
    { preventDefault: vi.fn() } as unknown as MouseEvent,
    "https://evil.test",
  );
  expect(activate).not.toHaveBeenCalled();
  expect(links(t, 1, () => false)).toBeUndefined();
});

it("recognizes strict IP endpoints without guessing non-web protocols", () => {
  for (const [raw, href] of [
    ["127.0.0.1:3000", "http://127.0.0.1:3000/"],
    [
      "192.0.2.1:80/status?q=中#part",
      "http://192.0.2.1/status?q=%E4%B8%AD#part",
    ],
    ["[2001:0db8::1]:8443/a", "http://[2001:db8::1]:8443/a"],
    ["[::1]:65535", "http://[::1]:65535/"],
    ["[::ffff:192.0.2.1]:8080", "http://[::ffff:c000:201]:8080/"],
  ] as const)
    expect(endpointLink(raw)?.href).toBe(href);
});
it("refuses ambiguous or invalid IP endpoint targets", () => {
  for (const raw of [
    "127.1:80",
    "127.000.0.1:80",
    "256.1.1.1:80",
    "0x7f000001:80",
    "127.0.0.1:0",
    "127.0.0.1:080",
    "127.0.0.1:65536",
    "127.0.0.1:123456",
    "127.0.0.1:80.evil",
    "127.0.0.1:80:90",
    "[not-ip]:80",
    "[1:2:3]:80",
    "[fe80::1%eth0]:80",
    "::1:80",
    "user@127.0.0.1:80",
    "127.0.0.1:80/with space",
    "127.0.0.1:80/\\evil",
    `127.0.0.1:80/${"中".repeat(1500)}`,
  ])
    expect(endpointLink(raw), raw).toBeUndefined();
});
it("maps wrapped IP endpoint cells and activates the reviewed HTTP target", async () => {
  const raw = "[2001:db8::1]:8080/path",
    t = await terminal(`中e\u0301 ${raw}`, 12, 4),
    activate = vi.fn();
  for (const row of [1, 2, 3]) {
    const match = links(t, row, () => true, activate)?.[0];
    expect(match?.text).toBe(raw);
    expect(match?.range.start).toEqual({ x: 5, y: 1 });
    expect(match?.range.end).toEqual({ x: 3, y: 3 });
    match?.activate(
      { preventDefault: vi.fn() } as unknown as MouseEvent,
      "javascript:ignored",
    );
  }
  expect(activate).toHaveBeenCalledTimes(3);
  expect(
    activate.mock.calls.every(
      (call) => call[1] === "http://[2001:db8::1]:8080/path",
    ),
  ).toBe(true);
});
it("does not link IP fragments inside other destinations or duplicate explicit URLs", async () => {
  const t = await terminal(
    "ssh://127.0.0.1:22 user@127.0.0.1:80 /127.0.0.1:80\r\nx127.0.0.1:80 127.0.0.1:80.evil 999.127.0.0.1:80\r\nhttps://127.0.0.1:443/path (127.0.0.1:8080).",
    120,
    4,
  );
  expect(links(t, 1)).toEqual([]);
  expect(links(t, 2)).toEqual([]);
  expect(links(t, 3)?.map((link) => link.text)).toEqual([
    "https://127.0.0.1:443/path",
    "127.0.0.1:8080",
  ]);
});
it("keeps endpoint activation within the existing retired-view and scan budgets", async () => {
  const t = await terminal("127.0.0.1:8080"),
    activate = vi.fn();
  let current = true;
  const match = links(t, 1, () => current, activate)?.[0];
  current = false;
  match?.activate(
    { preventDefault: vi.fn() } as unknown as MouseEvent,
    "http://evil.test",
  );
  expect(activate).not.toHaveBeenCalled();
  const long = await terminal("[2001:db8::1]:8080/" + "a".repeat(300), 8, 48);
  expect(links(long, 2)).toBeUndefined();
});

it("recognizes absolute file paths separately from web and endpoint targets", async () => {
  const t = await terminal(
    "/data/main.rs:42:7 C:\\work\\main.rs:12 D:/code/a.ts\r\nhttps://x.test/a /tmp/a (127.0.0.1:8080).",
    110,
    4,
  );
  const activate = vi.fn();
  const first = links(t, 1, () => true, activate) ?? [];
  expect(first.map((link) => link.text)).toEqual([
    "/data/main.rs:42:7",
    "C:\\work\\main.rs:12",
    "D:/code/a.ts",
  ]);
  for (const link of first)
    link.activate(
      { preventDefault: vi.fn() } as unknown as MouseEvent,
      "file:///ignored",
    );
  expect(activate.mock.calls.map((call) => call.slice(1))).toEqual(
    first.map((link) => [link.text, "file"]),
  );
  expect(links(t, 2)?.map((link) => link.text)).toEqual([
    "https://x.test/a",
    "/tmp/a",
    "127.0.0.1:8080",
  ]);
});
it("does not infer file paths from URI fragments, relative paths or quoted partial tokens", async () => {
  const t = await terminal(
    'file:///tmp/a ssh://host/tmp/a user@/tmp/a ~/tmp/a ./tmp/a ../tmp/a " /tmp/a"\r\n"/with space/a" "C:\\with space\\a" /tmp/a:0 C:relative',
    120,
    4,
  );
  expect(links(t, 1)).toEqual([]);
  expect(links(t, 2)?.map((link) => link.text)).toEqual([
    '"/with space/a"',
    '"C:\\with space\\a"',
  ]);
});
it("keeps wrapped wide file paths in cell coordinates and rejects retired activations", async () => {
  const raw = "/tmp/中文/main.rs:3:2",
    t = await terminal(`中e\u0301 ${raw}`, 12, 5),
    activate = vi.fn();
  let current = true;
  const found = links(t, 2, () => current, activate)?.[0];
  expect(found?.text).toBe(raw);
  expect(found?.range.start).toEqual({ x: 5, y: 1 });
  expect(found?.range.end.y).toBeGreaterThan(1);
  current = false;
  found?.activate({ preventDefault: vi.fn() } as unknown as MouseEvent, raw);
  expect(activate).not.toHaveBeenCalled();
});

it("preserves literal file punctuation and removes only an enclosing delimiter", async () => {
  const t = await terminal(
    "/tmp/file. (/tmp/main.rs:3:2). /tmp/a(b).ts",
    80,
    4,
  );
  expect(links(t, 1)?.map((link) => link.text)).toEqual([
    "/tmp/file.",
    "/tmp/main.rs:3:2",
    "/tmp/a(b).ts",
  ]);
});

it("recognizes complete quoted file tokens with independent source locations", async () => {
  const raws = [
    '"/work/my project/main.rs":12:3',
    "'C:\\Program Files\\code\\main.rs:9'",
    '"/tmp/$HOME/a file."',
  ];
  const t = await terminal(raws.join(" "), 160, 4),
    activate = vi.fn();
  const found = links(t, 1, () => true, activate);
  expect(found?.map((l) => l.text)).toEqual(raws);
  found?.[0]?.activate(
    { preventDefault: vi.fn() } as unknown as MouseEvent,
    "file:///different",
  );
  expect(activate).toHaveBeenCalledWith(expect.anything(), raws[0], "file");
});
it("never truncates incomplete, nested, adjacent or invalid quoted tokens into file targets", async () => {
  for (const raw of [
    '"/tmp/a file',
    '"/tmp/a file"tail',
    '"/tmp/a file":bad',
    '"/tmp/a file":0',
    '"/tmp/a file":1:2:3',
    'user@"/tmp/a file"',
    'file://"/tmp/a file"',
    "'prefix \"/tmp/a file\" suffix'",
    '" /tmp/a file"',
  ]) {
    const t = await terminal(raw, 120, 4);
    expect(links(t, 1), raw).toEqual([]);
  }
});
it("maps wrapped quoted wide paths including quotes and location suffix", async () => {
  const raw = '"/work/中文 project/e\u0301.rs":3:2',
    t = await terminal(`中e\u0301 ${raw}`, 12, 6),
    activate = vi.fn();
  let current = true;
  const found = links(t, 2, () => current, activate)?.[0];
  expect(found?.text).toBe(raw);
  expect(found?.range.start).toEqual({ x: 5, y: 1 });
  expect(found?.range.end.y).toBeGreaterThan(1);
  current = false;
  found?.activate({ preventDefault: vi.fn() } as unknown as MouseEvent, raw);
  expect(activate).not.toHaveBeenCalled();
});
