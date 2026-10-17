import { expect, it } from "vitest";
import { rankMetadata } from "./workspace-metadata-search";

it("orders exact, prefix, substring and subsequence with stable ties and unchanged objects", () => {
  const rows = [
    { id: "fuzzy", name: "provisioned" },
    { id: "substring", name: "my prod server" },
    { id: "exact", name: "prod" },
    { id: "prefix", name: "prod east" },
    { id: "tie", name: "prod west" },
  ];
  expect(
    rankMetadata(rows, "prod", (row) => [row.name]).map((row) => row.id),
  ).toEqual(["exact", "prefix", "tie", "substring", "fuzzy"]);
  expect(rankMetadata(rows, "prd", (row) => [row.name])).toContain(rows[0]);
  expect(rankMetadata(rows, "", (row) => [row.name])).toBe(rows);
});
it("requires every term in metadata fields, preserving Unicode and distinguishing port/username queries", () => {
  const rows = [
    {
      id: "a",
      fields: [
        "生产数据库",
        "admin@db.example:2222",
        "Asia / 数据服务",
        "production",
      ],
    },
    { id: "b", fields: ["生产日志", "root@logs.example:22", "Europe"] },
  ];
  expect(rankMetadata(rows, "prd 2222", (r) => r.fields)).toEqual([rows[0]]);
  expect(rankMetadata(rows, "生库", (r) => r.fields)).toEqual([rows[0]]);
  expect(rankMetadata(rows, "ROOT europe", (r) => r.fields)).toEqual([rows[1]]);
  expect(rankMetadata(rows, "root Asia", (r) => r.fields)).toEqual([]);
  expect(rankMetadata(rows, "ｐｒｏｄ", (r) => r.fields)).toEqual([rows[0]]);
});
it("treats punctuation as literal and bounds long queries without touching source rows", () => {
  const rows = ["user@host:2222", "[::1]", ".*", "<script>"];
  expect(rankMetadata(rows, ".*", (r) => [r])).toEqual([".*"]);
  expect(rankMetadata(rows, "[::1]", (r) => [r])).toEqual(["[::1]"]);
  expect(rankMetadata(rows, "x".repeat(257), (r) => [r])).toEqual([]);
  expect(rows).toEqual(["user@host:2222", "[::1]", ".*", "<script>"]);
});
