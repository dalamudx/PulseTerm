import { expect, it } from "vitest";
import { reviewableFilePath } from "./terminal-file-links";

it("projects absolute terminal paths and source positions without resolving them locally", () => {
  expect(reviewableFilePath("/data/中文/e\u0301.rs:42:7")).toEqual({
    path: "/data/中文/e\u0301.rs",
    style: "unix",
    line: 42,
    column: 7,
  });
  expect(reviewableFilePath("C:\\work\\main.rs:12")).toEqual({
    path: "C:\\work\\main.rs",
    style: "windows",
    line: 12,
  });
  expect(reviewableFilePath("D:/code/main.rs")).toEqual({
    path: "D:/code/main.rs",
    style: "windows",
  });
  expect(reviewableFilePath("/etc/hosts")).toEqual({
    path: "/etc/hosts",
    style: "unix",
  });
});
it("refuses ambiguous relative, network, unsafe or over-budget path targets", () => {
  for (const raw of [
    "/",
    "C:\\",
    "src/main.rs:1",
    "~/main.rs",
    "./main.rs",
    "../main.rs",
    "//server/share/main.rs",
    "\\\\server\\share\\main.rs",
    "C:main.rs",
    "file:///etc/passwd",
    "https://x.test/a",
    "/a/../b",
    "/a/./b",
    "/a//b",
    "/with space/a",
    "/a\n/b",
    "/a\u202Eb",
    "/a\u200Bb",
    "/file:0",
    "/file:1:0",
    "/file:2147483648",
    "/127.0.0.1:80",
    "C:\\a?.rs",
    "C:\\a|b",
    "/" + "中".repeat(1500),
  ])
    expect(reviewableFilePath(raw), raw).toBeUndefined();
});

it("keeps quoted Unix and Windows spaces literal and separates inner or outer locations", () => {
  for (const raw of [
    '"/work/my project/main.rs":12:3',
    "'/work/my project/main.rs:12:3'",
  ])
    expect(reviewableFilePath(raw)).toEqual({
      path: "/work/my project/main.rs",
      style: "unix",
      line: 12,
      column: 3,
    });
  expect(reviewableFilePath('"C:\\Program Files\\code\\main.rs":9')).toEqual({
    path: "C:\\Program Files\\code\\main.rs",
    style: "windows",
    line: 9,
  });
  expect(reviewableFilePath("'D:/work project/main.rs:9:2'")).toEqual({
    path: "D:/work project/main.rs",
    style: "windows",
    line: 9,
    column: 2,
  });
  expect(reviewableFilePath(`"/work/$HOME/\${name}/a file."`)).toEqual({
    path: `/work/$HOME/\${name}/a file.`,
    style: "unix",
  });
  expect(reviewableFilePath('"/work/name:5":7')).toEqual({
    path: "/work/name:5",
    style: "unix",
    line: 7,
  });
});
it("rejects incomplete or unsafe quoted targets without shell interpretation", () => {
  for (const raw of [
    '"/work/a file',
    "'/work/a file\"",
    '"/work/a file"junk',
    '"/work/a file":0',
    '"/work/a file":1:0',
    '"/work/a file":2147483648',
    '"/work/a file":1:2:3',
    '"relative/a file"',
    '"//server/a file"',
    '"/work/../a file"',
    '"/work/a\tfile"',
    '"/work/a\nfile"',
    '"/work/a\u00a0file"',
    '"/work/a\u202efile"',
    '"/127.0.0.1:80"',
    '"C:\\a?.rs"',
    `"/${"中".repeat(1400)}"`,
  ])
    expect(reviewableFilePath(raw), raw).toBeUndefined();
});
